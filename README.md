# Superlight Tray

A tiny tray app for the **Logitech PRO X Superlight**. It shows the battery on the tray icon and lets you change DPI from the menu. No G HUB needed. Works on Windows and Linux.

## ⚠️ Supported mouse

This app is made for, and tested with, **one mouse only**:

| | |
|---|---|
| Mouse | **Logitech PRO X Superlight** (1st generation) |
| Sensor | HERO 25K, 100–25,600 DPI |
| Connection | LIGHTSPEED USB receiver, or the USB cable |
| How to recognise it | micro-USB port on the front, no DPI button on top; in this app it reports itself as "PRO X Wireless" |

**Not supported:** Superlight 2, Superlight 2 DEX, G Pro Wireless, G305/G502 and every other mouse. They might connect, but nothing is tested for them, and saving to mouse memory assumes the Superlight's memory layout. Please don't use it with other mice.

## What it does

- **Battery on the tray icon**: big percentage digits plus a small battery bar. Green, yellow below 35%, red below 15%, blue while charging, grey "–" when the mouse is asleep or off.
- **Right-click (or left-click) the icon** for the menu:
  - **Battery: 93%**: click it to refresh right away.
  - **DPI ›**: quick presets (400, 800, 1200, 1600, 3200, 6400).
  - **Set DPI…**: a slider from 100 to 25,600. The DPI changes as you let go, so you can feel it straight away. OK keeps it, Cancel puts back the old value.
  - **Save to mouse memory**: stores the current DPI inside the mouse, so it survives power-off and works on any PC. It asks first and makes a backup.
  - **Recovery ›**: restore the last backup, restore the factory profile, or open the backups folder.
  - **Edit DPI presets…**: opens the settings file (see below).
  - **Refresh now**, **Start with Windows** (on/off), **Quit**.
- **Hover over the icon** to see e.g. "Superlight PRO: 93% • 800 DPI".
- It remembers your last DPI and puts it back when the mouse wakes up or reconnects.

## How light is it?

- **About 0.3 MB of RAM** when idle on Windows (Task Manager's "Memory" column), around 2 MB for a moment while the menu is open.
- Checks the battery **every 5 minutes**. When the mouse sleeps it checks every few seconds whether it's back.
- 1.2 MB program, no background services, no internet access, no telemetry, no admin rights.

## Install

### Windows 10 / 11

Download **`superlight-tray-<version>-setup.exe`** from [Releases](../../releases) and run it.
- No admin prompt; it installs just for you.
- Options during setup: **start when you sign in** (on by default) and **desktop shortcut** (off by default).
- Uninstall from **Settings → Apps → Installed apps → Superlight Tray**. It asks whether to also delete your settings and backups.

Prefer no installer? Use `superlight-tray-portable.exe` instead.

### Linux

| Distro | File | Install |
|---|---|---|
| Debian, Ubuntu, Mint, Pop!_OS | `.deb` | `sudo apt install ./superlight-tray_*_amd64.deb` |
| Fedora, Rocky Linux, RHEL, Alma (8 and 9) | `.rpm` | `sudo dnf install ./superlight-tray-*.x86_64.rpm` |

After installing, **unplug and replug the receiver once**. Then start "Superlight Tray" from your app menu.

**GNOME users** (Fedora, Rocky): GNOME has no tray by default. Install the AppIndicator extension (`sudo dnf install gnome-shell-extension-appindicator`; on Rocky it's in EPEL), then log out and back in. KDE, XFCE, Cinnamon, MATE and Ubuntu work out of the box.

The **Set DPI** slider on Linux needs `zenity` (GNOME/GTK) or `kdialog` (KDE). Most desktops already have one.

Uninstall: `sudo apt remove superlight-tray` or `sudo dnf remove superlight-tray`.

## Before you start

**Close G HUB.** G HUB takes over the mouse and the two will fight over it.

## Settings

Change them with **Edit DPI presets…** in the menu, then restart the app.

| Setting | Default | What it does |
|---|---|---|
| `presets` | `[400, 800, 1200, 1600, 3200, 6400]` | DPI values in the DPI menu (100–25,600, steps of 50) |
| `poll_seconds` | `300` | How often the battery is checked, in seconds (10–3600) |
| `reapply_on_connect` | `true` | Put your last DPI back when the mouse wakes up or reconnects |
| `last_dpi` | — | Saved automatically; the DPI you picked last |

Where things are kept:

| | Windows | Linux |
|---|---|---|
| Settings | `%APPDATA%\superlight-tray\config\config.toml` | `~/.config/superlight-tray/config.toml` |
| Backups and log | `%APPDATA%\superlight-tray\data\` | `~/.local/share/superlight-tray/` |

## Is saving to the mouse safe?

Yes, it's built to be careful:

- Changing DPI from the menu or slider is **temporary** until you press **Save to mouse memory**. Nothing can break there.
- When you save, the app **backs up** the mouse's profile first, changes **only the DPI**, then **reads it back to check**. If anything doesn't match, it puts the old profile back automatically.
- It can't touch the mouse's **firmware**; that part of the mouse is blocked in the code.
- If something ever goes wrong, the mouse falls back to its built-in factory profile, and **Recovery** in the menu can restore your backup or the factory profile.
- Saving is refused when the battery is below 15%.

## Privacy

The app talks to the mouse only to read the battery and set DPI. **It can't see your mouse movement, clicks or keystrokes**: it never opens those channels, and the mouse features that could redirect them are blocked. On Linux the installed permission rule only opens the settings channel, so other programs can't use it to spy on your input either.

## Command line (optional)

For troubleshooting, or if you prefer a terminal:

```
superlight-tray --probe            show what the app sees (changes nothing)
superlight-tray --set-dpi 1600     set DPI until the mouse powers off
superlight-tray --save-dpi 1600    save DPI to the mouse (asks first)
superlight-tray --dump             back up the mouse profile to a file
superlight-tray --restore <file>   put a backup back on the mouse
superlight-tray --factory-reset-profile
```

## Building from source

```
cargo test --workspace
cargo build --release
```

GitHub Actions builds the Windows installer, the Linux packages and a release automatically when a `v*` tag is pushed.

---

MIT license. Not affiliated with Logitech.
