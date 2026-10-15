use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use ffsp_linux::medium::{BluetoothMedium, TcpMedium, UsbMedium, tcp};
use ffsp_linux::{FileLinkStore, net};
use ffsp_protocol::device::{Capabilities, DeviceClass, DeviceId};
use ffsp_protocol::engine::{
    AdoptedLane, Diagnostic, Engine, EngineCommand, EngineConfig, EngineEvent, EngineEvents,
    Tuning, start,
};
use ffsp_protocol::medium::Medium;
use ffsp_protocol::metrics::TransferReport;
use ffsp_protocol::platform::PowerHint;
use ffsp_protocol::wire::LaneId;

/// How long to wait for the phone to appear on a medium before giving up on it.
const MEDIUM_TIMEOUT: Duration = Duration::from_secs(30);
/// How long the adapter stays visible while waiting to be found.
const DISCOVERABLE_FOR: Duration = Duration::from_secs(180);

/// What the rig should bring up and what it should then do.
#[derive(Debug, Clone)]
pub(crate) struct RigOptions {
    pub(crate) mediums: Vec<Medium>,
    pub(crate) port: u16,
    pub(crate) destination: PathBuf,
    pub(crate) name: String,
}

impl Default for RigOptions {
    fn default() -> Self {
        Self {
            mediums: vec![
                Medium::BluetoothClassic,
                Medium::WifiInfra,
                Medium::UsbAccessory,
            ],
            port: tcp::DEFAULT_PORT,
            destination: PathBuf::from("."),
            name: hostname(),
        }
    }
}

/// The Linux side of one benchmark run.
///
/// This end is always the session initiator, whichever medium happened to connect first. Someone
/// has to be, because the initiator is the only allocator of lane identifiers, and a laptop
/// driving a benchmark is the obvious candidate.
#[derive(Debug)]
pub(crate) struct Rig {
    engine: Engine,
    events: EngineEvents,
    options: RigOptions,
    tcp: Option<Arc<TcpMedium>>,
    peer: Option<DeviceId>,
}

impl Rig {
    pub(crate) fn start(options: RigOptions) -> Result<Self> {
        let store = Arc::new(FileLinkStore::open(FileLinkStore::default_path())?);
        let identity = store.identity()?;
        std::fs::create_dir_all(&options.destination)
            .with_context(|| format!("creating {}", options.destination.display()))?;

        let config = EngineConfig::new(identity, store, options.destination.clone())
            .with_name(options.name.clone())
            .with_class(DeviceClass::Laptop)
            .with_capabilities(capabilities_of(&options.mediums))
            // Balanced caps bulk lanes at two, which would quietly drop one of the three
            // mediums and read as a slow medium rather than a missing one.
            .with_tuning(Tuning::for_power(PowerHint::Unconstrained));

        let (engine, events) = start(config);
        Ok(Self {
            engine,
            events,
            options,
            tcp: None,
            peer: None,
        })
    }

    /// Brings up every requested medium and joins them all to one session.
    ///
    /// Bluetooth goes first on purpose: it needs no network configuration and no cable, only an
    /// agent that says yes. Once it carries control frames the other two can be arranged over
    /// it — this end sends its address, and asks the phone to become a USB accessory — which is
    /// what makes the whole setup unattended.
    pub(crate) async fn bring_up(&mut self) -> Result<DeviceId> {
        let first = *self
            .options
            .mediums
            .first()
            .ok_or_else(|| anyhow!("no mediums were asked for"))?;
        let peer = self.open_first(first).await?;
        self.peer = Some(peer);

        for medium in self.options.mediums.clone().into_iter().skip(1) {
            if let Err(error) = self.join(peer, medium).await {
                // One medium failing is a result, not a failure: the report should say the rig
                // ran on two lanes rather than pretend it ran on three.
                tracing::warn!(%medium, "could not bring up: {error:#}");
            }
        }
        Ok(peer)
    }

    async fn open_first(&mut self, medium: Medium) -> Result<DeviceId> {
        let channel = match medium {
            Medium::BluetoothClassic => {
                let mut bluetooth = BluetoothMedium::open(DISCOVERABLE_FOR).await?;
                tracing::info!(
                    adapter = bluetooth.adapter_name(),
                    "waiting for the phone to connect over Bluetooth"
                );
                let (channel, address) = bluetooth.accept().await?;
                tracing::info!(%address, "Bluetooth connected");
                channel
            }
            Medium::WifiInfra => {
                let listener = self.listener().await?;
                tracing::info!(port = self.options.port, "waiting for the phone over Wi-Fi");
                let (channel, peer) = listener.accept().await?;
                tracing::info!(%peer, "Wi-Fi connected");
                channel
            }
            Medium::UsbAccessory => UsbMedium::new(&self.options.name).open().await?,
            other => bail!("{other} cannot be the first medium"),
        };

        self.engine.send(EngineCommand::AttachLane {
            medium,
            endpoint: describe(medium),
            channel: Box::new(channel),
            initiate: true,
            adopt: None,
        })?;

        loop {
            match self.next_event().await? {
                EngineEvent::LinkSettled { device, accepted } if accepted => return Ok(device),
                EngineEvent::Diagnostic(Diagnostic::LaneFailed { reason, .. }) => {
                    bail!("the first lane failed: {reason}")
                }
                _ => {}
            }
        }
    }

