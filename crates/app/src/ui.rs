//! Text shown in the tray, shared by the Windows and Linux front-ends.

use hidpp::battery::Charging;

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
