//! Adjustable DPI (0x2201). `set` changes the sensor in RAM only: it is lost when
//! the mouse powers off, so it can never damage anything.

use crate::feature::ADJUSTABLE_DPI;
use crate::{Device, Error, Result};

/// Supported DPI values: either a list or ranges with steps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DpiCaps {
    pub min: u16,
    pub max: u16,
    /// (from, to, step) ranges; a plain list is represented as step-0 entries.
    pub ranges: Vec<(u16, u16, u16)>,
}

impl DpiCaps {
    pub fn supports(&self, dpi: u16) -> bool {
        self.ranges.iter().any(|&(a, b, step)| {
            if step == 0 {
                dpi == a
            } else {
                dpi >= a && dpi <= b && (dpi - a).is_multiple_of(step)
            }
        })
    }

    /// Closest supported value.
    pub fn clamp(&self, dpi: u16) -> u16 {
        let mut best = self.min;
        for &(a, b, step) in &self.ranges {
            let c = match (dpi.clamp(a, b) - a + step / 2).checked_div(step) {
                Some(n) => a + n * step,
                None => a,
            }
            .min(b.max(a));
            if dpi.abs_diff(c) < dpi.abs_diff(best) {
                best = c;
            }
        }
        best
    }
}

/// Parse the u16 BE list from getSensorDpiList. Values >= 0xE000 are a step
/// marker: `value & 0x1FFF` is the step between the previous and next value.
pub fn parse_dpi_list(words: &[u16]) -> Result<DpiCaps> {
    let mut ranges = Vec::new();
    let mut i = 0;
    while i < words.len() && words[i] != 0 {
        let w = words[i];
        if w >= 0xE000 {
            let step = w & 0x1FFF;
            let from = match ranges.pop() {
                Some((a, _, 0)) => a,
                _ => return Err(Error::Protocol("DPI step without start value".into())),
            };
            let to = *words
                .get(i + 1)
                .filter(|v| **v != 0 && **v < 0xE000)
                .ok_or_else(|| Error::Protocol("DPI step without end value".into()))?;
            ranges.push((from, to, step.max(1)));
            i += 2;
        } else {
            ranges.push((w, w, 0));
            i += 1;
        }
    }
    if ranges.is_empty() {
        return Err(Error::Protocol("empty DPI list".into()));
    }
    let min = ranges.iter().map(|r| r.0).min().unwrap_or(0);
    let max = ranges.iter().map(|r| r.1.max(r.0)).max().unwrap_or(0);
    Ok(DpiCaps { min, max, ranges })
}

pub fn caps(dev: &mut Device) -> Result<DpiCaps> {
    let r = dev.request(ADJUSTABLE_DPI, 1, &[0])?;
    if r[0] != 0 {
        return Err(Error::Protocol("unexpected sensor index".into()));
    }
    let mut words: Vec<u16> = r[1..15].chunks(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
    words.push(0);
    parse_dpi_list(&words)
}

/// (current, default)
pub fn get(dev: &mut Device) -> Result<(u16, u16)> {
    let r = dev.request(ADJUSTABLE_DPI, 2, &[0])?;
    Ok((u16::from_be_bytes([r[1], r[2]]), u16::from_be_bytes([r[3], r[4]])))
}

/// Set sensor DPI (RAM only). Returns the value the mouse confirmed.
pub fn set(dev: &mut Device, caps: &DpiCaps, dpi: u16) -> Result<u16> {
    if !caps.supports(dpi) {
        return Err(Error::Blocked(format!(
            "{dpi} DPI is not supported (range {}-{})",
            caps.min, caps.max
        )));
    }
    let b = dpi.to_be_bytes();
    dev.request(ADJUSTABLE_DPI, 3, &[0, b[0], b[1]])?;
    let (now, _) = get(dev)?;
    if now != dpi {
        return Err(Error::Verify(format!("asked for {dpi} DPI, mouse reports {now}")));
    }
    Ok(now)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn superlight_style_range() {
        // 100..25600 step 50
        let c = parse_dpi_list(&[100, 0xE000 | 50, 25600, 0]).unwrap();
        assert_eq!(c.min, 100);
        assert_eq!(c.max, 25600);
        assert!(c.supports(800));
        assert!(c.supports(25600));
        assert!(!c.supports(825));
        assert!(!c.supports(50));
        assert_eq!(c.clamp(830), 850);
        assert_eq!(c.clamp(60000), 25600);
    }

    #[test]
    fn plain_list() {
        let c = parse_dpi_list(&[400, 800, 1600, 0, 0]).unwrap();
        assert!(c.supports(800));
        assert!(!c.supports(1000));
        assert_eq!(c.clamp(1000), 800);
    }

    #[test]
    fn malformed_list_rejected() {
        assert!(parse_dpi_list(&[0xE032, 800]).is_err());
        assert!(parse_dpi_list(&[0]).is_err());
        assert!(parse_dpi_list(&[100, 0xE032, 0]).is_err());
    }
}
