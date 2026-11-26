//! Durable profile backups. Files are never overwritten; each save makes a new one.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use hidpp::onboard::{sector_crc_ok, Snapshot};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct BackupFile {
    pub format: u32,
    pub created_unix: u64,
    pub created: String,
    pub app_version: String,
    pub device: String,
    pub reason: String,
    pub profile_format: u8,
    pub sector_size: u16,
    pub sectors: Vec<SectorData>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SectorData {
    pub sector: u16,
    pub crc_ok: bool,
    pub hex: String,
}

impl SectorData {
    pub fn new(sector: u16, data: &[u8]) -> Self {
        Self { sector, crc_ok: sector_crc_ok(data), hex: to_hex(data) }
    }
    pub fn bytes(&self) -> Result<Vec<u8>> {
        from_hex(&self.hex)
    }
}

pub fn dir() -> PathBuf {
    crate::config::data_dir().join("backups")
}

pub fn from_snapshot(s: &Snapshot, device: &str, reason: &str) -> BackupFile {
    let mut sectors = vec![SectorData::new(s.sector, &s.data)];
    if let Some(rom) = &s.rom_data {
        sectors.push(SectorData::new(s.rom_sector, rom));
    }
    new_file(device, reason, s.info.profile_format, s.info.sector_size, sectors)
}

pub fn new_file(device: &str, reason: &str, profile_format: u8, sector_size: u16, sectors: Vec<SectorData>) -> BackupFile {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    BackupFile {
        format: 1,
        created_unix: now,
        created: utc_string(now),
        app_version: env!("CARGO_PKG_VERSION").into(),
        device: device.into(),
        reason: reason.into(),
        profile_format,
        sector_size,
        sectors,
    }
}

/// Write JSON + raw .bin files and fsync them. Returns the JSON path.
pub fn write(b: &BackupFile) -> std::io::Result<PathBuf> {
    let d = dir();
    fs::create_dir_all(&d)?;
    let stamp = utc_string(b.created_unix).replace([':', ' '], "").replace('-', "");
    let mut base = d.join(format!("backup-{stamp}"));
    let mut n = 1;
    while base.with_extension("json").exists() {
        n += 1;
        base = d.join(format!("backup-{stamp}-{n}"));
    }
    for s in &b.sectors {
        let bin = PathBuf::from(format!("{}-sector{:04x}.bin", base.display(), s.sector));
        let bytes = from_hex(&s.hex).map_err(std::io::Error::other)?;
        write_synced(&bin, &bytes)?;
    }
    let json = base.with_extension("json");
    let text = serde_json::to_string_pretty(b).map_err(std::io::Error::other)?;
    write_synced(&json, text.as_bytes())?;
    // read back to make sure the backup is really there
    let check: BackupFile = serde_json::from_slice(&fs::read(&json)?).map_err(std::io::Error::other)?;
    if check.sectors.len() != b.sectors.len() || check.sectors.iter().zip(&b.sectors).any(|(a, b)| a.hex != b.hex) {
        return Err(std::io::Error::other("backup readback mismatch"));
    }
    log::info!("backup written: {}", json.display());
    Ok(json)
}

fn write_synced(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let mut f = File::options().write(true).create_new(true).open(path)?;
    f.write_all(data)?;
    f.sync_all()
}

pub fn read(path: &Path) -> Result<BackupFile> {
    let b: BackupFile = serde_json::from_slice(&fs::read(path).with_context(|| format!("reading {}", path.display()))?)
        .context("not a superlight-tray backup file")?;
    if b.format != 1 {
        bail!("unsupported backup format {}", b.format);
    }
    Ok(b)
}

/// Most recent backup JSON in the backup directory.
pub fn latest() -> Option<PathBuf> {
    let mut v: Vec<PathBuf> = fs::read_dir(dir())
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    v.sort_by_key(|p| fs::metadata(p).and_then(|m| m.modified()).ok());
    v.pop()
}

pub fn to_hex(d: &[u8]) -> String {
    d.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn from_hex(s: &str) -> Result<Vec<u8>> {
    if s.len() % 2 != 0 {
        bail!("odd hex length");
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).context("bad hex"))
        .collect()
}

/// "YYYY-MM-DD HH:MM:SS" in UTC without a date crate.
pub fn utc_string(secs: u64) -> String {
    let days = (secs / 86400) as i64;
    let rem = secs % 86400;
    // civil-from-days (Howard Hinnant)
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}", rem / 3600, rem % 3600 / 60, rem % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_roundtrip() {
        let d = vec![0, 1, 0xAB, 0xFF];
        assert_eq!(from_hex(&to_hex(&d)).unwrap(), d);
        assert!(from_hex("abc").is_err());
    }

    #[test]
    fn dates() {
        assert_eq!(utc_string(0), "1970-01-01 00:00:00");
        assert_eq!(utc_string(1_795_000_000), "2026-11-18 11:06:40");
    }
}
