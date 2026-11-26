//! Text shown in the tray, shared by the Windows and Linux front-ends.

use hidpp::battery::Charging;
use hidpp::dpi::DpiCaps;

use crate::worker::State;

pub fn battery_label(s: &State) -> String {
    if !s.receiver_present && !s.connected {
        return "Receiver not found".into();
    }
    match (s.connected, s.battery) {
        (true, Some(b)) => format!("Battery: {}%{}", b.percent, charging_suffix(b.charging)),
        (true, None) => "Battery: reading…".into(),
        (false, Some(b)) => format!("Mouse asleep / off (last {}%)", b.percent),
        (false, None) => "Mouse asleep / off".into(),
    }
}

fn charging_suffix(c: Charging) -> &'static str {
    match c {
        Charging::Discharging => "",
        Charging::Charging => " (charging)",
        Charging::Full => " (full)",
        Charging::Error => " (charging error)",
    }
}

pub fn dpi_label(s: &State, presets: &[u16]) -> String {
    match s.dpi {
        Some(d) if presets.contains(&d) => format!("DPI: {d}"),
        Some(d) => format!("DPI: {d} (custom)"),
        None => "DPI".into(),
    }
}

pub fn save_label(s: &State) -> String {
    match s.dpi {
        Some(d) => format!("Save {d} DPI to mouse memory…"),
        None => "Save DPI to mouse memory…".into(),
    }
}

pub fn tooltip(s: &State) -> String {
    let mut t = String::from("Superlight PRO");
    if s.connected {
        if let Some(b) = s.battery {
            t += &format!(": {}%{}", b.percent, charging_suffix(b.charging));
        }
        if let Some(d) = s.dpi {
            t += &format!(" • {d} DPI");
        }
    } else {
        t += ": not connected";
    }
    t
}

/// Percentage to draw on the icon (None = grey "-").
pub fn icon_percent(s: &State) -> Option<u8> {
    if s.connected {
        s.battery.map(|b| b.percent)
    } else {
        None
    }
}

pub fn icon_charging(s: &State) -> bool {
    s.connected && s.battery.is_some_and(|b| matches!(b.charging, Charging::Charging))
}

pub const SET_DPI_TITLE: &str = "Set DPI";

pub fn set_dpi_prompt(caps: Option<&DpiCaps>) -> String {
    match caps {
        Some(c) => format!("Enter a DPI value ({}–{}, steps of {}):", c.min, c.max, step_of(c)),
        None => "Enter a DPI value:".into(),
    }
}

fn step_of(c: &DpiCaps) -> u16 {
    c.ranges.iter().map(|r| r.2).find(|s| *s > 0).unwrap_or(1)
}

/// Validate what the user typed. Errors are shown to the user as-is.
pub fn parse_dpi(input: &str, caps: Option<&DpiCaps>) -> Result<u16, String> {
    let t = input.trim();
    if t.is_empty() {
        return Err("Please type a number.".into());
    }
    let v: u32 = t.parse().map_err(|_| format!("\"{t}\" is not a number."))?;
    let (min, max) = caps.map(|c| (c.min, c.max)).unwrap_or((100, 25600));
    if v < min as u32 || v > max as u32 {
        return Err(format!("{v} is out of range. The mouse supports {min}–{max} DPI."));
    }
    let v = v as u16;
    match caps {
        Some(c) if !c.supports(v) => Err(format!(
            "{v} is not possible: the mouse supports steps of {}. Nearest value: {}.",
            step_of(c),
            c.clamp(v)
        )),
        _ => Ok(v),
    }
}

#[cfg_attr(not(windows), allow(dead_code))]
pub const SAVE_CONFIRM: &str = "This writes the DPI into the mouse's onboard profile so it stays \
after power-off and works on other computers.\n\n\
Before writing, the current profile is backed up to a file. After writing, it is read back and \
compared; if anything differs the old profile is put back automatically.\n\n\
Continue?";

#[cfg_attr(not(windows), allow(dead_code))]
pub const FACTORY_CONFIRM: &str = "Copy the factory default profile over the mouse's active \
profile? (Default DPI 800, default buttons.) The current profile is backed up first.";

#[cfg_attr(not(windows), allow(dead_code))]
pub const RESTORE_CONFIRM: &str = "Write the most recent backup back to the mouse? The current \
profile is backed up first.";

#[cfg(test)]
mod tests {
    use super::*;

    fn caps() -> DpiCaps {
        hidpp::dpi::parse_dpi_list(&[100, 0xE000 | 50, 25600, 0]).unwrap()
    }

    #[test]
    fn parse_dpi_accepts_valid_values() {
        assert_eq!(parse_dpi("1600", Some(&caps())), Ok(1600));
        assert_eq!(parse_dpi("  850 ", Some(&caps())), Ok(850));
        assert_eq!(parse_dpi("100", Some(&caps())), Ok(100));
        assert_eq!(parse_dpi("25600", Some(&caps())), Ok(25600));
    }

    #[test]
    fn parse_dpi_rejects_bad_input_with_helpful_message() {
        assert!(parse_dpi("", Some(&caps())).is_err());
        assert!(parse_dpi("abc", Some(&caps())).unwrap_err().contains("not a number"));
        assert!(parse_dpi("-5", Some(&caps())).is_err());
        assert!(parse_dpi("99999999999", Some(&caps())).is_err());
        assert!(parse_dpi("50", Some(&caps())).unwrap_err().contains("out of range"));
        assert!(parse_dpi("30000", Some(&caps())).unwrap_err().contains("out of range"));
        assert!(parse_dpi("1620", Some(&caps()))
            .unwrap_err()
            .contains("Nearest value: 1600"));
    }

    #[test]
    fn prompt_shows_range() {
        assert_eq!(
            set_dpi_prompt(Some(&caps())),
            "Enter a DPI value (100–25600, steps of 50):"
        );
    }
}
