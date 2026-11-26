//! Linux tray: StatusNotifierItem over D-Bus via `ksni` (pure Rust, no GTK).
//! Works on KDE, XFCE, Cinnamon, MATE, LXQt, and GNOME with the AppIndicator
//! extension (installed by default on Ubuntu/Fedora-KDE; on Fedora/Rocky GNOME
//! install `gnome-shell-extension-appindicator`).

use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use crossbeam_channel::{unbounded, Sender};
use ksni::blocking::TrayMethods;
use ksni::menu::{CheckmarkItem, StandardItem, SubMenu};
use ksni::{Icon, MenuItem, ToolTip, Tray};

use crate::config::{self, Config};
use crate::worker::{self, Cmd, Event, State};
use crate::{backup, icon, platform, ui};

struct SlTray {
    state: State,
    presets: Vec<u16>,
    cmd: Sender<Cmd>,
}

fn item(label: impl Into<String>, enabled: bool, f: impl Fn(&mut SlTray) + Send + 'static) -> MenuItem<SlTray> {
    StandardItem {
        label: label.into(),
        enabled,
        activate: Box::new(f),
        ..Default::default()
    }
    .into()
}

fn label(text: impl Into<String>) -> MenuItem<SlTray> {
    StandardItem {
        label: text.into(),
        enabled: false,
        ..Default::default()
    }
    .into()
}

/// A submenu whose only entry performs the action: a confirmation that works on
/// every desktop without needing a dialog toolkit.
fn confirm_sub(text: String, enabled: bool, yes: &str, f: impl Fn(&mut SlTray) + Send + 'static) -> MenuItem<SlTray> {
    SubMenu {
        label: text,
        enabled,
        submenu: vec![item(yes, true, f)],
        ..Default::default()
    }
    .into()
}

impl Tray for SlTray {
    fn id(&self) -> String {
        "superlight-tray".into()
    }

    fn title(&self) -> String {
        "Superlight Tray".into()
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        let p = ui::icon_percent(&self.state);
        let ch = ui::icon_charging(&self.state);
        [22u32, 24, 32, 48]
            .iter()
            .map(|&s| Icon {
                width: s as i32,
                height: s as i32,
                data: icon::rgba_to_argb(&icon::render(p, ch, s)),
            })
            .collect()
    }

