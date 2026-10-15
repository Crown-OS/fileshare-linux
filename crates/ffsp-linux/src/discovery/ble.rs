use std::collections::BTreeMap;
use std::time::Duration;

use bluer::adv::Advertisement as BleAdvertisement;
use bluer::adv::AdvertisementHandle;
use bluer::{Adapter, DiscoveryFilter, DiscoveryTransport, Session, Uuid};
use ffsp_protocol::device::{Capabilities, DeviceClass};
use ffsp_protocol::discovery::{Advertisement, Epoch, SERVICE_UUID_16, ephemeral_id};
use ffsp_protocol::link::LinkSecret;
use futures_util::StreamExt;

use crate::error::Result;

/// The 16-bit service the beacon rides in, widened to the full Bluetooth base UUID.
///
/// Sixteen bits rather than a full UUID because the legacy advertising packet has 31 bytes and
/// the beacon needs 24 of them.
pub fn service_uuid() -> Uuid {
    Uuid::from_u128(0x0000_0000_0000_1000_8000_0080_5F9B_34FB | ((SERVICE_UUID_16 as u128) << 96))
}

/// A beacon seen over the air.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BleSighting {
    pub advertisement: Advertisement,
    pub address: bluer::Address,
    pub rssi: Option<i16>,
}

/// Advertises and scans the same 24 bytes mDNS carries.
#[derive(Debug)]
pub struct BleBeacon {
    adapter: Adapter,
    handle: Option<AdvertisementHandle>,
}

impl BleBeacon {
    pub async fn new() -> Result<Self> {
        let session = Session::new().await?;
        let adapter = session.default_adapter().await?;
        adapter.set_powered(true).await?;
        Ok(Self {
            adapter,
            handle: None,
        })
    }

    pub async fn advertise(
        &mut self,
        secret: &LinkSecret,
        class: DeviceClass,
        capabilities: Capabilities,
        epoch: Epoch,
    ) -> Result<()> {
        let identifier = ephemeral_id(secret, epoch);
        let beacon = Advertisement::new(class, identifier, epoch, capabilities);
        let mut service_data = BTreeMap::new();
        service_data.insert(service_uuid(), beacon.to_bytes().to_vec());

        self.handle = Some(
            self.adapter
                .advertise(BleAdvertisement {
                    service_data,
                    discoverable: Some(true),
                    // No name: the beacon says nothing about this device to anyone who cannot
                    // already resolve it.
                    local_name: None,
                    ..Default::default()
                })
                .await?,
        );
        Ok(())
    }

    pub fn stop_advertising(&mut self) {
        self.handle = None;
    }

    /// Scans for the service, for as long as asked.
    pub async fn scan(&self, duration: Duration) -> Result<Vec<BleSighting>> {
        self.adapter
            .set_discovery_filter(DiscoveryFilter {
                uuids: [service_uuid()].into_iter().collect(),
                transport: DiscoveryTransport::Le,
                duplicate_data: false,
                ..Default::default()
            })
            .await?;
        let mut events = self.adapter.discover_devices().await?;
        let deadline = tokio::time::Instant::now() + duration;
        let mut found = Vec::new();

        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                break;
            }
            let Ok(Some(event)) = tokio::time::timeout(remaining, events.next()).await else {
                break;
            };
            let bluer::AdapterEvent::DeviceAdded(address) = event else {
                continue;
            };
            let Ok(device) = self.adapter.device(address) else {
                continue;
            };
            let Ok(Some(data)) = device.service_data().await else {
                continue;
            };
            let Some(payload) = data.get(&service_uuid()) else {
                continue;
            };
            let Ok(advertisement) = Advertisement::parse(payload) else {
                continue;
            };
            found.push(BleSighting {
                advertisement,
                address,
                rssi: device.rssi().await.ok().flatten(),
            });
        }
        Ok(found)
    }
}
