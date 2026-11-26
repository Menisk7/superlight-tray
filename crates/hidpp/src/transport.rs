//! Raw HID transport. A `Transport` moves whole HID++ reports (report id first).

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::{Error, Result, LOGITECH_VID};

pub const REPORT_SHORT: u8 = 0x10;
pub const REPORT_LONG: u8 = 0x11;
pub const SHORT_LEN: usize = 7;
pub const LONG_LEN: usize = 20;
const HIDPP_USAGE_PAGE: u16 = 0xFF00;

/// LIGHTSPEED receivers known to pair with the PRO X Superlight family.
pub const RECEIVER_PIDS: &[u16] = &[0xC539, 0xC53A, 0xC53D, 0xC53F, 0xC545, 0xC547, 0xC54D];
/// The mouse itself when connected by USB cable.
pub const WIRED_PIDS: &[u16] = &[0xC094, 0xC09B, 0xC09D];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkKind {
    Receiver,
    Wired,
}

pub trait Transport: Send {
    fn write(&mut self, report: &[u8]) -> Result<()>;
    /// Read one report into `buf`. `Ok(None)` on timeout.
    fn read(&mut self, buf: &mut [u8; 64], timeout: Duration) -> Result<Option<usize>>;
    fn kind(&self) -> LinkKind;
    fn describe(&self) -> String;
}

/// hidapi-backed transport. On Windows the short and long HID++ collections are
/// separate handles; on Linux there is a single hidraw node.
pub struct HidTransport {
    handles: Vec<(u16, hidapi::HidDevice)>, // (usage, handle)
    kind: LinkKind,
    pid: u16,
}

#[derive(Debug, Clone)]
pub struct Candidate {
    pub pid: u16,
    pub kind: LinkKind,
    pub paths: Vec<(u16, std::ffi::CString)>,
}

/// List receivers / wired mice that expose a HID++ interface. Receivers first.
pub fn candidates(api: &hidapi::HidApi) -> Vec<Candidate> {
    let mut out: Vec<Candidate> = Vec::new();
    for info in api.device_list() {
        if info.vendor_id() != LOGITECH_VID || info.usage_page() != HIDPP_USAGE_PAGE {
            continue;
        }
        let pid = info.product_id();
        let kind = if RECEIVER_PIDS.contains(&pid) {
            LinkKind::Receiver
        } else if WIRED_PIDS.contains(&pid) {
            LinkKind::Wired
        } else {
            continue;
        };
        let path = info.path().to_owned();
        // Group the collections of one physical device: same path (Linux) or same
        // device instance (Windows: path prefix before "&col").
        let key = instance_key(&path);
        if let Some(c) = out
            .iter_mut()
            .find(|c| c.pid == pid && c.paths.iter().any(|(_, p)| instance_key(p) == key))
        {
            if !c.paths.iter().any(|(_, p)| *p == path) {
                c.paths.push((info.usage(), path));
            }
        } else {
            out.push(Candidate {
                pid,
                kind,
                paths: vec![(info.usage(), path)],
            });
        }
    }
    out.sort_by_key(|c| (c.kind != LinkKind::Receiver, c.pid));
    out
}

fn instance_key(path: &std::ffi::CStr) -> String {
    let s = path.to_string_lossy().to_lowercase();
    match s.find("&col") {
        Some(i) => s[..i].to_string(),
        None => s,
    }
}

impl HidTransport {
    pub fn open(api: &hidapi::HidApi, c: &Candidate) -> Result<Self> {
        let mut handles = Vec::new();
        for (usage, path) in &c.paths {
            match api.open_path(path) {
                Ok(h) => handles.push((*usage, h)),
                Err(e) => log::debug!("open {path:?} failed: {e}"),
            }
        }
        if handles.is_empty() {
            return Err(Error::Hid(format!(
                "could not open HID++ interface of 046D:{:04X}",
                c.pid
            )));
        }
        // long collection (usage 2) first so writes go there
        handles.sort_by_key(|(u, _)| if *u == 2 { 0 } else { 1 });
        Ok(Self {
            handles,
            kind: c.kind,
            pid: c.pid,
        })
    }
}

impl Transport for HidTransport {
    fn write(&mut self, report: &[u8]) -> Result<()> {
        let n = self.handles[0].1.write(report)?;
        if n < report.len() {
            return Err(Error::Hid(format!("short write {n}/{}", report.len())));
        }
        Ok(())
    }

    fn read(&mut self, buf: &mut [u8; 64], timeout: Duration) -> Result<Option<usize>> {
        if self.handles.len() == 1 {
            let ms = timeout.as_millis().clamp(1, i32::MAX as u128) as i32;
            let n = self.handles[0].1.read_timeout(buf, ms)?;
            return Ok((n > 0).then_some(n));
        }
        let deadline = Instant::now() + timeout;
        loop {
            for (_, h) in &self.handles {
                let n = h.read_timeout(buf, 4)?;
                if n > 0 {
                    return Ok(Some(n));
                }
            }
            if Instant::now() >= deadline {
                return Ok(None);
            }
        }
    }

    fn kind(&self) -> LinkKind {
        self.kind
    }

    fn describe(&self) -> String {
        let k = match self.kind {
            LinkKind::Receiver => "LIGHTSPEED receiver",
            LinkKind::Wired => "USB cable",
        };
        format!("{k} 046D:{:04X}", self.pid)
    }
}

type Responder = Box<dyn FnMut(&[u8]) -> Vec<Vec<u8>> + Send>;

/// Scripted transport for tests: every written report goes to a responder
/// closure which returns the reports the "device" sends back.
pub struct MockTransport {
    pub kind: LinkKind,
    queue: VecDeque<Vec<u8>>,
    responder: Responder,
}

impl MockTransport {
    pub fn new(kind: LinkKind, responder: impl FnMut(&[u8]) -> Vec<Vec<u8>> + Send + 'static) -> Self {
        Self {
            kind,
            queue: VecDeque::new(),
            responder: Box::new(responder),
        }
    }
}

impl Transport for MockTransport {
    fn write(&mut self, report: &[u8]) -> Result<()> {
        let replies = (self.responder)(report);
        self.queue.extend(replies);
        Ok(())
    }
    fn read(&mut self, buf: &mut [u8; 64], _timeout: Duration) -> Result<Option<usize>> {
        match self.queue.pop_front() {
            Some(r) => {
                buf[..r.len()].copy_from_slice(&r);
                Ok(Some(r.len()))
            }
            None => Ok(None),
        }
    }
    fn kind(&self) -> LinkKind {
        self.kind
    }
    fn describe(&self) -> String {
        "mock".into()
    }
}
