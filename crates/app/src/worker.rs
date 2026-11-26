//! Background thread that owns the HID connection. The UI only sends commands
//! and receives state snapshots, so a slow or sleeping mouse never freezes the tray.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};
use hidpp::battery::{self, Battery};
use hidpp::dpi::{self, DpiCaps};
use hidpp::{feature, Device};

use crate::{backup, cli, config::Config};

#[derive(Debug, Clone)]
pub enum Cmd {
    SetDpi(u16),
    SaveDpi(u16),
    RestoreLatest,
    FactoryReset,
    Refresh,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct State {
    pub connected: bool,
    pub receiver_present: bool,
    pub name: String,
    pub battery: Option<Battery>,
    pub dpi: Option<u16>,
    pub caps: Option<DpiCaps>,
    pub onboard: bool,
    pub busy: bool,
}

#[derive(Debug, Clone)]
pub enum Event {
    State(State),
    Message { title: String, body: String, error: bool },
}

pub fn spawn(cfg: Arc<Mutex<Config>>, notify: impl Fn(Event) + Send + 'static) -> Sender<Cmd> {
    let (tx, rx) = crossbeam_channel::unbounded();
    std::thread::Builder::new()
        .name("device".into())
        .spawn(move || {
            Worker {
                cfg,
                notify: Box::new(notify),
                dev: None,
                state: State::default(),
                api: None,
            }
            .run(rx)
        })
        .expect("spawn device thread");
    tx
}

struct Worker {
    cfg: Arc<Mutex<Config>>,
    notify: Box<dyn Fn(Event) + Send>,
    dev: Option<Device>,
    state: State,
    api: Option<hidapi::HidApi>,
}

impl Worker {
    fn run(mut self, rx: Receiver<Cmd>) {
        let mut next_poll = Instant::now();
        let mut sender = StateSender::default();
        loop {
            if Instant::now() >= next_poll {
                self.poll();
                let secs = if self.dev.is_some() {
                    self.cfg.lock().unwrap().poll_seconds
                } else {
                    5
                };
                next_poll = Instant::now() + Duration::from_secs(secs);
            }
            sender.send_if_changed(&self.state, &self.notify);
            let wait = next_poll.saturating_duration_since(Instant::now());
            match rx.recv_timeout(wait) {
                Ok(cmd) => {
                    self.state.busy = true;
                    sender.send_if_changed(&self.state, &self.notify);
                    self.handle(cmd);
                    self.state.busy = false;
                    if self.dev.is_none() {
                        next_poll = Instant::now();
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    }

    fn message(&self, title: &str, body: impl Into<String>, error: bool) {
        (self.notify)(Event::Message {
            title: title.into(),
            body: body.into(),
            error,
        });
    }

    fn connect(&mut self) {
        let api = match self.api.as_mut() {
            Some(a) => {
                let _ = a.refresh_devices();
                a
            }
            None => match hidapi::HidApi::new() {
                Ok(a) => self.api.insert(a),
                Err(e) => {
                    log::error!("hidapi init: {e}");
                    return;
                }
            },
        };
        let cands = hidpp::transport::candidates(api);
        self.state.receiver_present = !cands.is_empty();
        for c in cands {
            let opened = hidpp::transport::HidTransport::open(api, &c).and_then(|t| Device::open(Box::new(t)));
            match opened {
                Ok(mut d) => {
                    log::info!("connected via {} slot {}", d.link(), d.index);
                    self.state.name = d.name().unwrap_or_else(|_| "Superlight".into());
                    self.state.caps = dpi::caps(&mut d).ok();
                    self.state.onboard = d.has_feature(feature::ONBOARD_PROFILES);
                    self.state.connected = true;
                    self.dev = Some(d);
                    self.on_connected();
                    return;
                }
                Err(e) => log::debug!("046D:{:04X}: {e}", c.pid),
            }
        }
    }

    /// Re-apply the user's DPI after the mouse wakes up / reconnects.
    fn on_connected(&mut self) {
        let (reapply, want) = {
            let c = self.cfg.lock().unwrap();
            (c.reapply_on_connect, c.last_dpi)
        };
        let Some(d) = self.dev.as_mut() else { return };
        let cur = dpi::get(d).ok().map(|x| x.0);
        self.state.dpi = cur;
        if let (true, Some(want), Some(caps)) = (reapply, want, self.state.caps.clone()) {
            if cur != Some(want) {
                match dpi::set(d, &caps, want) {
                    Ok(v) => {
                        log::info!("re-applied {v} DPI");
                        self.state.dpi = Some(v);
                    }
                    Err(e) => log::warn!("re-apply DPI failed: {e}"),
                }
            }
        }
    }

    fn disconnected(&mut self, why: &hidpp::Error) {
        log::info!("mouse unavailable: {why}");
        self.dev = None;
        self.state.connected = false;
        self.state.dpi = None;
    }

    fn poll(&mut self) {
        if self.dev.is_none() {
            self.connect();
            if self.dev.is_none() {
                self.state.connected = false;
                return;
            }
        }
        let d = self.dev.as_mut().unwrap();
        match battery::read(d) {
            Ok(b) => {
                self.state.battery = Some(b);
                if let Ok((cur, _)) = dpi::get(d) {
                    self.state.dpi = Some(cur);
                }
            }
            Err(e) => self.disconnected(&e),
        }
    }

    fn handle(&mut self, cmd: Cmd) {
        if self.dev.is_none() {
            self.connect();
        }
        if matches!(cmd, Cmd::Refresh) {
            self.poll();
            return;
        }
        let Some(d) = self.dev.as_mut() else {
            self.message(
                "Mouse not connected",
                "Switch the mouse on (move it to wake it up) and try again.",
                true,
            );
            return;
        };
        match cmd {
            Cmd::SetDpi(v) => {
                let Some(caps) = self.state.caps.clone() else { return };
                match dpi::set(d, &caps, v) {
                    Ok(v) => {
                        self.state.dpi = Some(v);
                        let mut c = self.cfg.lock().unwrap();
                        c.last_dpi = Some(v);
                        c.save();
                    }
                    Err(e) => {
                        let offline = e.is_device_offline();
                        self.message("Could not change DPI", e.to_string(), true);
                        if offline {
                            self.disconnected(&e);
                        }
                    }
                }
            }
            Cmd::SaveDpi(v) => {
                if let Some(b) = self.state.battery {
                    if b.percent < 15 && b.charging == battery::Charging::Discharging {
                        self.message(
                            "Not saved",
                            format!("Battery is at {}%. Charge the mouse first.", b.percent),
                            true,
                        );
                        return;
                    }
                }
                let name = self.state.name.clone();
                match cli::save_dpi_on(d, &name, v) {
                    Ok(msg) => {
                        self.state.dpi = Some(v);
                        self.message("DPI saved to mouse", msg, false);
                    }
                    Err(e) => self.message("DPI not saved", format!("{e:#}"), true),
                }
            }
            Cmd::RestoreLatest => {
                let r = backup::latest()
                    .ok_or_else(|| anyhow::anyhow!("no backup found in {}", backup::dir().display()))
                    .and_then(|p| backup::read(&p).map(|b| (p, b)))
                    .and_then(|(p, b)| cli::restore_on(d, &b).map(|m| format!("Restored {}\n{m}", p.display())));
                match r {
                    Ok(m) => self.message("Backup restored", m, false),
                    Err(e) => self.message("Restore failed", format!("{e:#}"), true),
                }
                let _ = self.refresh_dpi();
            }
            Cmd::FactoryReset => {
                match cli::factory_reset_on(d) {
                    Ok(m) => self.message("Factory profile restored", m, false),
                    Err(e) => self.message("Factory reset failed", format!("{e:#}"), true),
                }
                let _ = self.refresh_dpi();
            }
            Cmd::Refresh => {}
        }
    }

    fn refresh_dpi(&mut self) -> Option<()> {
        let d = self.dev.as_mut()?;
        self.state.dpi = dpi::get(d).ok().map(|x| x.0);
        Some(())
    }
}

/// Sends a state snapshot to the UI only when it differs from the last one sent.
/// Every send goes through here, so the "busy" snapshot is remembered too and the
/// following "not busy" snapshot is never mistaken for "unchanged".
#[derive(Default)]
struct StateSender {
    last: Option<State>,
}

impl StateSender {
    fn send_if_changed(&mut self, s: &State, notify: &dyn Fn(Event)) {
        if self.last.as_ref() != Some(s) {
            notify(Event::State(s.clone()));
            self.last = Some(s.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn busy_flag_is_always_cleared_in_the_ui() {
        // Regression: a command that changes nothing but `busy` (e.g. saving the
        // DPI that is already active) left the menu greyed out forever.
        let seen = RefCell::new(Vec::<bool>::new());
        let notify = |e: Event| {
            if let Event::State(s) = e {
                seen.borrow_mut().push(s.busy);
            }
        };
        let mut sender = StateSender::default();
        let mut s = State {
            connected: true,
            dpi: Some(1600),
            ..Default::default()
        };
        sender.send_if_changed(&s, &notify);
        s.busy = true;
        sender.send_if_changed(&s, &notify);
        s.busy = false; // command finished, nothing else changed
        sender.send_if_changed(&s, &notify);
        sender.send_if_changed(&s, &notify); // no duplicate
        assert_eq!(*seen.borrow(), vec![false, true, false]);
    }
}
