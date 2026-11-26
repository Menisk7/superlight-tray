//! HID++ 2.0 request/response and feature discovery.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::feature::*;
use crate::safety;
use crate::transport::{LinkKind, Transport, LONG_LEN, REPORT_LONG, REPORT_SHORT};
use crate::{Error, Result};

const SW_ID: u8 = 0x0B;
const TIMEOUT: Duration = Duration::from_millis(1200);
pub const WIRED_INDEX: u8 = 0xFF;

pub struct Device {
    transport: Box<dyn Transport>,
    pub index: u8,
    features: HashMap<u16, Option<u8>>,
    pub protocol: (u8, u8),
}

#[derive(Debug, Clone)]
pub struct FeatureInfo {
    pub index: u8,
    pub id: u16,
    pub kind: u8,
    pub version: u8,
}

impl Device {
    /// Find the mouse behind `transport`. Over a receiver, tries pairing slots 1..=6
    /// and picks the first HID++ 2.0 device that has the Adjustable DPI feature.
    pub fn open(transport: Box<dyn Transport>) -> Result<Self> {
        let indices: Vec<u8> = match transport.kind() {
            LinkKind::Wired => vec![WIRED_INDEX],
            LinkKind::Receiver => (1..=6).collect(),
        };
        let mut dev = Device { transport, index: 0, features: HashMap::new(), protocol: (0, 0) };
        let mut last_err = Error::NotFound;
        for idx in indices {
            dev.index = idx;
            dev.features.clear();
            match dev.ping() {
                Ok(p) if p.0 >= 2 => {
                    dev.protocol = p;
                    match dev.feature_index(ADJUSTABLE_DPI) {
                        Ok(_) => return Ok(dev),
                        Err(e) => last_err = e,
                    }
                }
                Ok(_) => last_err = Error::Protocol("HID++ 1.0 device, not a Superlight".into()),
                Err(e) => {
                    // keep the most informative error ("mouse asleep" over "empty slot")
                    if !matches!(last_err, Error::Hidpp10(0x09) | Error::Timeout) {
                        last_err = e;
                    }
                }
            }
        }
        Err(last_err)
    }

    pub fn link(&self) -> String {
        self.transport.describe()
    }

    /// IRoot.getProtocolVersion. Also tells us whether the mouse is awake.
    pub fn ping(&mut self) -> Result<(u8, u8)> {
        let r = self.raw(0, 1, &[0, 0, 0x5A], Duration::from_millis(800))?;
        if r[2] != 0x5A {
            return Err(Error::Protocol("bad ping echo".into()));
        }
        Ok((r[0], r[1]))
    }

    /// Feature index for a feature id (cached). Only allow-listed features.
    pub fn feature_index(&mut self, id: u16) -> Result<u8> {
        if id == ROOT {
            return Ok(0);
        }
        safety::check_request(id, 0)?;
        if let Some(c) = self.features.get(&id) {
            return c.ok_or(Error::FeatureNotSupported(id));
        }
        let r = self.raw(0, 0, &id.to_be_bytes(), TIMEOUT)?;
        let idx = if r[0] == 0 { None } else { Some(r[0]) };
        self.features.insert(id, idx);
        idx.ok_or(Error::FeatureNotSupported(id))
    }

    pub fn has_feature(&mut self, id: u16) -> bool {
        self.feature_index(id).is_ok()
    }

    /// Generic, safety-checked request.
    pub fn request(&mut self, feature: u16, func: u8, params: &[u8]) -> Result<[u8; 16]> {
        safety::check_request(feature, func)?;
        let idx = self.feature_index(feature)?;
        self.raw(idx, func, params, TIMEOUT)
    }

    /// Only for `onboard::GuardedWriter`.
    pub(crate) fn guarded_write_request(&mut self, feature: u16, func: u8, params: &[u8]) -> Result<[u8; 16]> {
        safety::check_guarded_write(feature, func)?;
        let idx = self.feature_index(feature)?;
        self.raw(idx, func, params, Duration::from_millis(2500))
    }

    /// Enumerate all features (read-only, for --probe).
    pub fn list_features(&mut self) -> Result<Vec<FeatureInfo>> {
        let fs = self.feature_index(FEATURE_SET)?;
        let count = self.raw(fs, 0, &[], TIMEOUT)?[0];
        let mut out = vec![FeatureInfo { index: 0, id: 0, kind: 0, version: 0 }];
        for i in 1..=count {
            let r = self.raw(fs, 1, &[i], TIMEOUT)?;
            out.push(FeatureInfo { index: i, id: u16::from_be_bytes([r[0], r[1]]), kind: r[2], version: r[3] });
        }
        Ok(out)
    }

    pub fn name(&mut self) -> Result<String> {
        let len = self.request(DEVICE_NAME, 0, &[])?[0] as usize;
        let mut name = Vec::new();
        while name.len() < len {
            let r = self.request(DEVICE_NAME, 1, &[name.len() as u8])?;
            let take = (len - name.len()).min(16);
            name.extend_from_slice(&r[..take]);
        }
        Ok(String::from_utf8_lossy(&name).trim_end_matches('\0').to_string())
    }

