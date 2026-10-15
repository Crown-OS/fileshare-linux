//! Finding the other device without telling the room.
//!
//! The payload is the protocol crate's: a 24-byte beacon whose identifier only a device that has
//! already been paired with can resolve. What is here is the two ways of carrying it.

pub mod ble;
pub mod mdns;

pub use ble::BleBeacon;
pub use mdns::MdnsBeacon;
