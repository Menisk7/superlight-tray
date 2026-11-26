//! Windows tray: `tray-icon` + a plain Win32 message loop.

use std::sync::{Arc, Mutex};

use anyhow::Result;
use crossbeam_channel::unbounded;
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{Icon, TrayIconBuilder};
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use crate::config::{self, Config};
use crate::worker::{self, Cmd, Event, State};
use crate::{backup, icon, platform, ui};

const WM_WAKE: u32 = WM_APP + 1;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn message_box(title: &str, body: &str, flags: u32) -> i32 {
    let (t, b) = (wide(title), wide(body));
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            b.as_ptr(),
            t.as_ptr(),
            flags | MB_TOPMOST | MB_SETFOREGROUND,
        )
    }
}

fn confirm(title: &str, body: &str) -> bool {
    message_box(title, body, MB_YESNO | MB_ICONWARNING | MB_DEFBUTTON2) == IDYES
}

/// Non-blocking info/error popup.
fn notify(title: String, body: String, error: bool) {
    std::thread::spawn(move || {
        message_box(
            &title,
            &body,
            if error { MB_ICONERROR } else { MB_ICONINFORMATION } | MB_OK,
        );
    });
}

fn tray_icon_for(s: &State) -> Option<Icon> {
    let size = unsafe { GetSystemMetrics(SM_CXSMICON) }.clamp(16, 64) as u32;
    Icon::from_rgba(
        icon::render(ui::icon_percent(s), ui::icon_charging(s), size),
        size,
        size,
    )
    .ok()
}

struct Items {
    battery: MenuItem,
    dpi: Submenu,
    presets: Vec<(u16, CheckMenuItem)>,
    save: MenuItem,
    restore: MenuItem,
    factory: MenuItem,
    open_backups: MenuItem,
    edit_config: MenuItem,
    refresh: MenuItem,
    autostart: CheckMenuItem,
    quit: MenuItem,
}

