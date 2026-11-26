use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("HID error: {0}")]
    Hid(String),
    #[error("no compatible Logitech receiver or mouse found")]
    NotFound,
    #[error("timed out waiting for the mouse to answer")]
    Timeout,
    #[error("HID++ 1.0 error 0x{code:02X} ({name})", code = .0, name = hidpp10_name(*.0))]
    Hidpp10(u8),
    #[error("HID++ 2.0 error 0x{code:02X} ({name})", code = .0, name = hidpp20_name(*.0))]
    Hidpp20(u8),
    #[error("the mouse does not support feature 0x{0:04X}")]
    FeatureNotSupported(u16),
    #[error("blocked by safety guard: {0}")]
    Blocked(String),
    #[error("protocol error: {0}")]
    Protocol(String),
    #[error("verification failed: {0}")]
    Verify(String),
}

impl Error {
    /// True when the receiver says the mouse itself is unreachable (off / asleep / out of range).
    pub fn is_device_offline(&self) -> bool {
        matches!(self, Error::Hidpp10(0x08 | 0x09 | 0x04) | Error::Timeout)
    }
}

impl From<hidapi::HidError> for Error {
    fn from(e: hidapi::HidError) -> Self {
        Error::Hid(e.to_string())
    }
}

fn hidpp10_name(c: u8) -> &'static str {
    match c {
        0x01 => "invalid sub-id/command",
        0x02 => "invalid address",
        0x03 => "invalid value",
        0x04 => "connection request failed",
        0x05 => "too many devices",
        0x06 => "already exists",
        0x07 => "busy",
        0x08 => "unknown device",
        0x09 => "resource error",
        0x0A => "request unavailable",
        0x0B => "invalid parameter value",
        0x0C => "wrong PIN code",
        _ => "unknown",
    }
}

fn hidpp20_name(c: u8) -> &'static str {
    match c {
        0x01 => "unknown",
        0x02 => "invalid argument",
        0x03 => "out of range",
        0x04 => "hardware error",
        0x05 => "logitech internal",
        0x06 => "invalid feature index",
        0x07 => "invalid function id",
        0x08 => "busy",
        0x09 => "unsupported",
        _ => "unknown",
    }
}
