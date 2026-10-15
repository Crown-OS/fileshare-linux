use thiserror::Error;

use ffsp_protocol::medium::Medium;

#[derive(Debug, Error)]
pub enum LinuxError {
    #[error("no usable network interface: every candidate was a loopback or a tunnel")]
    NoNetworkInterface,

    #[error("no Bluetooth adapter is present")]
    NoBluetoothAdapter,

    #[error("no Android device is attached over USB")]
    NoAndroidDevice,

    #[error("the attached device did not answer the accessory protocol")]
    NotAccessoryCapable,

    #[error(
        "the device re-enumerated in accessory mode but is not readable. Install a udev rule:\n\
         {rule}"
    )]
    AccessoryPermissionDenied { rule: String },

    #[error("the accessory interface has no {direction} bulk endpoint")]
    MissingBulkEndpoint { direction: &'static str },

    #[error("{medium} is not available: {reason}")]
    Unavailable { medium: Medium, reason: String },

    #[error("the stored link records are corrupt at record {ordinal}")]
    CorruptStore { ordinal: usize },

    #[error(transparent)]
    Bluetooth(#[from] bluer::Error),

    #[error(transparent)]
    Usb(#[from] nusb::Error),

    #[error(transparent)]
    UsbTransfer(#[from] nusb::transfer::TransferError),

    #[error(transparent)]
    Mdns(#[from] mdns_sd::Error),

    #[error(transparent)]
    Discovery(#[from] ffsp_protocol::discovery::DiscoveryError),

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, LinuxError>;
