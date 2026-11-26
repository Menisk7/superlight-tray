//! Command-line tools. They work without the tray, so recovery is always possible.

use std::io::{BufRead, Write};
use std::path::Path;

use anyhow::{anyhow, bail, Context, Result};
use hidpp::{battery, dpi, feature, onboard, Device};

use crate::backup;

pub const HELP: &str = "\
superlight-tray - tray app for the Logitech PRO X Superlight

USAGE:
  superlight-tray                     start the tray app
  superlight-tray --probe             read-only report about the mouse
  superlight-tray --set-dpi <N>       set DPI live (RAM only, lost at power-off)
  superlight-tray --save-dpi <N>      save DPI into the onboard profile (backs up first)
  superlight-tray --dump              back up all onboard profile sectors to files
  superlight-tray --restore <file>    write a backup .json back to the mouse
  superlight-tray --factory-reset-profile
                                      copy the factory ROM profile over the user profile
  superlight-tray --backups           print the backup folder
  superlight-tray --version
";

/// Open the first Superlight we can find (receiver first, then cable).
pub fn connect() -> hidpp::Result<Device> {
    let api = hidapi::HidApi::new()?;
    let mut last = hidpp::Error::NotFound;
    for c in hidpp::transport::candidates(&api) {
        match hidpp::transport::HidTransport::open(&api, &c).and_then(|t| Device::open(Box::new(t))) {
            Ok(d) => return Ok(d),
            Err(e) => {
                log::debug!("candidate {:04X}: {e}", c.pid);
                last = e;
            }
        }
    }
    Err(last)
}

fn feature_name(id: u16) -> &'static str {
    match id {
        0x0000 => "Root",
        0x0001 => "Feature set",
        0x0003 => "Firmware info",
        0x0005 => "Device name",
        0x00C2 => "DFU control (blocked)",
        0x00D0 => "DFU (blocked)",
        0x1000 => "Battery status",
        0x1001 => "Battery voltage",
        0x1004 => "Unified battery",
        0x1802 => "Device reset (blocked)",
        0x1803 => "GPIO access (blocked)",
        0x1806 => "Device properties (blocked)",
        0x1814 => "Change host",
        0x1815 => "Hosts info",
        0x1B04 => "Reprogrammable keys",
        0x1D4B => "Wireless status",
        0x1F20 => "ADC measurement",
        0x2201 => "Adjustable DPI",
        0x2250 => "Analysis mode",
        0x8060 => "Report rate",
        0x8061 => "Extended report rate",
        0x8100 => "Onboard profiles",
        0x8110 => "Mouse button spy",
        _ => "",
    }
}

