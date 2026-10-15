use std::time::Duration;

use ffsp_protocol::medium::{
    FramedReceiver, FramedSender, LaneChannel, LaneProfile, RFCOMM_MAX_TRANSFER,
};
use ffsp_protocol::wire::LaneId;
use nusb::transfer::{Bulk, In, Out};
use nusb::{DeviceInfo, MaybeFuture};

use crate::error::{LinuxError, Result};
use crate::medium::aoa;
use crate::medium::named::Named;

/// How long to wait for a device to come back after being told to become an accessory.
const REENUMERATION_TIMEOUT: Duration = Duration::from_secs(5);
const REENUMERATION_POLL: Duration = Duration::from_millis(100);

/// Brings up the USB lane, including putting the phone into accessory mode.
///
/// The phone is a peripheral and this machine is the host, so all of the setup is on this side:
/// find the device, ask whether it speaks the accessory protocol, hand it the identity strings,
/// tell it to switch, and wait for it to come back as something else entirely.
#[derive(Debug, Default)]
pub struct UsbMedium {
    serial: String,
}

impl UsbMedium {
    pub fn new(serial: impl Into<String>) -> Self {
        Self {
            serial: serial.into(),
        }
    }

    /// Whether anything is attached that could become an accessory.
    pub fn is_available() -> bool {
        aoa::attached()
            .map(|devices| !devices.is_empty())
            .unwrap_or(false)
    }

    /// Opens a lane, switching the phone into accessory mode first if it is not already.
    pub async fn open(&self) -> Result<LaneChannel> {
        if let Some(info) = find_accessory()? {
            return self.claim(&info);
        }

        let candidates = aoa::attached()?;
        for info in candidates {
            let Ok(device) = info.open().await else {
                continue;
            };
            if aoa::protocol_version(&device).await.is_err() {
                continue;
            }
            aoa::switch_to_accessory(&device, &self.serial).await?;
            drop(device);
            return self.claim(&await_accessory().await?);
        }
        Err(LinuxError::NoAndroidDevice)
    }

    fn claim(&self, info: &DeviceInfo) -> Result<LaneChannel> {
        let device = info.open().wait().map_err(|error| {
            if matches!(error.kind(), nusb::ErrorKind::PermissionDenied) {
                LinuxError::AccessoryPermissionDenied {
                    rule: aoa::udev_rule(),
                }
            } else {
                LinuxError::Usb(error)
            }
        })?;
        let interface = device.claim_interface(0).wait()?;

        // The accessory interface has exactly one bulk pair, and it is the whole lane.
        let write = interface
            .endpoint::<Bulk, Out>(bulk_address(&interface, false)?)?
            .writer(RFCOMM_MAX_TRANSFER);
        let read = interface
            .endpoint::<Bulk, In>(bulk_address(&interface, true)?)?
            .reader(RFCOMM_MAX_TRANSFER);

        let profile = LaneProfile::USB_ACCESSORY;
        Ok(LaneChannel::new(
            LaneId::HANDSHAKE,
            profile,
            Box::new(FramedSender::new(
                profile,
                Named::new("usb accessory out", write),
            )),
            Box::new(FramedReceiver::new(
                profile,
                Named::new("usb accessory in", read),
            )),
        ))
    }
}

fn bulk_address(interface: &nusb::Interface, inbound: bool) -> Result<u8> {
    let descriptors = interface.descriptors();
    for alternate in descriptors {
        for endpoint in alternate.endpoints() {
            let address = endpoint.address();
            let is_in = address & 0x80 != 0;
            if endpoint.transfer_type() == nusb::descriptors::TransferType::Bulk && is_in == inbound
            {
                return Ok(address);
            }
        }
    }
    Err(LinuxError::MissingBulkEndpoint {
        direction: if inbound { "in" } else { "out" },
    })
}

fn find_accessory() -> Result<Option<DeviceInfo>> {
    Ok(nusb::list_devices()
        .wait()?
        .find(|info| aoa::is_accessory(info.vendor_id(), info.product_id())))
}

/// Waits for the device to come back under its accessory identity.
async fn await_accessory() -> Result<DeviceInfo> {
    let deadline = tokio::time::Instant::now() + REENUMERATION_TIMEOUT;
    loop {
        if let Some(info) = find_accessory()? {
            return Ok(info);
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(LinuxError::NoAndroidDevice);
        }
        tokio::time::sleep(REENUMERATION_POLL).await;
    }
}