    fn tool_tip(&self) -> ToolTip {
        ToolTip {
            title: ui::tooltip(&self.state),
            ..Default::default()
        }
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let s = &self.state;
        let ok = s.connected && !s.busy;
        let dpi_items: Vec<MenuItem<Self>> = self
            .presets
            .iter()
            .map(|&d| {
                CheckmarkItem {
                    label: format!("{d} DPI"),
                    enabled: ok && s.caps.as_ref().is_none_or(|c| c.supports(d)),
                    checked: s.dpi == Some(d),
                    activate: Box::new(move |t: &mut SlTray| {
                        let _ = t.cmd.send(Cmd::SetDpi(d));
                    }),
                    ..Default::default()
                }
                .into()
            })
            .collect();
        let save_dpi = s.dpi;
        vec![
            label(ui::battery_label(s)),
            MenuItem::Separator,
            SubMenu {
                label: ui::dpi_label(s, &self.presets),
                enabled: ok,
                submenu: dpi_items,
                ..Default::default()
            }
            .into(),
            item("Set DPI…", ok, |t| {
                let (cmd, caps, cur) = (t.cmd.clone(), t.state.caps.clone(), t.state.dpi);
                // dialogs block, so never run them inside the D-Bus callback
                std::thread::spawn(move || ask_dpi(cmd, caps, cur));
            }),
            confirm_sub(
                ui::save_label(s),
                ok && s.onboard && s.dpi.is_some(),
                "Yes, write it to the mouse (backup is made first)",
                move |t| {
                    if let Some(d) = save_dpi {
                        let _ = t.cmd.send(Cmd::SaveDpi(d));
                    }
                },
            ),
            MenuItem::Separator,
            SubMenu {
                label: "Recovery".into(),
                submenu: vec![
                    confirm_sub(
                        "Restore last backup".into(),
                        ok && s.onboard,
                        "Yes, restore the newest backup",
                        |t| {
                            let _ = t.cmd.send(Cmd::RestoreLatest);
                        },
                    ),
                    confirm_sub(
                        "Restore factory profile".into(),
                        ok && s.onboard,
                        "Yes, copy the factory profile to the mouse",
                        |t| {
                            let _ = t.cmd.send(Cmd::FactoryReset);
                        },
                    ),
                    MenuItem::Separator,
                    item("Open backups folder", true, |_| {
                        let d = backup::dir();
                        let _ = std::fs::create_dir_all(&d);
                        platform::open_path(&d);
                    }),
                ],
                ..Default::default()
            }
            .into(),
            item("Edit DPI presets…", true, |_| {
                let p = config::config_dir().join("config.toml");
                if !p.exists() {
                    Config::default().save();
                }
                platform::open_path(&p);
            }),
            item("Refresh now", true, |t| {
                let _ = t.cmd.send(Cmd::Refresh);
            }),
            CheckmarkItem {
                label: "Start on login".into(),
                checked: platform::autostart_enabled(),
                activate: Box::new(|_| {
                    let _ = platform::set_autostart(!platform::autostart_enabled());
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            item("Quit", true, |_| std::process::exit(0)),
        ]
    }
}

enum Asked {
    Value(String),
    Cancelled,
    NoDialogTool,
}

/// Text input via zenity (GNOME/GTK desktops) or kdialog (KDE).
fn input_box(title: &str, prompt: &str, initial: &str) -> Asked {
    use std::process::Command;
    let tries: [(&str, Vec<&str>); 2] = [
        (
            "zenity",
            vec!["--entry", "--title", title, "--text", prompt, "--entry-text", initial],
        ),
        ("kdialog", vec!["--title", title, "--inputbox", prompt, initial]),
    ];
    for (prog, args) in tries {
        match Command::new(prog).args(&args).output() {
            Ok(out) if out.status.success() => {
                return Asked::Value(String::from_utf8_lossy(&out.stdout).trim().to_string())
            }
            Ok(_) => return Asked::Cancelled,
            Err(_) => continue, // not installed, try the next one
        }
    }
    Asked::NoDialogTool
}

fn ask_dpi(cmd: Sender<Cmd>, caps: Option<hidpp::dpi::DpiCaps>, current: Option<u16>) {
    let mut text = current.map(|d| d.to_string()).unwrap_or_default();
    let mut prompt = ui::set_dpi_prompt(caps.as_ref());
    loop {
        match input_box(ui::SET_DPI_TITLE, &prompt, &text) {
            Asked::Value(t) => match ui::parse_dpi(&t, caps.as_ref()) {
                Ok(v) => {
                    let _ = cmd.send(Cmd::SetDpi(v));
                    return;
                }
                Err(e) => {
                    // show the problem inside the next prompt
                    prompt = format!("{e}\n\n{}", ui::set_dpi_prompt(caps.as_ref()));
                    text = t;
                }
            },
            Asked::Cancelled => return,
            Asked::NoDialogTool => {
                notify(
                    ui::SET_DPI_TITLE.into(),
                    "Install 'zenity' (GNOME/GTK) or 'kdialog' (KDE) for the DPI input window, \
                     or run: superlight-tray --set-dpi <value>"
                        .into(),
                    true,
                );
                return;
            }
        }
    }
}

fn notify(title: String, body: String, error: bool) {
    std::thread::spawn(move || {
        let r = notify_rust::Notification::new()
            .appname("Superlight Tray")
            .summary(&title)
            .body(&body)
            .icon(if error { "dialog-error" } else { "input-mouse" })
            .timeout(if error { 0 } else { 8000 })
            .show();
        if let Err(e) = r {
            log::warn!("notification failed ({e}): {title}: {body}");
        }
    });
}

pub fn run(cfg: Arc<Mutex<Config>>) -> Result<()> {
    let (ev_tx, ev_rx) = unbounded::<Event>();
    let cmd = worker::spawn(cfg.clone(), move |e| {
        let _ = ev_tx.send(e);
    });
    let presets = cfg.lock().unwrap().presets.clone();
    let tray = SlTray {
        state: State::default(),
        presets,
        cmd,
    };
    let handle = tray.spawn().context(
        "could not create the tray icon. Is a StatusNotifierItem host running? \
         (On GNOME install and enable the AppIndicator extension.)",
    )?;
    for e in ev_rx {
        match e {
            Event::State(s) => {
                handle.update(|t| t.state = s);
            }
            Event::Message { title, body, error } => notify(title, body, error),
        }
    }
    Ok(())
}
