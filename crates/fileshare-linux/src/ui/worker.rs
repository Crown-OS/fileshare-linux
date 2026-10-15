use std::path::PathBuf;

use ffsp_protocol::engine::{Diagnostic, EngineEvent};
use ffsp_protocol::medium::Medium;
use xilem::ViewCtx;
use xilem::core::{MessageProxy, NoElement, View};
use xilem::tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use xilem::view::worker_raw;

use crate::session::{Rig, RigOptions};
use crate::ui::state::{AppState, RigCommand, RigEvent};

/// Bridges the engine to the window.
///
/// Mounted at the root rather than inside a page, so navigating between pages does not restart
/// the rig. Samples are batched into one message per tick: every message sent through the proxy
/// costs a full rebuild of the view tree, so one per medium per tick would multiply that by the
/// number of mediums for no extra information.
pub(crate) fn rig_worker() -> impl View<AppState, (), ViewCtx, Element = NoElement> + Send + Sync {
    worker_raw(
        |proxy: MessageProxy<RigEvent>, commands: UnboundedReceiver<RigCommand>| {
            drive(proxy, commands)
        },
        |state: &mut AppState, sender: UnboundedSender<RigCommand>| state.attach(sender),
        |state: &mut AppState, event: RigEvent| state.apply(event),
    )
}

async fn drive(proxy: MessageProxy<RigEvent>, mut commands: UnboundedReceiver<RigCommand>) {
    let _ = proxy.message(RigEvent::Readiness(readiness().await));

    while let Some(command) = commands.recv().await {
        let outcome = match command {
            RigCommand::Receive => receive(&proxy).await,
            RigCommand::Send(path) => send(&proxy, path).await,
            RigCommand::SelfTest => self_test(&proxy).await,
        };
        if let Err(reason) = outcome {
            let _ = proxy.message(RigEvent::Failed(format!("{reason:#}")));
        }
        let _ = proxy.message(RigEvent::Readiness(readiness().await));
    }
}

async fn receive(proxy: &MessageProxy<RigEvent>) -> anyhow::Result<()> {
    let _ = proxy.message(RigEvent::Status(
        "Bringing up Bluetooth, then Wi-Fi and USB over it…".to_owned(),
    ));
    let mut rig = Rig::start(RigOptions {
        destination: destination(),
        ..RigOptions::default()
    })?;
    rig.bring_up().await?;
    let _ = proxy.message(RigEvent::Status("Waiting for an offer…".to_owned()));
    pump(proxy, rig, None).await
}

async fn send(proxy: &MessageProxy<RigEvent>, path: PathBuf) -> anyhow::Result<()> {
    let _ = proxy.message(RigEvent::Status(format!(
        "Bringing up mediums to send {}…",
        path.display()
    )));
    let mut rig = Rig::start(RigOptions {
        destination: destination(),
        ..RigOptions::default()
    })?;
    let peer = rig.bring_up().await?;
    let _ = proxy.message(RigEvent::Status("Offering…".to_owned()));
    pump(proxy, rig, Some((peer, path))).await
}

async fn self_test(proxy: &MessageProxy<RigEvent>) -> anyhow::Result<()> {
    let _ = proxy.message(RigEvent::Status(
        "Running the stack against itself — no radio involved.".to_owned(),
    ));
    let scratch = std::env::temp_dir().join(format!("ffsp-selftest-{}", std::process::id()));
    let report = crate::selftest::run(3, 64 * 1024 * 1024, scratch.clone()).await?;
    let _ = std::fs::remove_dir_all(&scratch);
    let _ = proxy.message(RigEvent::Finished(Box::new(report)));
    Ok(())
}

/// Drives one transfer, forwarding progress as it happens.
async fn pump(
    proxy: &MessageProxy<RigEvent>,
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
                let _ = proxy.message(RigEvent::Status(format!(
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
                let _ = proxy.message(RigEvent::Progress(progress));
                for lane in lanes {
                    let _ = proxy.message(RigEvent::Lane {
                        lane: lane.lane,
                        medium: lane.medium,
                        kbps: lane.bandwidth_kbps,
                        bytes: lane.delivered_bytes,
                    });
                }
            }
            EngineEvent::LaneOpened { medium, .. } => {
                let _ = proxy.message(RigEvent::Status(format!("{medium} joined the session")));
            }
            EngineEvent::TransferReport { report, .. } => {
                let _ = proxy.message(RigEvent::Finished(report));
                return Ok(());
            }
            EngineEvent::Diagnostic(Diagnostic::EngineFailed { reason })
            | EngineEvent::Diagnostic(Diagnostic::LaneFailed { reason, .. }) => {
                let _ = proxy.message(RigEvent::Status(reason));
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
