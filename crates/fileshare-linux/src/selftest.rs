use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Result, anyhow, bail};
use ffsp_protocol::control::TransferId;
use ffsp_protocol::device::{Capabilities, DeviceClass, DeviceId};
use ffsp_protocol::engine::{
    AdoptedLane, Diagnostic, Engine, EngineCommand, EngineConfig, EngineEvent, EngineEvents,
    Tuning, start,
};
use ffsp_protocol::link::{LinkRecord, LinkStore, LinkStoreError, generate_identity};
use ffsp_protocol::medium::{Medium, loopback_channel_pair};
use ffsp_protocol::metrics::TransferReport;
use ffsp_protocol::platform::PowerHint;
use ffsp_protocol::wire::LaneId;

/// The three mediums a real run uses, in the order the rig brings them up.
pub(crate) const MEDIUMS: [Medium; 3] = [
    Medium::BluetoothClassic,
    Medium::WifiInfra,
    Medium::UsbAccessory,
];

/// Runs the whole stack against itself, with no radio involved.
///
/// Worth having for its own sake: it separates "the protocol is wrong" from "the Bluetooth
/// adapter is wrong", which are otherwise the same red light. It also gives the rig something
/// to check after every change without a phone on the desk.
pub(crate) async fn run(lanes: usize, bytes: u64, directory: PathBuf) -> Result<TransferReport> {
    std::fs::create_dir_all(&directory)?;
    let source = directory.join("selftest-source.bin");
    let inbox = directory.join("selftest-inbox");
    std::fs::create_dir_all(&inbox)?;
    write_payload(&source, bytes)?;

    let (sender, mut sender_events) = engine("laptop", DeviceClass::Laptop, directory.join("out"))?;
    let (receiver, mut receiver_events) = engine("phone", DeviceClass::Phone, inbox.clone())?;

    let (near, far) = loopback_channel_pair(LaneId::HANDSHAKE, 8 * 1024 * 1024);
    attach(&sender, MEDIUMS[0], near, true, None)?;
    attach(&receiver, MEDIUMS[0], far, false, None)?;

    let peer_of_sender = await_link(&mut sender_events).await?;
    let peer_of_receiver = await_link(&mut receiver_events).await?;

    for medium in MEDIUMS.iter().copied().take(lanes.clamp(1, 3)).skip(1) {
        sender.send(EngineCommand::InviteLane {
            peer: peer_of_sender,
            medium,
            endpoint: format!("loopback:{medium}"),
        })?;
        let lane = await_invite(&mut sender_events, medium).await?;
        let echoed = await_invite(&mut receiver_events, medium).await?;
        if lane != echoed {
            bail!("the two ends disagreed about the lane identifier");
        }
        let (near, far) = loopback_channel_pair(LaneId::HANDSHAKE, 8 * 1024 * 1024);
        attach(
            &sender,
            medium,
            near,
            true,
            Some(AdoptedLane {
                peer: peer_of_sender,
                lane,
            }),
        )?;
        attach(
            &receiver,
            medium,
            far,
            false,
            Some(AdoptedLane {
                peer: peer_of_receiver,
                lane,
            }),
        )?;
    }

    let accepting = tokio::spawn(async move {
        loop {
            match receiver_events.next().await {
                Some(EngineEvent::TransferOffered { device, .. }) => {
                    receiver.send(EngineCommand::Accept {
                        peer: device,
                        transfer: TransferId::new(1),
                    })?;
                }
                Some(EngineEvent::TransferReport { report, .. }) => {
                    return Ok::<_, anyhow::Error>(*report);
                }
                Some(EngineEvent::Diagnostic(Diagnostic::EngineFailed { reason })) => {
                    bail!("the receiver failed: {reason}")
                }
                Some(_) => {}
                None => bail!("the receiver stopped"),
            }
        }
    });

    sender.send(EngineCommand::Offer {
        peer: peer_of_sender,
        path: source.clone(),
    })?;

    let sent = tokio::time::timeout(Duration::from_secs(300), async {
        loop {
            match sender_events.next().await {
                Some(EngineEvent::TransferReport { report, .. }) => {
                    return Ok::<_, anyhow::Error>(*report);
                }
                Some(EngineEvent::Diagnostic(Diagnostic::EngineFailed { reason })) => {
                    bail!("the sender failed: {reason}")
                }
                Some(_) => {}
                None => bail!("the sender stopped"),
            }
        }
    })
    .await??;

    let received = tokio::time::timeout(Duration::from_secs(60), accepting).await???;

    let landed = inbox.join("selftest-source.bin");
    let original = std::fs::metadata(&source)?.len();
    let arrived = std::fs::metadata(&landed)?.len();
    if original != arrived {
        bail!("{arrived} bytes arrived of {original}");
    }
    if !same_contents(&source, &landed)? {
        bail!("the payload did not survive the trip");
    }
    let _ = std::fs::remove_file(&source);
    let _ = std::fs::remove_file(&landed);
    let _ = sent;
    Ok(received)
}

