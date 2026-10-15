//! What the FFSP engine needs from Linux: the transports, the radios, and somewhere to keep a
//! pairing.
//!
//! The protocol crate deliberately has no transports. This is the other half: it opens mediums
//! and hands the engine lanes that already work.

pub mod discovery;
pub mod error;
pub mod medium;
pub mod net;
pub mod store;

pub use error::{LinuxError, Result};
pub use store::FileLinkStore;