pub fn probe() -> Result<()> {
    println!("superlight-tray {} probe (read-only)\n", env!("CARGO_PKG_VERSION"));
    let api = hidapi::HidApi::new()?;
    let cands = hidpp::transport::candidates(&api);
    if cands.is_empty() {
        bail!("no Logitech LIGHTSPEED receiver or wired Superlight found");
    }
    for c in &cands {
        println!(
            "found 046D:{:04X} ({:?}), {} HID++ collection(s)",
            c.pid,
            c.kind,
            c.paths.len()
        );
    }
    let mut d = connect().context("could not talk to the mouse (is it switched on? is G HUB running?)")?;
    println!("\nlink:      {}", d.link());
    println!("dev index: {:#04x}", d.index);
    println!("protocol:  HID++ {}.{}", d.protocol.0, d.protocol.1);
    match d.name() {
        Ok(n) => println!("name:      {n}"),
        Err(e) => println!("name:      ({e})"),
    }

    println!("\nfeatures:");
    match d.list_features() {
        Ok(fs) => {
            for f in fs {
                println!(
                    "  [{:2}] 0x{:04X} v{} {}{}",
                    f.index,
                    f.id,
                    f.version,
                    feature_name(f.id),
                    if f.kind & 0x80 != 0 {
                        " (obsolete)"
                    } else if f.kind & 0x40 != 0 {
                        " (hidden)"
                    } else {
                        ""
                    }
                )
            }
        }
        Err(e) => println!("  ({e})"),
    }

    println!();
    match battery::read(&mut d) {
        Ok(b) => println!(
            "battery:   {}% {:?}{}",
            b.percent,
            b.charging,
            b.millivolts.map(|m| format!(" ({m} mV)")).unwrap_or_default()
        ),
        Err(e) => println!("battery:   ({e})"),
    }
    match dpi::caps(&mut d) {
        Ok(c) => println!("dpi caps:  {}-{} {:?}", c.min, c.max, c.ranges),
        Err(e) => println!("dpi caps:  ({e})"),
    }
    match dpi::get(&mut d) {
        Ok((cur, def)) => println!("dpi:       current {cur}, default {def}"),
        Err(e) => println!("dpi:       ({e})"),
    }

    if !d.has_feature(feature::ONBOARD_PROFILES) {
        println!("\nonboard profiles: not supported");
        return Ok(());
    }
    println!("\nonboard profiles:");
    let info = onboard::info(&mut d)?;
    println!("  {info:?}");
    println!(
        "  known format: {}",
        onboard::KNOWN_PROFILE_FORMATS.contains(&info.profile_format)
    );
    println!("  mode: {:?}", onboard::mode(&mut d));
    let cur = onboard::current_profile(&mut d);
    println!(
        "  current profile sector: {:?}",
        cur.as_ref().map(|s| format!("{s:#06x}"))
    );
    println!("  current dpi index: {:?}", onboard::current_dpi_index(&mut d));
    let size = info.sector_size;
    match onboard::read_sector(&mut d, 0, size) {
        Ok(dir) => {
            println!("  directory (sector 0x0000) crc ok: {}", onboard::sector_crc_ok(&dir));
            println!("  directory entries: {:?}", onboard::parse_directory(&dir));
            println!("  directory head: {}", backup::to_hex(&dir[..32.min(dir.len())]));
        }
        Err(e) => println!("  directory: ({e})"),
    }
    let mut sectors = vec![];
    if let Ok(s) = cur {
        sectors.push(s);
    }
    for i in 1..=info.profile_count.max(1) as u16 {
        for s in [i, onboard::rom_sector_for(&info, i)] {
            if !sectors.contains(&s) {
                sectors.push(s);
            }
        }
    }
    for s in sectors {
        match onboard::read_sector(&mut d, s, size) {
            Ok(data) => {
                let crc = onboard::sector_crc_ok(&data);
                println!("  sector {s:#06x}: crc ok {crc}, head {}", backup::to_hex(&data[..32]));
                if crc {
                    if let Ok(p) = onboard::parse_profile(&data) {
                        println!("      {p:?}");
                    }
                }
            }
            Err(e) => println!("  sector {s:#06x}: ({e})"),
        }
    }
    println!("\nNothing was written to the mouse.");
    Ok(())
}

pub fn set_dpi(v: u16) -> Result<()> {
    let mut d = connect()?;
    let caps = dpi::caps(&mut d)?;
    let got = dpi::set(&mut d, &caps, v)?;
    println!("DPI set to {got} (live; lost when the mouse powers off)");
    Ok(())
}

fn confirm(prompt: &str) -> Result<()> {
    print!("{prompt}\nType YES to continue: ");
    std::io::stdout().flush()?;
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;
    if line.trim() != "YES" {
        bail!("cancelled, nothing written");
    }
    Ok(())
}

pub fn save_dpi(v: u16, ask: bool) -> Result<()> {
    let mut d = connect()?;
    let name = d.name().unwrap_or_default();
    let caps = dpi::caps(&mut d)?;
    if !caps.supports(v) {
        bail!("{v} DPI not supported ({}-{})", caps.min, caps.max);
    }
    if let Ok(b) = battery::read(&mut d) {
        if b.percent < 15 && b.charging == battery::Charging::Discharging {
            bail!(
                "battery is at {}%; charge the mouse before writing its memory",
                b.percent
            );
        }
    }
    if ask {
        confirm(&format!(
            "This writes {v} DPI into the mouse's onboard profile. A backup is made first."
        ))?;
    }
    let out = save_dpi_on(&mut d, &name, v)?;
    println!("{out}");
    Ok(())
}

