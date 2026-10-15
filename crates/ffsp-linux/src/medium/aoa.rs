use std::time::Duration;

use ffsp_protocol::medium::{AOA_DESCRIPTION, AOA_MANUFACTURER, AOA_MODEL, AOA_URI, AOA_VERSION};
use nusb::MaybeFuture;
use nusb::transfer::{ControlIn, ControlOut, ControlType, Recipient};
use nusb::{Device, DeviceInfo};

use crate::error::{LinuxError, Result};

/// Google's vendor id, which every device in accessory mode reports.
pub const ACCESSORY_VENDOR: u16 = 0x18D1;
/// Accessory mode, with the debug bridge gone.
pub const ACCESSORY_PRODUCT: u16 = 0x2D00;
/// Accessory mode with the debug bridge kept.
///
/// Worth asking for while developing: entering plain accessory mode detaches adb, so `logcat`
/// disappears at exactly the moment the USB lane comes up.
pub const ACCESSORY_PRODUCT_WITH_ADB: u16 = 0x2D01;

const GET_PROTOCOL: u8 = 51;
const SEND_STRING: u8 = 52;
const START: u8 = 53;
const TIMEOUT: Duration = Duration::from_millis(1_000);

/// The six identity strings, in the order the accessory protocol expects them.
///
/// Five come from the protocol crate so the Android manifest and this cannot drift; the serial
/// is ours and is only used to tell two attached phones apart.
const STRINGS: [&str; 5] = [
    AOA_MANUFACTURER,
    AOA_MODEL,
    AOA_DESCRIPTION,
    AOA_VERSION,
    AOA_URI,
];

/// Whether a device is already answering as an accessory.
pub const fn is_accessory(vendor: u16, product: u16) -> bool {
    vendor == ACCESSORY_VENDOR
        && (product == ACCESSORY_PRODUCT || product == ACCESSORY_PRODUCT_WITH_ADB)
}

/// Asks a device whether it speaks the accessory protocol.
pub async fn protocol_version(device: &Device) -> Result<u16> {
    let answer = device
        .control_in(
            ControlIn {
                control_type: ControlType::Vendor,
                recipient: Recipient::Device,
                request: GET_PROTOCOL,
                value: 0,
                index: 0,
                length: 2,
            },
            TIMEOUT,
        )
        .await?;
    let bytes: [u8; 2] = answer
        .get(..2)
        .and_then(|slice| slice.try_into().ok())
        .ok_or(LinuxError::NotAccessoryCapable)?;
    let version = u16::from_le_bytes(bytes);
    if version == 0 {
        return Err(LinuxError::NotAccessoryCapable);
    }
    Ok(version)
}

/// Puts a device into accessory mode, after which it re-enumerates.
///
/// The strings decide which app Android offers the accessory to. A mismatch with the manifest
/// filter produces no intent and no error on either side: the device comes back, nothing is
/// launched, and nothing anywhere says why.
pub async fn switch_to_accessory(device: &Device, serial: &str) -> Result<()> {
    for (index, value) in STRINGS.iter().enumerate() {
        send_string(device, index as u16, value).await?;
    }
    send_string(device, 5, serial).await?;
    device
        .control_out(
            ControlOut {
                control_type: ControlType::Vendor,
                recipient: Recipient::Device,
                request: START,
                value: 0,
                index: 0,
                data: &[],
            },
            TIMEOUT,
        )
        .await?;
    Ok(())
}

async fn send_string(device: &Device, index: u16, value: &str) -> Result<()> {
    let mut data = Vec::with_capacity(value.len() + 1);
    data.extend_from_slice(value.as_bytes());
    // The protocol carries these as C strings.
    data.push(0);
    device
        .control_out(
            ControlOut {
                control_type: ControlType::Vendor,
                recipient: Recipient::Device,
                request: SEND_STRING,
                value: 0,
                index,
                data: &data,
            },
            TIMEOUT,
        )
        .await?;
    Ok(())
}

/// The udev rule that makes an accessory readable without root.
pub fn udev_rule() -> String {
    format!(
        "# /etc/udev/rules.d/70-crownos-fileshare.rules\n\
         SUBSYSTEM==\"usb\", ATTR{{idVendor}}==\"{ACCESSORY_VENDOR:04x}\", \
         ATTR{{idProduct}}==\"{ACCESSORY_PRODUCT:04x}\", MODE=\"0660\", TAG+=\"uaccess\"\n\
         SUBSYSTEM==\"usb\", ATTR{{idVendor}}==\"{ACCESSORY_VENDOR:04x}\", \
         ATTR{{idProduct}}==\"{ACCESSORY_PRODUCT_WITH_ADB:04x}\", MODE=\"0660\", TAG+=\"uaccess\"\n\
         # then: sudo udevadm control --reload-rules && sudo udevadm trigger\n"
    )
}

/// Every attached device that is not a hub, newest first.
pub fn attached() -> Result<Vec<DeviceInfo>> {
    Ok(nusb::list_devices()
        .wait()?
        .filter(|info| info.class() != 0x09)
        .collect())
}
