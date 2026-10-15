use std::collections::HashMap;
use std::net::Ipv4Addr;

use ffsp_protocol::device::{Capabilities, DeviceClass};
use ffsp_protocol::discovery::{
    Advertisement, EphemeralId, Epoch, SERVICE_TYPE, TxtRecord, ephemeral_id,
};
use ffsp_protocol::link::LinkSecret;
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};

use crate::error::Result;

/// A resolved peer, as mDNS saw it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MdnsSighting {
    pub advertisement: Advertisement,
    pub address: Ipv4Addr,
    pub port: u16,
}

impl MdnsSighting {
    pub fn endpoint(&self) -> String {
        format!("{}:{}", self.address, self.port)
    }
}

/// Publishes and watches the beacon on the local network.
///
/// The same 24 bytes travel here and over Bluetooth low energy, hex-encoded into a TXT record,
/// so a device seen on both planes resolves to one peer rather than two.
pub struct MdnsBeacon {
    daemon: ServiceDaemon,
    registered: Option<String>,
}

impl core::fmt::Debug for MdnsBeacon {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("MdnsBeacon")
            .field("registered", &self.registered)
            .finish_non_exhaustive()
    }
}

impl MdnsBeacon {
    pub fn new() -> Result<Self> {
        Ok(Self {
            daemon: ServiceDaemon::new()?,
            registered: None,
        })
    }

    /// Announces this device under a name derived from its beacon, not from its hostname.
    ///
    /// A hostname would identify the machine to anyone listening; the ephemeral identifier only
    /// means something to a device that has already been paired with.
    pub fn publish(
        &mut self,
        secret: &LinkSecret,
        class: DeviceClass,
        capabilities: Capabilities,
        epoch: Epoch,
        address: Ipv4Addr,
        port: u16,
    ) -> Result<()> {
        let identifier = ephemeral_id(secret, epoch);
        let advertisement = Advertisement::new(class, identifier, epoch, capabilities);
        let record = TxtRecord {
            advertisement,
            port,
        };
        let properties: HashMap<String, String> = record
            .to_pairs()
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect();

        let instance = short_name(&identifier);
        let host = format!("{instance}.local.");
        let info = ServiceInfo::new(
            SERVICE_TYPE_DOMAIN,
            &instance,
            &host,
            &address.to_string()[..],
            port,
            properties,
        )?;
        self.unpublish();
        self.daemon.register(info)?;
        self.registered = Some(format!("{instance}.{SERVICE_TYPE_DOMAIN}"));
        Ok(())
    }

    pub fn unpublish(&mut self) {
        if let Some(fullname) = self.registered.take() {
            let _ = self.daemon.unregister(&fullname);
        }
    }

    /// Watches for peers, handing back whatever the beacon said about each.
    ///
    /// Resolution to a device happens above this, against the pairings this machine holds; an
    /// unknown beacon is indistinguishable from noise and is simply passed on.
    pub fn browse(&self) -> Result<mdns_sd::Receiver<ServiceEvent>> {
        Ok(self.daemon.browse(SERVICE_TYPE_DOMAIN)?)
    }

    /// Turns a resolution into a beacon, or `None` if it was not one of ours.
    pub fn read(event: &ServiceEvent) -> Option<MdnsSighting> {
        let ServiceEvent::ServiceResolved(resolved) = event else {
            return None;
        };
        let record = TxtRecord::parse(|key| resolved.get_property_val_str(key)).ok()?;
        let address = resolved.get_addresses_v4().into_iter().next()?;
        Some(MdnsSighting {
            advertisement: record.advertisement,
            address,
            port: record.port,
        })
    }
}

impl Drop for MdnsBeacon {
    fn drop(&mut self) {
        self.unpublish();
        let _ = self.daemon.shutdown();
    }
}

/// The protocol crate names the service; mDNS wants it with a trailing domain.
const SERVICE_TYPE_DOMAIN: &str = concat!("_crowncrate._tcp", ".local.");

const _: () = assert!(SERVICE_TYPE.len() == "_crowncrate._tcp".len());

fn short_name(identifier: &EphemeralId) -> String {
    identifier
        .as_bytes()
        .iter()
        .take(6)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