/// Shared with the tray. Returns a human-readable result.
pub fn save_dpi_on(d: &mut Device, name: &str, v: u16) -> Result<String> {
    let mut path = None;
    let out = onboard::save_dpi(d, v, |snap| {
        let b = backup::from_snapshot(snap, name, &format!("before saving {v} DPI"));
        path = Some(backup::write(&b)?);
        Ok(())
    })?;
    let caps = dpi::caps(d)?;
    let _ = dpi::set(d, &caps, v);
    let bp = path
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "(no change needed)".into());
    Ok(match out {
        onboard::WriteOutcome::Ok => format!("Saved {v} DPI to the mouse's memory and verified it.\nBackup: {bp}"),
        onboard::WriteOutcome::RolledBack(why) => {
            format!("The write could not be verified ({why}); the previous profile was restored and verified.\nBackup: {bp}")
        }
    })
}

pub fn dump() -> Result<()> {
    let mut d = connect()?;
    let name = d.name().unwrap_or_default();
    let p = dump_on(&mut d, &name, "manual dump")?;
    println!("Backup written to {}", p.display());
    Ok(())
}

pub fn dump_on(d: &mut Device, name: &str, reason: &str) -> Result<std::path::PathBuf> {
    let info = onboard::info(d)?;
    let mut sectors = vec![];
    let mut list: Vec<u16> = vec![0];
    for i in 1..=info.profile_count.max(1) as u16 {
        list.push(i);
        if i <= info.profile_count_oob as u16 {
            list.push(onboard::ROM_BASE | i);
        }
    }
    for s in list {
        match onboard::read_sector(d, s, info.sector_size) {
            Ok(data) => sectors.push(backup::SectorData::new(s, &data)),
            Err(e) => log::warn!("sector {s:#06x}: {e}"),
        }
    }
    if sectors.is_empty() {
        bail!("could not read any sector");
    }
    let b = backup::new_file(name, reason, info.profile_format, info.sector_size, sectors);
    Ok(backup::write(&b)?)
}

pub fn restore(file: &Path, ask: bool) -> Result<()> {
    let b = backup::read(file)?;
    let mut d = connect()?;
    if ask {
        let list: Vec<String> = b
            .sectors
            .iter()
            .filter(|s| s.sector != 0 && s.sector < onboard::ROM_BASE)
            .map(|s| format!("{:#06x}", s.sector))
            .collect();
        confirm(&format!(
            "Restore profile sector(s) {} from {} ({}).",
            list.join(", "),
            file.display(),
            b.created
        ))?;
    }
    println!("{}", restore_on(&mut d, &b)?);
    Ok(())
}

pub fn restore_on(d: &mut Device, b: &backup::BackupFile) -> Result<String> {
    let name = d.name().unwrap_or_default();
    // safety net: back up the current state before restoring
    let pre = dump_on(d, &name, "automatic backup before restore")?;
    let mut msgs = vec![format!("Current state backed up to {}", pre.display())];
    let mut any = false;
    for s in &b.sectors {
        if s.sector == 0 || s.sector >= onboard::ROM_BASE {
            continue; // directory and ROM are never written
        }
        if !s.crc_ok {
            msgs.push(format!("Skipped sector {:#06x}: backup copy has a bad CRC", s.sector));
            continue;
        }
        let data = s.bytes()?;
        let out = onboard::restore_sector(d, s.sector, &data).map_err(|e| anyhow!("sector {:#06x}: {e}", s.sector))?;
        msgs.push(format!("Sector {:#06x}: {:?}", s.sector, out));
        any = true;
    }
    if !any {
        bail!("backup contains no restorable profile sector");
    }
    Ok(msgs.join("\n"))
}

pub fn factory_reset(ask: bool) -> Result<()> {
    let mut d = connect()?;
    if ask {
        confirm("Copy the factory ROM profile over the active user profile. A backup is made first.")?;
    }
    println!("{}", factory_reset_on(&mut d)?);
    Ok(())
}

pub fn factory_reset_on(d: &mut Device) -> Result<String> {
    let name = d.name().unwrap_or_default();
    let pre = dump_on(d, &name, "automatic backup before factory reset")?;
    let (sector, out) = onboard::restore_factory_profile(d)?;
    Ok(format!(
        "Factory profile copied to sector {sector:#06x}: {out:?}\nPrevious state backed up to {}",
        pre.display()
    ))
}