fn engine(name: &str, class: DeviceClass, destination: PathBuf) -> Result<(Engine, EngineEvents)> {
    std::fs::create_dir_all(&destination)?;
    let config = EngineConfig::new(
        generate_identity()?,
        Arc::new(EphemeralLinks::default()),
        destination,
    )
    .with_name(name)
    .with_class(class)
    .with_capabilities(
        Capabilities::WIFI_INFRA | Capabilities::USB_ACCESSORY | Capabilities::BLUETOOTH_CLASSIC,
    )
    .with_tuning(Tuning::for_power(PowerHint::Unconstrained));
    Ok(start(config))
}

fn attach(
    engine: &Engine,
    medium: Medium,
    channel: ffsp_protocol::medium::LaneChannel,
    initiate: bool,
    adopt: Option<AdoptedLane>,
) -> Result<()> {
    engine.send(EngineCommand::AttachLane {
        medium,
        endpoint: format!("loopback:{medium}"),
        channel: Box::new(channel),
        initiate,
        adopt,
    })?;
    Ok(())
}

async fn await_link(events: &mut EngineEvents) -> Result<DeviceId> {
    loop {
        match events.next().await {
            Some(EngineEvent::LinkSettled { device, accepted }) if accepted => return Ok(device),
            Some(EngineEvent::Diagnostic(Diagnostic::LaneFailed { reason, .. })) => {
                bail!("a lane failed during pairing: {reason}")
            }
            Some(_) => {}
            None => bail!("the engine stopped before pairing"),
        }
    }
}

async fn await_invite(events: &mut EngineEvents, medium: Medium) -> Result<LaneId> {
    loop {
        match events.next().await {
            Some(EngineEvent::LaneInvited {
                lane,
                medium: invited,
                ..
            }) if invited == medium => return Ok(lane),
            Some(EngineEvent::Diagnostic(Diagnostic::EngineFailed { reason })) => {
                bail!("the invite was refused: {reason}")
            }
            Some(_) => {}
            None => bail!("the engine stopped during bring-up"),
        }
    }
}

/// Deterministic filler, so a failure is reproducible without keeping the file.
fn write_payload(path: &std::path::Path, bytes: u64) -> Result<()> {
    use std::io::Write;

    let mut file = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut state = 0x2545_F491_4F6C_DD1Du64;
    let mut block = vec![0u8; 1 << 16];
    let mut written = 0u64;
    while written < bytes {
        for slot in block.iter_mut() {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            *slot = (state >> 33) as u8;
        }
        let take = usize::try_from((bytes - written).min(block.len() as u64))
            .map_err(|_| anyhow!("payload too large"))?;
        file.write_all(block.get(..take).unwrap_or_default())?;
        written += take as u64;
    }
    file.flush()?;
    Ok(())
}

fn same_contents(left: &std::path::Path, right: &std::path::Path) -> Result<bool> {
    use std::io::Read;

    let mut left = std::io::BufReader::new(std::fs::File::open(left)?);
    let mut right = std::io::BufReader::new(std::fs::File::open(right)?);
    let mut a = vec![0u8; 1 << 16];
    let mut b = vec![0u8; 1 << 16];
    loop {
        let read = left.read(&mut a)?;
        let mirrored = right.read(&mut b)?;
        if read != mirrored {
            return Ok(false);
        }
        if read == 0 {
            return Ok(true);
        }
        if a.get(..read) != b.get(..read) {
            return Ok(false);
        }
    }
}

/// Pairings that live only as long as the test.
#[derive(Debug, Default)]
struct EphemeralLinks {
    records: std::sync::Mutex<std::collections::BTreeMap<DeviceId, LinkRecord>>,
}

impl LinkStore for EphemeralLinks {
    fn load(&self, device: &DeviceId) -> Result<Option<LinkRecord>, LinkStoreError> {
        Ok(self.lock()?.get(device).cloned())
    }

    fn store(&self, record: &LinkRecord) -> Result<(), LinkStoreError> {
        self.lock()?.insert(record.device, record.clone());
        Ok(())
    }

    fn remove(&self, device: &DeviceId) -> Result<(), LinkStoreError> {
        self.lock()?.remove(device);
        Ok(())
    }

    fn all(&self) -> Result<Vec<LinkRecord>, LinkStoreError> {
        Ok(self.lock()?.values().cloned().collect())
    }
}

impl EphemeralLinks {
    fn lock(
        &self,
    ) -> Result<
        std::sync::MutexGuard<'_, std::collections::BTreeMap<DeviceId, LinkRecord>>,
        LinkStoreError,
    > {
        self.records
            .lock()
            .map_err(|_| LinkStoreError::Unavailable("poisoned".to_owned()))
    }
}
