# Superlight Tray

A small tray app for the **Logitech PRO X Superlight** on Windows and Linux. Written in Rust; no G HUB needed.

- The tray icon *is* the battery: large percentage digits plus a coloured gauge (green → yellow → red, blue while charging, grey "–" when the mouse is asleep or off).
- Right-click (or left-click) the icon for the battery level and a **DPI** submenu with presets. DPI changes apply instantly.
- **Set DPI…** opens a slider (100–25600: steps of 50 up to 3200, coarser above). The DPI applies as you release the slider, so you can try it on the spot. **OK** keeps it, **Cancel**/Esc puts the old value back. Arrow keys move one step, Page Up/Down move 8. On Linux the slider comes from zenity or kdialog and applies on OK.
- **Save DPI to mouse memory**: writes the DPI into the mouse's onboard profile, so it sticks after power-off and works on any computer. This is opt-in and heavily guarded (see *Safety*).
- Remembers your last DPI and puts it back when the mouse wakes up or reconnects.
- Optional start on login. Tiny footprint: one background thread, polls the battery every 5 minutes (`poll_seconds` in the config).

Tested on: PRO X Superlight (1st gen, reports itself as "PRO X Wireless") on LIGHTSPEED receiver `046D:C547`, firmware HID++ 4.2, profile format 4, 255-byte sectors.

## Install

| OS | File |
|----|------|
| Windows 10/11 | `superlight-tray-<ver>-setup.exe`: installer, no admin needed, shows up in *Installed apps*. Or `superlight-tray-portable.exe` with no install. |
| Debian 11+, Ubuntu 22.04+, Mint, Pop!_OS | `superlight-tray_<ver>_amd64.deb` |
| Fedora, Rocky Linux / RHEL / Alma 8 & 9 | `superlight-tray-<ver>-1.x86_64.rpm` |
| anything else | `superlight-tray-<ver>-linux-x86_64.tar.gz` |

```sh
sudo apt install ./superlight-tray_*_amd64.deb      # Debian/Ubuntu
sudo dnf install ./superlight-tray-*.x86_64.rpm     # Fedora/Rocky
```

**Windows installer:** installs to `%LOCALAPPDATA%\Programs\Superlight Tray`, adds a Start menu entry and optionally starts the app at sign-in. Uninstall from *Settings → Apps → Installed apps*. That closes the tray and removes the startup entry, then asks whether to delete your settings and backups too (No by default).

**Linux packages** show up in your package manager and app menu. Remove with `sudo apt remove superlight-tray` or `sudo dnf remove superlight-tray`. Your settings in `~/.config/superlight-tray`, backups in `~/.local/share/superlight-tray` and the autostart file `~/.config/autostart/superlight-tray.desktop` stay behind, as with most Linux apps. Delete them by hand for a clean slate.

The packages install a udev rule (`/usr/lib/udev/rules.d/60-superlight-tray.rules`) so your user can access the receiver without root. **Unplug and replug the receiver once after installing.** With the tarball, copy that rule yourself, then run `sudo udevadm control --reload-rules`.

**GNOME** (Fedora Workstation, Rocky with GNOME) has no tray by default. Install and enable the AppIndicator extension:
`sudo dnf install gnome-shell-extension-appindicator` (Rocky: from EPEL), then log out and back in. Ubuntu, KDE, XFCE, Cinnamon and MATE work out of the box.

Close **G HUB** first. It switches the mouse to "host mode" and fights over the receiver.

## Command line

All recovery tools also work without the tray:

```
superlight-tray --probe                 read-only report (features, battery, DPI, profiles)
superlight-tray --set-dpi 1600          set DPI live (RAM only)
superlight-tray --save-dpi 1600         save into the onboard profile (asks first)
superlight-tray --dump                  back up every profile sector to a file
superlight-tray --restore <backup.json> write a backup back (asks first)
superlight-tray --factory-reset-profile copy the factory ROM profile over the user profile
superlight-tray --backups               show where backups are stored
```