    /// Adds a medium to a session that already exists.
    async fn join(&mut self, peer: DeviceId, medium: Medium) -> Result<()> {
        let endpoint = match medium {
            Medium::WifiInfra => {
                let interface = net::preferred_interface()?;
                format!("{}:{}", interface.address, self.options.port)
            }
            _ => describe(medium),
        };

        self.engine.send(EngineCommand::InviteLane {
            peer,
            medium,
            endpoint: endpoint.clone(),
        })?;
        let lane = self.await_invite(medium).await?;

        let channel = tokio::time::timeout(MEDIUM_TIMEOUT, async {
            match medium {
                Medium::WifiInfra => {
                    let listener = self.listener().await?;
                    let (channel, _) = listener.accept().await?;
                    Ok::<_, anyhow::Error>(channel)
                }
                Medium::UsbAccessory => Ok(UsbMedium::new(&self.options.name).open().await?),
                Medium::BluetoothClassic => {
                    let mut bluetooth = BluetoothMedium::open(DISCOVERABLE_FOR).await?;
                    let (channel, _) = bluetooth.accept().await?;
                    Ok(channel)
                }
                other => bail!("{other} is not supported on this platform"),
            }
        })
        .await
        .with_context(|| format!("{medium} did not come up within {MEDIUM_TIMEOUT:?}"))??;

        self.engine.send(EngineCommand::AttachLane {
            medium,
            endpoint,
            channel: Box::new(channel),
            initiate: true,
            adopt: Some(AdoptedLane { peer, lane }),
        })?;
        tracing::info!(%medium, lane = lane.as_u16(), "lane joined the session");
        Ok(())
    }

    async fn listener(&mut self) -> Result<Arc<TcpMedium>> {
        if let Some(listener) = &self.tcp {
            return Ok(Arc::clone(listener));
        }
        let interface = net::preferred_interface()?;
        let address = SocketAddr::new(IpAddr::V4(interface.address), self.options.port);
        let listener = Arc::new(TcpMedium::bind(address).await?);
        self.tcp = Some(Arc::clone(&listener));
        Ok(listener)
    }

    async fn await_invite(&mut self, medium: Medium) -> Result<LaneId> {
        loop {
            match self.next_event().await? {
                EngineEvent::LaneInvited {
                    lane,
                    medium: invited,
                    ..
                } if invited == medium => return Ok(lane),
                EngineEvent::Diagnostic(Diagnostic::EngineFailed { reason }) => {
                    bail!("the engine refused the invite: {reason}")
                }
                _ => {}
            }
        }
    }

    /// Sends a file and waits for this end's report.
    pub(crate) async fn send(&mut self, peer: DeviceId, path: PathBuf) -> Result<TransferReport> {
        self.engine.send(EngineCommand::Offer { peer, path })?;
        self.await_report().await
    }

    /// Accepts whatever is offered and waits for this end's report.
    pub(crate) async fn receive(&mut self) -> Result<TransferReport> {
        loop {
            match self.next_event().await? {
                EngineEvent::TransferOffered {
                    device,
                    transfer,
                    manifest,
                } => {
                    let total: u64 = manifest.entries().iter().map(|entry| entry.len()).sum();
                    tracing::info!(bytes = total, "accepting");
                    self.engine.send(EngineCommand::Accept {
                        peer: device,
                        transfer,
                    })?;
                }
                EngineEvent::TransferReport { report, .. } => return Ok(*report),
                EngineEvent::Diagnostic(Diagnostic::EngineFailed { reason }) => {
                    bail!("the transfer failed: {reason}")
                }
                _ => {}
            }
        }
    }

    async fn await_report(&mut self) -> Result<TransferReport> {
        loop {
            match self.next_event().await? {
                EngineEvent::TransferReport { report, .. } => return Ok(*report),
                EngineEvent::Diagnostic(Diagnostic::EngineFailed { reason }) => {
                    bail!("the transfer failed: {reason}")
                }
                _ => {}
            }
        }
    }

    async fn next_event(&mut self) -> Result<EngineEvent> {
        self.events
            .next()
            .await
            .ok_or_else(|| anyhow!("the engine stopped"))
    }

    /// Takes the event stream, for a caller that wants to watch it themselves.
    pub(crate) fn into_parts(self) -> (Engine, EngineEvents) {
        (self.engine, self.events)
    }
}

fn capabilities_of(mediums: &[Medium]) -> Capabilities {
    let mut capabilities = Capabilities::empty();
    for medium in mediums {
        capabilities |= match medium {
            Medium::WifiInfra => Capabilities::WIFI_INFRA,
            Medium::WifiDirect => Capabilities::WIFI_DIRECT,
            Medium::UsbAccessory => Capabilities::USB_ACCESSORY,
            Medium::BluetoothLe => Capabilities::BLUETOOTH_LE,
            Medium::BluetoothClassic => Capabilities::BLUETOOTH_CLASSIC,
            Medium::Loopback => Capabilities::empty(),
        };
    }
    if ffsp_protocol::crypto::has_aes_hardware() {
        capabilities |= Capabilities::AES_HARDWARE;
    }
    capabilities
}

fn describe(medium: Medium) -> String {
    medium.to_string()
}

fn hostname() -> String {
    std::fs::read_to_string("/proc/sys/kernel/hostname")
        .map(|name| name.trim().to_owned())
        .unwrap_or_else(|_| "linux".to_owned())
}
