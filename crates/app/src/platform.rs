//! Small OS integrations: autostart, single instance, opening folders.

use std::path::Path;

#[cfg(windows)]
mod imp {
    use super::*;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
    use winreg::RegKey;

    const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const NAME: &str = "SuperlightTray";

    pub fn autostart_enabled() -> bool {
        RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey_with_flags(RUN, KEY_READ)
            .and_then(|k| k.get_value::<String, _>(NAME))
            .is_ok()
    }

    pub fn set_autostart(on: bool) -> std::io::Result<()> {
        let key = RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(RUN, KEY_READ | KEY_WRITE)?;
        if on {
            let exe = std::env::current_exe()?;
            key.set_value(NAME, &format!("\"{}\"", exe.display()))
        } else {
            match key.delete_value(NAME) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                r => r,
            }
        }
    }

    /// Returns false if another instance is running. The handle is leaked on purpose.
    pub fn single_instance() -> bool {
        use windows_sys::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
        use windows_sys::Win32::System::Threading::CreateMutexW;
        let name: Vec<u16> = "Local\\superlight-tray-instance\0".encode_utf16().collect();
        unsafe {
            let h = CreateMutexW(std::ptr::null(), 0, name.as_ptr());
            !h.is_null() && GetLastError() != ERROR_ALREADY_EXISTS
        }
    }

    pub fn open_path(p: &Path) {
        let _ = std::process::Command::new("explorer").arg(p).spawn();
    }

    /// Open a text file in Notepad. Uses Notepad's full path and starts it in the
    /// file's own folder: the Windows 11 Notepad reports "path not found" when it
    /// inherits a working directory it cannot use (e.g. a very long one).
    pub fn edit_file(p: &Path) {
        let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
        let notepad = Path::new(&root).join("System32").join("notepad.exe");
        let dir = p.parent().map(Path::to_path_buf).unwrap_or_else(std::env::temp_dir);
        let r = std::process::Command::new(&notepad).arg(p).current_dir(&dir).spawn();
        if let Err(e) = r {
            log::warn!("notepad failed ({e}); opening folder instead");
            open_path(&dir);
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;
    use std::path::PathBuf;

    fn autostart_file() -> PathBuf {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .unwrap_or_else(|| PathBuf::from("."));
        base.join("autostart").join("superlight-tray.desktop")
    }

    pub fn autostart_enabled() -> bool {
        autostart_file().exists()
    }

    pub fn set_autostart(on: bool) -> std::io::Result<()> {
        let f = autostart_file();
        if on {
            std::fs::create_dir_all(f.parent().unwrap())?;
            let exe = std::env::current_exe()?;
            std::fs::write(
                &f,
                format!(
                    "[Desktop Entry]\nType=Application\nName=Superlight Tray\nExec={}\nIcon=input-mouse\n\
                     X-GNOME-Autostart-enabled=true\nNoDisplay=true\n",
                    exe.display()
                ),
            )
        } else {
            match std::fs::remove_file(&f) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                r => r,
            }
        }
    }

    pub fn single_instance() -> bool {
        use std::os::linux::net::SocketAddrExt;
        use std::os::unix::net::{SocketAddr, UnixListener};
        let Ok(addr) = SocketAddr::from_abstract_name(b"superlight-tray-instance") else {
            return true;
        };
        match UnixListener::bind_addr(&addr) {
            Ok(l) => {
                std::mem::forget(l);
                true
            }
            Err(_) => false,
        }
    }

    pub fn open_path(p: &Path) {
        let _ = std::process::Command::new("xdg-open").arg(p).spawn();
    }
}

pub use imp::*;
