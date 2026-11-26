//! Minimal HID++ 2.0 client for the Logitech PRO X Superlight.
//!
//! Design rules (see `safety.rs`):
//! * Only allow-listed features can be addressed. Firmware/DFU features are
//!   unreachable from this crate.
//! * Onboard-memory write functions can only be reached through
//!   [`onboard::GuardedWriter`], which verifies every write by reading it back.

pub mod battery;
pub mod device;
pub mod dpi;
pub mod error;
pub mod onboard;
pub mod safety;
pub mod transport;

pub use device::Device;
pub use error::{Error, Result};

/// Logitech USB vendor id.
pub const LOGITECH_VID: u16 = 0x046D;

/// Feature ids used by this crate.
pub mod feature {
    pub const ROOT: u16 = 0x0000;
    pub const FEATURE_SET: u16 = 0x0001;
    pub const FIRMWARE_INFO: u16 = 0x0003;
    pub const DEVICE_NAME: u16 = 0x0005;
    pub const BATTERY_STATUS: u16 = 0x1000;
    pub const BATTERY_VOLTAGE: u16 = 0x1001;
    pub const UNIFIED_BATTERY: u16 = 0x1004;
    pub const ADJUSTABLE_DPI: u16 = 0x2201;
    pub const ONBOARD_PROFILES: u16 = 0x8100;
}
