use std::path::PathBuf;

use crownui::prelude::Cx;
use ffsp_protocol::engine::{Diagnostic, EngineEvent};
use ffsp_protocol::medium::Medium;
use tokio::runtime::Handle;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::session::{Rig, RigOptions};
use crate::ui::state::{RigCommand, RigEvent, Store};

/// The maximum number of events that one UI wake-up applies.
const EVENT_BURST: usize = 64;

type EventSender = UnboundedSender<RigEvent>;

/// Starts the rig on `rig_runtime` and connects it to `store`.
///
/// The root view calls this function, not a page. Thus navigation does not restart the rig.
pub(crate) fn connect_rig(cx: &mut Cx, store: Store, rig_runtime: &Handle) {
    let (commands, command_receiver) = unbounded_channel();
    let (events, event_receiver) = unbounded_channel();
    rig_runtime.spawn(drive(events, command_receiver));
    store.attach(cx, commands);
    apply_events(cx, store, event_receiver);
}

fn apply_events(cx: &mut Cx, store: Store, mut events: UnboundedReceiver<RigEvent>) {
    let next_burst = async move {
        let mut burst = Vec::with_capacity(EVENT_BURST);
        let open = events.recv_many(&mut burst, EVENT_BURST).await > 0;
        (events, burst, open)
    };
    cx.spawn(next_burst, move |cx, (events, burst, open)| {
        for event in burst {
            store.apply(cx, event);
        }
        if open {
            apply_events(cx, store, events);
        }
    });
}

async fn drive(to_window: EventSender, mut commands: UnboundedReceiver<RigCommand>) {
    let _ = to_window.send(RigEvent::Readiness(readiness().await));

    while let Some(command) = commands.recv().await {
        let outcome = match command {
            RigCommand::Receive => receive(&to_window).await,
            RigCommand::Send(path) => send(&to_window, path).await,
            RigCommand::SelfTest => self_test(&to_window).await,
        };
        if let Err(reason) = outcome {
            let _ = to_window.send(RigEvent::Failed(format!("{reason:#}")));
        }
        let _ = to_window.send(RigEvent::Readiness(readiness().await));
    }
}

async fn receive(to_window: &EventSender) -> anyhow::Result<()> {
    let _ = to_window.send(RigEvent::Status(
        "Bringing up Bluetooth, then Wi-Fi and USB over it…".to_owned(),
    ));
    let mut rig = Rig::start(RigOptions {
        destination: destination(),
        ..RigOptions::default()
    })?;
    rig.bring_up().await?;
    let _ = to_window.send(RigEvent::Status("Waiting for an offer…".to_owned()));
    pump(to_window, rig, None).await
}

async fn send(to_window: &EventSender, path: PathBuf) -> anyhow::Result<()> {
    let _ = to_window.send(RigEvent::Status(format!(
        "Bringing up mediums to send {}…",
        path.display()
    )));
    let mut rig = Rig::start(RigOptions {
        destination: destination(),
        ..RigOptions::default()
    })?;
    let peer = rig.bring_up().await?;
    let _ = to_window.send(RigEvent::Status("Offering…".to_owned()));
    pump(to_window, rig, Some((peer, path))).await
}

async fn self_test(to_window: &EventSender) -> anyhow::Result<()> {
    let _ = to_window.send(RigEvent::Status(
        "Running the stack against itself — no radio involved.".to_owned(),
    ));
    let scratch = std::env::temp_dir().join(format!("ffsp-selftest-{}", std::process::id()));
    let report = crate::selftest::run(3, 64 * 1024 * 1024, scratch.clone()).await?;
    let _ = std::fs::remove_dir_all(&scratch);
    let _ = to_window.send(RigEvent::Finished(Box::new(report)));
    Ok(())
}

/// Drives one transfer, forwarding progress as it happens.
async fn pump(
    to_window: &EventSender,
    rig: Rig,
    offer: Option<(ffsp_protocol::device::DeviceId, PathBuf)>,
) -> anyhow::Result<()> {
    let (engine, mut events) = rig.into_parts();
    if let Some((peer, path)) = offer {
        engine.send(ffsp_protocol::engine::EngineCommand::Offer { peer, path })?;
    }

    while let Some(event) = events.next().await {
        match event {
            EngineEvent::TransferOffered {
                device,
                transfer,
                manifest,
            } => {
                let bytes: u64 = manifest.entries().iter().map(|entry| entry.len()).sum();
                let _ = to_window.send(RigEvent::Status(format!(
                    "Receiving {} MiB",
                    bytes / (1024 * 1024)
                )));
                engine.send(ffsp_protocol::engine::EngineCommand::Accept {
                    peer: device,
                    transfer,
                })?;
            }
            EngineEvent::TransferProgress {
                progress, lanes, ..
            } => {
                let _ = to_window.send(RigEvent::Progress(progress));
                for lane in lanes {
                    let _ = to_window.send(RigEvent::Lane {
                        lane: lane.lane,
                        medium: lane.medium,
                        kbps: lane.bandwidth_kbps,
                        bytes: lane.delivered_bytes,
                    });
                }
            }
            EngineEvent::LaneOpened { medium, .. } => {
                let _ = to_window.send(RigEvent::Status(format!("{medium} joined the session")));
            }
            EngineEvent::TransferReport { report, .. } => {
                let _ = to_window.send(RigEvent::Finished(report));
                return Ok(());
            }
            EngineEvent::Diagnostic(Diagnostic::EngineFailed { reason })
            | EngineEvent::Diagnostic(Diagnostic::LaneFailed { reason, .. }) => {
                let _ = to_window.send(RigEvent::Status(reason));
            }
            _ => {}
        }
    }
    Ok(())
}

async fn readiness() -> Vec<(Medium, bool)> {
    vec![
        (
            Medium::BluetoothClassic,
            ffsp_linux::medium::BluetoothMedium::is_available().await,
        ),
        (
            Medium::WifiInfra,
            ffsp_linux::net::preferred_interface().is_ok(),
        ),
        (
            Medium::UsbAccessory,
            ffsp_linux::medium::UsbMedium::is_available(),
        ),
    ]
}

fn destination() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Downloads")
}