Files:
- Windows: config `%APPDATA%\superlight-tray\config\config.toml`; backups and log in `%APPDATA%\superlight-tray\data\`
- Linux: config `~/.config/superlight-tray/config.toml`; backups and log in `~/.local/share/superlight-tray/`

Edit `presets = [400, 800, 1200, 1600, 3200, 6400]` in the config to change the menu (100–25600, steps of 50). Set `SUPERLIGHT_DEBUG=1` for a verbose log.

## Safety and disaster recovery

The mouse's *firmware* can't be touched by this program. The HID++ layer has an allow-list: only the features for root, feature set, firmware info, device name, battery, DPI and onboard profiles can be addressed at all. DFU, firmware-update, reset, GPIO and manufacturing features are blocked in code (`crates/hidpp/src/safety.rs`, with unit tests).

- **DPI from the menu** changes the sensor in RAM only. It's gone after a power cycle, so there's nothing to break.
- **Save to mouse memory** writes exactly one 255-byte *profile* sector, and only after all of these pass:
  1. Onboard mode is active, the profile format is one we know, and the active profile is a user sector. ROM sectors and the profile directory are never written.
  2. Battery is above 15%. The sector reads back identically twice (the wireless link is stable) and its CRC is valid.
  3. A backup (`.json` and `.bin`) is written, fsynced and read back. If that fails, nothing is written.
  4. Only the 2 bytes of the active DPI slot and the CRC may change. Anything else aborts.
  5. After writing, the sector is read back and compared byte for byte. If it differs, the original is written back and verified automatically.
- **Worst case**, a sector with a bad CRC: the firmware ignores it and runs the built-in factory profile. Use the tray's *Recovery → Restore last backup*, or *Restore factory profile* (copies the ROM profile, 800 DPI default, back to the user sector). Both also exist as CLI commands. Every restore makes a fresh backup of the current state first.

## Privacy and security

- **It can't watch your input.** The app opens only the receiver's vendor-specific HID++ settings channel, never the mouse/keyboard input channels. HID++ features that could reroute movement, clicks or wheel to that channel (button spy `0x8110`, reprogrammable keys `0x1B04`, analysis mode `0x2250`, wheel divert, …) are blocked by the allow-list, and unit tests check this. Unrelated messages from the receiver are dropped, never stored or logged.
- **Linux udev rule** opens up only the HID++ interface (receiver interface 2, cable interface 1). Mouse movement and keystrokes stay root-only.
- **No network code** and no telemetry. No admin rights: the Windows installer is per-user, and the app never asks for elevation.
- A broken or fake device can't crash it: replies are bounds-checked and fuzz-style tests feed it garbage.
- What it can't protect against: a program already running as *your* user can talk to the mouse directly, with or without this app, so the app doesn't widen that.

Idle memory on Windows is about 0.3 MB in Task Manager. A few seconds after any activity, pages it isn't using go back to Windows.

## Building

```sh
cargo test --workspace
cargo build --release            # target/release/superlight-tray(.exe)
cargo install cargo-deb cargo-generate-rpm
cargo deb -p superlight-tray --no-build
cargo generate-rpm -p crates/app
```

Linux needs no system libraries to build (pure-Rust hidraw and D-Bus). Release binaries are built on Rocky Linux 8 (glibc 2.28), so they run on every distro listed above.

On Windows, if linking fails with `LNK1104` the checkout path is too long. Set `CARGO_TARGET_DIR` to a short path.

## Layout

```
crates/hidpp   HID++ 2.0 protocol, safety allow-list, battery / DPI / onboard profiles (unit-tested with a simulated mouse)
crates/app     tray (Windows: tray-icon, Linux: ksni), device worker thread, icon renderer, backups, CLI
packaging/     udev rule, .desktop, package scripts
```

License: MIT. Not affiliated with Logitech.
