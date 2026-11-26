//! Safety allow-list. Everything not listed here is unreachable.

use crate::feature::*;
use crate::{Error, Result};

/// Features that may be addressed at all.
pub const ALLOWED_FEATURES: &[u16] = &[
    ROOT,
    FEATURE_SET,
    FIRMWARE_INFO,
    DEVICE_NAME,
    BATTERY_STATUS,
    BATTERY_VOLTAGE,
    UNIFIED_BATTERY,
    ADJUSTABLE_DPI,
    ONBOARD_PROFILES,
];

/// Features that are explicitly dangerous. Listed for documentation and tests;
/// they are already blocked by not being in [`ALLOWED_FEATURES`].
pub const FORBIDDEN_FEATURES: &[u16] = &[
    0x00C0, // DFU control (legacy)
    0x00C1, // DFU control (signed)
    0x00C2, // DFU control
    0x00C3, // DFU control (secure)
    0x00D0, // DFU
    0x00D1, // DFU (secure)
    0x1802, // device reset
    0x1803, // GPIO access
    0x1806, // config device properties (writes serial/name)
    0x1811, 0x1830, 0x1890, 0x18A1, 0x18B0, 0x1DF3, 0x1E00, 0x1E01, 0x1E02, 0x1E22, 0x1EB0, // test/manufacturing
];

/// Onboard-profile functions that write memory or change state.
/// Reachable only through `onboard::GuardedWriter`.
pub const ONBOARD_WRITE_FUNCTIONS: &[u8] = &[0x6, 0x7, 0x8];

/// Read-only or RAM-only functions per feature that the generic request path allows.
fn function_allowed(feature: u16, func: u8) -> bool {
    match feature {
        ROOT => func <= 1,
        FEATURE_SET => func <= 1,
        FIRMWARE_INFO => func <= 1,
        DEVICE_NAME => func <= 2,
        BATTERY_STATUS => func <= 1,
        BATTERY_VOLTAGE => func == 0,
        UNIFIED_BATTERY => func <= 1,
        // 0 count, 1 list, 2 get, 3 set (RAM only, lost at power-off)
        ADJUSTABLE_DPI => func <= 3,
        // 0 descr, 2 get mode, 4 get current profile, 5 memory read, 9 get dpi index
        ONBOARD_PROFILES => matches!(func, 0x0 | 0x2 | 0x4 | 0x5 | 0x9),
        _ => false,
    }
}

pub fn check_request(feature: u16, func: u8) -> Result<()> {
    if FORBIDDEN_FEATURES.contains(&feature) || !ALLOWED_FEATURES.contains(&feature) {
        return Err(Error::Blocked(format!("feature 0x{feature:04X} is not allowed")));
    }
    if !function_allowed(feature, func) {
        return Err(Error::Blocked(format!(
            "function {func} of feature 0x{feature:04X} is not allowed on the generic path"
        )));
    }
    Ok(())
}

pub fn check_guarded_write(feature: u16, func: u8) -> Result<()> {
    if feature == ONBOARD_PROFILES && ONBOARD_WRITE_FUNCTIONS.contains(&func) {
        Ok(())
    } else {
        Err(Error::Blocked(format!("0x{feature:04X}/{func} is not a guarded write")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dfu_is_blocked() {
        for f in FORBIDDEN_FEATURES {
            for func in 0..16 {
                assert!(check_request(*f, func).is_err());
                assert!(check_guarded_write(*f, func).is_err());
            }
        }
    }

    #[test]
    fn onboard_writes_blocked_on_generic_path() {
        for func in ONBOARD_WRITE_FUNCTIONS {
            assert!(check_request(ONBOARD_PROFILES, *func).is_err());
        }
        // set mode / set profile / set dpi index also not on generic path
        for func in [0x1, 0x3, 0xA] {
            assert!(check_request(ONBOARD_PROFILES, func).is_err());
        }
        assert!(check_request(ONBOARD_PROFILES, 0x5).is_ok());
    }

    #[test]
    fn dpi_set_is_allowed() {
        assert!(check_request(ADJUSTABLE_DPI, 3).is_ok());
        assert!(check_request(ADJUSTABLE_DPI, 4).is_err());
    }
}
