//! Battery reading: Unified Battery (0x1004), Battery Voltage (0x1001) or Battery Status (0x1000).

use crate::feature::*;
use crate::{Device, Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Charging {
    Discharging,
    Charging,
    Full,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Battery {
    pub percent: u8,
    pub charging: Charging,
    /// Millivolts, when the mouse reports voltage (0x1001).
    pub millivolts: Option<u16>,
}

pub fn read(dev: &mut Device) -> Result<Battery> {
    if dev.has_feature(UNIFIED_BATTERY) {
        let r = dev.request(UNIFIED_BATTERY, 1, &[])?;
        let charging = match r[2] {
            0 => Charging::Discharging,
            1 | 2 => Charging::Charging,
            3 => Charging::Full,
            _ => Charging::Error,
        };
        return Ok(Battery { percent: r[0].min(100), charging, millivolts: None });
    }
    if dev.has_feature(BATTERY_VOLTAGE) {
        let r = dev.request(BATTERY_VOLTAGE, 0, &[])?;
        return Ok(decode_voltage(u16::from_be_bytes([r[0], r[1]]), r[2]));
    }
    if dev.has_feature(BATTERY_STATUS) {
        let r = dev.request(BATTERY_STATUS, 0, &[])?;
        let charging = match r[2] {
            0 => Charging::Discharging,
            1 | 2 | 4 => Charging::Charging,
            3 => Charging::Full,
            _ => Charging::Error,
        };
        return Ok(Battery { percent: r[0].min(100), charging, millivolts: None });
    }
    Err(Error::FeatureNotSupported(BATTERY_VOLTAGE))
}

/// Decode 0x1001 getBatteryInfo: voltage (mV, BE) + flags.
pub fn decode_voltage(mv: u16, flags: u8) -> Battery {
    let mut charging = Charging::Discharging;
    if flags & 0x80 != 0 {
        charging = match flags & 0x07 {
            0 => Charging::Charging,
            1 => Charging::Full,
            2 => Charging::Discharging, // "not charging"
            _ => Charging::Error,
        };
    }
    let percent = if charging == Charging::Full { 100 } else { voltage_to_percent(mv) };
    Battery { percent, charging, millivolts: Some(mv) }
}

/// Li-ion discharge curve for Logitech mice (same points Solaar uses), interpolated.
pub fn voltage_to_percent(mv: u16) -> u8 {
    const CURVE: &[(u16, u8)] = &[
        (4186, 100),
        (4067, 90),
        (3989, 80),
        (3922, 70),
        (3859, 60),
        (3811, 50),
        (3778, 40),
        (3751, 30),
        (3717, 20),
        (3671, 10),
        (3646, 5),
        (3579, 2),
        (3500, 0),
    ];
    if mv >= CURVE[0].0 {
        return 100;
    }
    for w in CURVE.windows(2) {
        let ((v_hi, p_hi), (v_lo, p_lo)) = (w[0], w[1]);
        if mv >= v_lo {
            let span = (v_hi - v_lo) as u32;
            let pos = (mv - v_lo) as u32;
            return p_lo + (((p_hi - p_lo) as u32 * pos + span / 2) / span) as u8;
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curve_points_and_bounds() {
        assert_eq!(voltage_to_percent(4300), 100);
        assert_eq!(voltage_to_percent(4186), 100);
        assert_eq!(voltage_to_percent(3811), 50);
        assert_eq!(voltage_to_percent(3500), 0);
        assert_eq!(voltage_to_percent(3000), 0);
        let mid = voltage_to_percent(3835);
        assert!(mid > 50 && mid < 60, "{mid}");
    }

    #[test]
    fn curve_is_monotonic() {
        let mut last = 0;
        for mv in 3400..4300 {
            let p = voltage_to_percent(mv);
            assert!(p >= last);
            last = p;
        }
    }

    #[test]
    fn voltage_flags() {
        assert_eq!(decode_voltage(3900, 0x00).charging, Charging::Discharging);
        assert_eq!(decode_voltage(3900, 0x80).charging, Charging::Charging);
        let full = decode_voltage(4100, 0x81);
        assert_eq!(full.charging, Charging::Full);
        assert_eq!(full.percent, 100);
    }
}
