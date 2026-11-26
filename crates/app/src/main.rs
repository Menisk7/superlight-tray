#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod backup;
mod cli;
mod config;
mod icon;
#[cfg(windows)]
mod input_windows;
mod platform;
#[cfg(target_os = "linux")]
mod tray_linux;
#[cfg(windows)]
mod tray_windows;
mod ui;
mod worker;

use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if !args.is_empty() {
        attach_console();
        init_log(true);
        let code = match run_cli(&args) {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("error: {e:#}");
                1
            }
        };
        std::process::exit(code);
    }
    init_log(false);
    log::info!("superlight-tray {} starting", env!("CARGO_PKG_VERSION"));
    if !platform::single_instance() {
        log::warn!("already running");
        return;
    }
    let cfg = std::sync::Arc::new(std::sync::Mutex::new(config::Config::load()));
    #[cfg(windows)]
    let r = tray_windows::run(cfg);
    #[cfg(target_os = "linux")]
    let r = tray_linux::run(cfg);
    if let Err(e) = r {
        log::error!("tray failed: {e:#}");
        eprintln!("tray failed: {e:#}");
        std::process::exit(1);
    }
}

fn run_cli(args: &[String]) -> anyhow::Result<()> {
    let num = |i: usize| -> anyhow::Result<u16> {
        args.get(i)
            .ok_or_else(|| anyhow::anyhow!("missing DPI value"))?
            .parse()
            .map_err(|_| anyhow::anyhow!("DPI must be a number"))
    };
    let yes = args.iter().any(|a| a == "--yes");
    match args[0].as_str() {
        "--probe" => cli::probe(),
        "--set-dpi" => cli::set_dpi(num(1)?),
        "--save-dpi" => cli::save_dpi(num(1)?, !yes),
        "--dump" => cli::dump(),
        "--restore" => {
            let f = args.get(1).ok_or_else(|| anyhow::anyhow!("missing backup file"))?;
            cli::restore(&PathBuf::from(f), !yes)
        }
        "--factory-reset-profile" => cli::factory_reset(!yes),
        #[cfg(windows)]
        "--ask-dpi" => {
            // opens the same input window as the tray's "Set DPI…" and prints the result
            let caps = hidpp::dpi::parse_dpi_list(&[100, 0xE000 | 50, 25600, 0]).ok();
            let t = input_windows::input_box(ui::SET_DPI_TITLE, &ui::set_dpi_prompt(caps.as_ref()), "1600");
            println!("{:?}", t.map(|t| ui::parse_dpi(&t, caps.as_ref())));
            Ok(())
        }
        "--backups" => {
            println!("{}", backup::dir().display());
            Ok(())
        }
        "--version" | "-V" => {
            println!("superlight-tray {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        _ => {
            print!("{}", cli::HELP);
            Ok(())
        }
    }
}

fn init_log(cli: bool) {
    use simplelog::*;
    let level = if std::env::var_os("SUPERLIGHT_DEBUG").is_some() {
        LevelFilter::Debug
    } else {
        LevelFilter::Info
    };
    let mut loggers: Vec<Box<dyn SharedLogger>> = Vec::new();
    let dir = config::data_dir();
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("superlight-tray.log");
    if std::fs::metadata(&path).map(|m| m.len() > 1_000_000).unwrap_or(false) {
        let _ = std::fs::rename(&path, dir.join("superlight-tray.old.log"));
    }
    if let Ok(f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        loggers.push(WriteLogger::new(level, Config::default(), f));
    }
    if cli {
        loggers.push(TermLogger::new(
            LevelFilter::Warn,
            Config::default(),
            TerminalMode::Stderr,
            ColorChoice::Never,
        ));
    }
    let _ = CombinedLogger::init(loggers);
}

#[cfg(windows)]
fn attach_console() {
    use windows_sys::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
    unsafe {
        AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

#[cfg(not(windows))]
fn attach_console() {}
