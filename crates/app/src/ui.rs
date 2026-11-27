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

pub fn save_label(_s: &State) -> String {
    "Save to mouse memory".into()
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

/// Tick marks drawn on the slider.
#[cfg_attr(not(windows), allow(dead_code))]
pub const SLIDER_TICKS: [u16; 7] = [400, 800, 1600, 3200, 6400, 12800, 25600];

/// Every DPI the slider can stop at: exactly the values the mouse accepts, in
/// its own step (50 on the Superlight, across the whole range). Without caps it
/// falls back to 100..25600 in steps of 50. The current DPI is always included.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn slider_values(caps: Option<&DpiCaps>, current: Option<u16>) -> Vec<u16> {
    let mut v = match caps {
        Some(c) => c.values(),
        None => (100..=25600).step_by(50).collect(),
    };
    if let Some(cur) = current {
        if !v.contains(&cur) {
            v.push(cur);
            v.sort_unstable();
        }
    }
    if v.is_empty() {
        v.push(current.unwrap_or(800));
    }
    v
}

/// Slider position of the value closest to `dpi`.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn slider_index(values: &[u16], dpi: u16) -> usize {
    values
        .iter()
        .enumerate()
        .min_by_key(|(_, v)| v.abs_diff(dpi))
        .map(|(i, _)| i)
        .unwrap_or(0)
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
    fn slider_uses_mouse_step_over_whole_range() {
        let v = slider_values(Some(&caps()), None);
        assert_eq!(v.first(), Some(&100));
        assert_eq!(v.last(), Some(&25600));
        assert_eq!(v.len(), 511);
        // every step is exactly 50, also above 3200
        assert!(v.windows(2).all(|w| w[1] - w[0] == 50));
        for d in [3250, 6450, 12850, 25550] {
            assert!(v.contains(&d), "{d}");
        }
        for t in SLIDER_TICKS {
            assert!(v.contains(&t));
        }
    }

    #[test]
    fn slider_without_caps_falls_back_to_50() {
        let v = slider_values(None, None);
        assert_eq!(v.len(), 511);
        assert!(v.windows(2).all(|w| w[1] - w[0] == 50));
    }

    #[test]
    fn slider_keeps_current_custom_value() {
        let v = slider_values(Some(&caps()), Some(7075));
        assert!(v.contains(&7075));
        assert_eq!(v[slider_index(&v, 7075)], 7075);
        assert!(v.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn slider_index_picks_nearest() {
        let v = slider_values(Some(&caps()), None);
        assert_eq!(v[slider_index(&v, 800)], 800);
        assert_eq!(v[slider_index(&v, 820)], 800);
        assert_eq!(v[slider_index(&v, 60000)], 25600);
    }

    #[test]
    fn slider_respects_device_limits() {
        let small = hidpp::dpi::parse_dpi_list(&[400, 800, 1600, 0]).unwrap();
        assert_eq!(slider_values(Some(&small), None), vec![400, 800, 1600]);
    }
}