    fn raw(&mut self, feat_idx: u8, func: u8, params: &[u8], timeout: Duration) -> Result<[u8; 16]> {
        if params.len() > 16 {
            return Err(Error::Protocol("params too long".into()));
        }
        let fs = (func << 4) | SW_ID;
        let mut report = [0u8; LONG_LEN];
        report[0] = REPORT_LONG;
        report[1] = self.index;
        report[2] = feat_idx;
        report[3] = fs;
        report[4..4 + params.len()].copy_from_slice(params);
        self.transport.write(&report)?;

        let deadline = Instant::now() + timeout;
        let mut buf = [0u8; 64];
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(Error::Timeout);
            }
            let Some(n) = self.transport.read(&mut buf, left)? else {
                return Err(Error::Timeout);
            };
            if let Some(res) = match_response(&buf[..n], self.index, feat_idx, fs) {
                return res;
            }
            // otherwise: unrelated notification, keep waiting
        }
    }
}

/// Classify one incoming report against the request we sent.
fn match_response(r: &[u8], dev: u8, feat_idx: u8, fs: u8) -> Option<Result<[u8; 16]>> {
    if r.len() < 7 || !(r[0] == REPORT_SHORT || r[0] == REPORT_LONG) || r[1] != dev {
        return None;
    }
    if r[2] == 0x8F && r[3] == feat_idx && r[4] == fs {
        return Some(Err(Error::Hidpp10(r[5])));
    }
    if r[2] == 0xFF && r[3] == feat_idx && r[4] == fs {
        return Some(Err(Error::Hidpp20(r[5])));
    }
    if r[2] == feat_idx && r[3] == fs {
        let mut out = [0u8; 16];
        let payload = &r[4..r.len().min(20)];
        out[..payload.len()].copy_from_slice(payload);
        return Some(Ok(out));
    }
    None
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::transport::MockTransport;

    pub type Handler = Box<dyn FnMut(u16, u8, &[u8]) -> Option<Vec<u8>> + Send>;

    /// Simulated mouse on receiver slot 1: feature table + per-feature handler.
    /// Handler returns `None` to answer with a HID++ 2.0 error.
    pub fn fake_mouse(features: Vec<(u16, u8)>, mut handler: Handler) -> Device {
        let t = MockTransport::new(LinkKind::Receiver, move |req| {
            let (dev, idx, fs) = (req[1], req[2], req[3]);
            let func = fs >> 4;
            if dev != 1 {
                return vec![vec![0x10, dev, 0x8F, idx, fs, 0x08, 0]];
            }
            let mut reply = vec![0x11, dev, idx, fs];
            let payload: Vec<u8> = if idx == 0 && func == 1 {
                vec![4, 2, req[6]]
            } else if idx == 0 && func == 0 {
                let id = u16::from_be_bytes([req[4], req[5]]);
                vec![features.iter().find(|f| f.0 == id).map(|f| f.1).unwrap_or(0), 0, 0]
            } else {
                let id = features.iter().find(|f| f.1 == idx).map(|f| f.0).unwrap_or(0xFFFF);
                match handler(id, func, &req[4..20]) {
                    Some(p) => p,
                    None => {
                        let mut e = vec![0x11, dev, 0xFF, idx, fs, 0x07];
                        e.resize(20, 0);
                        return vec![e];
                    }
                }
            };
            reply.extend(payload);
            reply.resize(20, 0);
            // an unrelated notification first, to prove we skip it
            vec![vec![0x10, 1, 0x41, 0x04, 0x00, 0x55, 0], reply]
        });
        Device::open(Box::new(t)).expect("open fake mouse")
    }

    #[test]
    fn finds_device_on_slot_1_and_ignores_notifications() {
        let mut d = fake_mouse(vec![(ADJUSTABLE_DPI, 7)], Box::new(|_, _, _| Some(vec![])));
        assert_eq!(d.index, 1);
        assert_eq!(d.protocol, (4, 2));
        assert_eq!(d.feature_index(ADJUSTABLE_DPI).unwrap(), 7);
        assert!(matches!(d.feature_index(BATTERY_VOLTAGE), Err(Error::FeatureNotSupported(_))));
    }

    #[test]
    fn hidpp20_error_is_reported() {
        let mut d = fake_mouse(vec![(ADJUSTABLE_DPI, 7)], Box::new(|_, _, _| None));
        assert!(matches!(d.request(ADJUSTABLE_DPI, 2, &[0]), Err(Error::Hidpp20(0x07))));
    }

    #[test]
    fn blocked_feature_never_resolves() {
        let mut d = fake_mouse(vec![(ADJUSTABLE_DPI, 7), (0x00D0, 9)], Box::new(|_, _, _| Some(vec![])));
        assert!(matches!(d.request(0x00D0, 1, &[]), Err(Error::Blocked(_))));
        assert!(matches!(d.feature_index(0x00D0), Err(Error::Blocked(_))));
    }
}
