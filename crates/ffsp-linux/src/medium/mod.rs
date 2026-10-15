//! Bringing each medium up, and turning it into a lane.
//!
//! Everything here is a wrapper around an operating system interface — BlueZ over D-Bus, the USB
//! host stack, the TCP stack — which is exactly why none of it belongs in the protocol crate.

pub mod aoa;
pub mod bluetooth;
pub mod named;
pub mod tcp;
pub mod usb;

pub use bluetooth::BluetoothMedium;
pub use named::Named;
pub use tcp::TcpMedium;
pub use usb::UsbMedium;

#[cfg(test)]
mod tests;