pub fn run(cfg: Arc<Mutex<Config>>) -> Result<()> {
    let tid = unsafe { GetCurrentThreadId() };
    let wake = move || unsafe {
        PostThreadMessageW(tid, WM_WAKE, 0, 0);
    };

    let (ev_tx, ev_rx) = unbounded::<Event>();
    let cmd = worker::spawn(cfg.clone(), move |e| {
        let _ = ev_tx.send(e);
        wake();
    });

    let (menu_tx, menu_rx) = unbounded::<MenuId>();
    MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
        let _ = menu_tx.send(e.id);
        wake();
    }));

    let presets = cfg.lock().unwrap().presets.clone();
    let it = Items {
        battery: MenuItem::new("Looking for mouse…", false, None),
        dpi: Submenu::new("DPI", true),
        presets: presets
            .iter()
            .map(|&d| (d, CheckMenuItem::new(format!("{d} DPI"), true, false, None)))
            .collect(),
        save: MenuItem::new(ui::save_label(&State::default()), false, None),
        restore: MenuItem::new("Restore last backup…", true, None),
        factory: MenuItem::new("Restore factory profile…", true, None),
        open_backups: MenuItem::new("Open backups folder", true, None),
        edit_config: MenuItem::new("Edit DPI presets…", true, None),
        refresh: MenuItem::new("Refresh now", true, None),
        autostart: CheckMenuItem::new("Start with Windows", true, platform::autostart_enabled(), None),
        quit: MenuItem::new("Quit", true, None),
    };
    for (_, p) in &it.presets {
        it.dpi.append(p)?;
    }
    let recovery = Submenu::new("Recovery", true);
    recovery.append_items(&[
        &it.restore,
        &it.factory,
        &PredefinedMenuItem::separator(),
        &it.open_backups,
    ])?;
    let menu = Menu::new();
    menu.append_items(&[
        &it.battery,
        &PredefinedMenuItem::separator(),
        &it.dpi,
        &it.save,
        &PredefinedMenuItem::separator(),
        &recovery,
        &it.edit_config,
        &it.refresh,
        &it.autostart,
        &PredefinedMenuItem::separator(),
        &it.quit,
    ])?;

    let mut state = State::default();
    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(true)
        .with_tooltip(ui::tooltip(&state))
        .with_icon(tray_icon_for(&state).expect("icon"))
        .build()?;

    // periodic wake-up so nothing is missed while a menu's modal loop runs
    unsafe { SetTimer(std::ptr::null_mut(), 0, 500, None) };

    let apply = |s: &State, it: &Items| {
        it.battery.set_text(ui::battery_label(s));
        it.dpi.set_text(ui::dpi_label(s, &presets));
        let ok = s.connected && !s.busy;
        it.dpi.set_enabled(ok);
        for (d, p) in &it.presets {
            p.set_checked(s.dpi == Some(*d));
            p.set_enabled(ok && s.caps.as_ref().is_none_or(|c| c.supports(*d)));
        }
        it.save.set_text(ui::save_label(s));
        it.save.set_enabled(ok && s.onboard && s.dpi.is_some());
        it.restore.set_enabled(ok && s.onboard);
        it.factory.set_enabled(ok && s.onboard);
        let _ = tray.set_tooltip(Some(ui::tooltip(s)));
        let _ = tray.set_icon(tray_icon_for(s));
    };

    let mut msg: MSG = unsafe { std::mem::zeroed() };
    loop {
        let r = unsafe { GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) };
        if r <= 0 {
            break;
        }
        unsafe {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        let mut changed = false;
        while let Ok(e) = ev_rx.try_recv() {
            match e {
                Event::State(s) => {
                    state = s;
                    changed = true;
                }
                Event::Message { title, body, error } => notify(title, body, error),
            }
        }
        if changed {
            apply(&state, &it);
        }

        while let Ok(id) = menu_rx.try_recv() {
            if id == it.quit.id() {
                return Ok(());
            } else if let Some((d, _)) = it.presets.iter().find(|(_, p)| *p.id() == id) {
                let _ = cmd.send(Cmd::SetDpi(*d));
                apply(&state, &it); // undo the automatic check toggle until the mouse confirms
            } else if id == it.save.id() {
                if let Some(d) = state.dpi {
                    if confirm(&format!("Save {d} DPI to the mouse?"), ui::SAVE_CONFIRM) {
                        let _ = cmd.send(Cmd::SaveDpi(d));
                    }
                }
            } else if id == it.restore.id() {
                let body = match backup::latest() {
                    Some(p) => format!("{}\n\n{}", ui::RESTORE_CONFIRM, p.display()),
                    None => "No backup exists yet.".into(),
                };
                if backup::latest().is_some() && confirm("Restore last backup", &body) {
                    let _ = cmd.send(Cmd::RestoreLatest);
                } else if backup::latest().is_none() {
                    notify("Restore".into(), body, false);
                }
            } else if id == it.factory.id() {
                if confirm("Restore factory profile", ui::FACTORY_CONFIRM) {
                    let _ = cmd.send(Cmd::FactoryReset);
                }
            } else if id == it.open_backups.id() {
                let d = backup::dir();
                let _ = std::fs::create_dir_all(&d);
                platform::open_path(&d);
            } else if id == it.edit_config.id() {
                let c = cfg.lock().unwrap().clone();
                c.save();
                let p = config::config_dir().join("config.toml");
                platform::edit_file(&p);
                notify(
                    "DPI presets".into(),
                    "Edit 'presets' in the file that opened, save it, then restart Superlight Tray.".into(),
                    false,
                );
            } else if id == it.refresh.id() {
                let _ = cmd.send(Cmd::Refresh);
            } else if id == it.autostart.id() {
                let want = it.autostart.is_checked();
                if let Err(e) = platform::set_autostart(want) {
                    notify("Autostart".into(), e.to_string(), true);
                }
                it.autostart.set_checked(platform::autostart_enabled());
            }
        }
    }
    Ok(())
}
