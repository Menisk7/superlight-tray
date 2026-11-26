//! Onboard Profiles (0x8100): read the mouse's profile memory and, very carefully,
//! change the DPI stored in it.
//!
//! What can go wrong and how it is handled:
//! * Only *profile* sectors (< 0x0100) can be written. ROM sectors (0x01xx) and the
//!   profile directory (0x0000) are never written.
//! * Before any write the caller must persist a backup (the `backup` callback);
//!   if that fails, nothing is written.
//! * The new sector differs from the old one only in the DPI bytes and the CRC;
//!   anything else aborts.
//! * After writing, the sector is read back and compared byte by byte. On mismatch
//!   the original is written back and verified again.
//! * Worst case (both writes fail) the sector has a bad CRC. The firmware then
//!   ignores it and falls back to the factory ROM profile, which
//!   `restore_factory_profile` can copy back.

use crate::feature::ONBOARD_PROFILES;
use crate::{Device, Error, Result};

pub const ROM_BASE: u16 = 0x0100;
const CHUNK: usize = 16;

/// Profile formats whose layout (report rate, dpi indices, 5×LE16 DPI at 3..13) we
/// have verified against libratbag/Solaar.
pub const KNOWN_PROFILE_FORMATS: &[u8] = &[1, 2, 3, 4, 5];
const DPI_OFFSET: usize = 3;
const DPI_SLOTS: usize = 5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfilesInfo {
    pub memory_model: u8,
    pub profile_format: u8,
    pub macro_format: u8,
    pub profile_count: u8,
    pub profile_count_oob: u8,
    pub button_count: u8,
    pub sector_count: u8,
    pub sector_size: u16,
    pub mechanical_layout: u8,
    pub various_info: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Onboard,
    Host,
    Other(u8),
}

pub fn info(dev: &mut Device) -> Result<ProfilesInfo> {
    let r = dev.request(ONBOARD_PROFILES, 0x0, &[])?;
    Ok(ProfilesInfo {
        memory_model: r[0],
        profile_format: r[1],
        macro_format: r[2],
        profile_count: r[3],
        profile_count_oob: r[4],
        button_count: r[5],
        sector_count: r[6],
        sector_size: u16::from_be_bytes([r[7], r[8]]),
        mechanical_layout: r[9],
        various_info: r[10],
    })
}

pub fn mode(dev: &mut Device) -> Result<Mode> {
    Ok(match dev.request(ONBOARD_PROFILES, 0x2, &[])?[0] {
        1 => Mode::Onboard,
        2 => Mode::Host,
        m => Mode::Other(m),
    })
}

/// Sector number of the active profile (0x00nn user, 0x01nn ROM).
pub fn current_profile(dev: &mut Device) -> Result<u16> {
    let r = dev.request(ONBOARD_PROFILES, 0x4, &[])?;
    Ok(u16::from_be_bytes([r[0], r[1]]))
}

pub fn current_dpi_index(dev: &mut Device) -> Result<u8> {
    Ok(dev.request(ONBOARD_PROFILES, 0x9, &[])?[0])
}

/// Read a whole sector in 16-byte chunks. The last chunk is aligned to the end of
/// the sector so we never read past it.
pub fn read_sector(dev: &mut Device, sector: u16, size: u16) -> Result<Vec<u8>> {
    let size = size as usize;
    if size < CHUNK {
        return Err(Error::Protocol(format!("sector size {size} too small")));
    }
    let mut buf = vec![0u8; size];
    let mut off = 0;
    while off < size {
        let at = off.min(size - CHUNK);
        let s = sector.to_be_bytes();
        let o = (at as u16).to_be_bytes();
        let r = dev.request(ONBOARD_PROFILES, 0x5, &[s[0], s[1], o[0], o[1]])?;
        buf[at..at + CHUNK].copy_from_slice(&r);
        off += CHUNK;
    }
    Ok(buf)
}

/// CRC-16/CCITT-FALSE (init 0xFFFF, poly 0x1021), as used by the profile sectors.
pub fn crc16(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &b in data {
        crc ^= (b as u16) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 { (crc << 1) ^ 0x1021 } else { crc << 1 };
        }
    }
    crc
}

pub fn sector_crc_ok(data: &[u8]) -> bool {
    if data.len() < 3 {
        return false;
    }
    let n = data.len();
    crc16(&data[..n - 2]) == u16::from_be_bytes([data[n - 2], data[n - 1]])
}

fn set_crc(data: &mut [u8]) {
    let n = data.len();
    let c = crc16(&data[..n - 2]).to_be_bytes();
    data[n - 2] = c[0];
    data[n - 1] = c[1];
}

/// Directory entry from sector 0x0000.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    pub sector: u16,
    pub enabled: bool,
}

pub fn parse_directory(data: &[u8]) -> Vec<DirEntry> {
    let mut out = Vec::new();
    for e in data.chunks(4) {
        if e.len() < 4 {
            break;
        }
        let sector = u16::from_be_bytes([e[0], e[1]]);
        if sector == 0xFFFF || sector == 0 {
            break;
        }
        out.push(DirEntry { sector, enabled: e[2] != 0 });
    }
    out
}

/// The parts of a profile sector we read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileView {
    pub report_rate: u8,
    pub default_dpi_index: u8,
    pub shift_dpi_index: u8,
    pub dpi: [u16; DPI_SLOTS],
}

pub fn parse_profile(data: &[u8]) -> Result<ProfileView> {
    if data.len() < 32 {
        return Err(Error::Protocol("profile sector too short".into()));
    }
    let mut dpi = [0u16; DPI_SLOTS];
    for (i, d) in dpi.iter_mut().enumerate() {
        let o = DPI_OFFSET + 2 * i;
        *d = u16::from_le_bytes([data[o], data[o + 1]]);
    }
    Ok(ProfileView { report_rate: data[0], default_dpi_index: data[1], shift_dpi_index: data[2], dpi })
}

/// Build the new sector content with the default DPI slot set to `new_dpi`.
/// Refuses anything that would change more than the DPI slot and the CRC.
pub fn build_dpi_update(old: &[u8], new_dpi: u16) -> Result<Vec<u8>> {
    if !sector_crc_ok(old) {
        return Err(Error::Blocked("current profile has a bad CRC; refusing to modify it".into()));
    }
    let view = parse_profile(old)?;
    let slot = view.default_dpi_index as usize;
    if slot >= DPI_SLOTS {
        return Err(Error::Blocked(format!("default DPI index {slot} out of range")));
    }
    let cur = view.dpi[slot];
    if cur == 0 || cur == 0xFFFF || !(50..=50000).contains(&cur) {
        return Err(Error::Blocked(format!("default DPI slot holds {cur:#06x}; layout not as expected")));
    }
    if !(100..=32000).contains(&new_dpi) {
        return Err(Error::Blocked(format!("{new_dpi} DPI out of safe range")));
    }
    let mut new = old.to_vec();
    let o = DPI_OFFSET + 2 * slot;
    new[o..o + 2].copy_from_slice(&new_dpi.to_le_bytes());
    set_crc(&mut new);
    check_diff(old, &new, &[o, o + 1])?;
    Ok(new)
}

/// Every differing byte must be in `allowed` or the trailing CRC.
fn check_diff(old: &[u8], new: &[u8], allowed: &[usize]) -> Result<()> {
    if old.len() != new.len() {
        return Err(Error::Blocked("sector length changed".into()));
    }
    let n = old.len();
    for i in 0..n {
        if old[i] != new[i] && !allowed.contains(&i) && i < n - 2 {
            return Err(Error::Blocked(format!("unexpected change at byte {i}")));
        }
    }
    Ok(())
}

/// Everything needed to undo a save.
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub info: ProfilesInfo,
    pub sector: u16,
    pub data: Vec<u8>,
    pub rom_sector: u16,
    pub rom_data: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteOutcome {
    /// New data written and verified.
    Ok,
    /// Write did not verify; original restored and verified.
    RolledBack(String),
}

/// Checks that make a write reasonable at all.
fn preflight(dev: &mut Device) -> Result<(ProfilesInfo, u16)> {
    let info = info(dev)?;
    if !KNOWN_PROFILE_FORMATS.contains(&info.profile_format) {
        return Err(Error::Blocked(format!("unknown profile format {}", info.profile_format)));
    }
    if !(64..=4096).contains(&info.sector_size) {
        return Err(Error::Blocked(format!("unexpected sector size {}", info.sector_size)));
    }
    if mode(dev)? != Mode::Onboard {
        return Err(Error::Blocked(
            "the mouse is in host mode (another app like G HUB controls it); onboard profiles are not in use".into(),
        ));
    }
    let sector = current_profile(dev)?;
    if sector == 0 || sector >= ROM_BASE {
        return Err(Error::Blocked(format!(
            "active profile is sector {sector:#06x} (factory ROM); there is no user profile to update"
        )));
    }
    Ok((info, sector))
}

/// Read the active profile twice (link must be stable) plus its ROM twin.
pub fn snapshot(dev: &mut Device) -> Result<Snapshot> {
    let (info, sector) = preflight(dev)?;
    let a = read_sector(dev, sector, info.sector_size)?;
    let b = read_sector(dev, sector, info.sector_size)?;
    if a != b {
        return Err(Error::Verify("two reads of the profile differ; wireless link unstable".into()));
    }
    let rom_sector = rom_sector_for(&info, sector);
    let rom_data = read_sector(dev, rom_sector, info.sector_size).ok();
    Ok(Snapshot { info, sector, data: a, rom_sector, rom_data })
}

/// Write a profile sector and verify. On mismatch, restore `original` and verify that.
fn write_verified(dev: &mut Device, sector: u16, size: u16, new: &[u8], original: &[u8]) -> Result<WriteOutcome> {
    if sector == 0 || sector >= ROM_BASE {
        return Err(Error::Blocked(format!("sector {sector:#06x} may not be written")));
    }
    if new.len() != size as usize || !sector_crc_ok(new) {
        return Err(Error::Blocked("refusing to write a sector with wrong size or CRC".into()));
    }
    let first = raw_write(dev, sector, new).and_then(|_| read_sector(dev, sector, size));
    match first {
        Ok(back) if back == new => return Ok(WriteOutcome::Ok),
        Ok(_) => log::error!("readback mismatch after writing sector {sector:#06x}"),
        Err(ref e) => log::error!("write of sector {sector:#06x} failed: {e}"),
    }
    let why = match &first {
        Ok(_) => "readback did not match".to_string(),
        Err(e) => e.to_string(),
    };
    // Roll back to the original (a good CRC is required or this can't help).
    if sector_crc_ok(original) && original.len() == new.len() {
        let restored = raw_write(dev, sector, original).and_then(|_| read_sector(dev, sector, size));
        if let Ok(back) = restored {
            if back == original {
                return Ok(WriteOutcome::RolledBack(why));
            }
        }
    }
    Err(Error::Verify(format!(
        "write failed ({why}) and the original could not be restored. The mouse will fall back to its \
         factory profile. Use 'Restore last backup' or 'Restore factory profile'."
    )))
}

fn raw_write(dev: &mut Device, sector: u16, data: &[u8]) -> Result<()> {
    let s = sector.to_be_bytes();
    let n = (data.len() as u16).to_be_bytes();
    dev.guarded_write_request(ONBOARD_PROFILES, 0x6, &[s[0], s[1], 0, 0, n[0], n[1]])?;
    for chunk in data.chunks(CHUNK) {
        let mut c = [0u8; CHUNK];
        c[..chunk.len()].copy_from_slice(chunk);
        dev.guarded_write_request(ONBOARD_PROFILES, 0x7, &c)?;
    }
    dev.guarded_write_request(ONBOARD_PROFILES, 0x8, &[])?;
    Ok(())
}

/// Save `new_dpi` as the default DPI of the active onboard profile.
/// `backup` must durably store the snapshot; if it errors, nothing is written.
pub fn save_dpi(
    dev: &mut Device,
    new_dpi: u16,
    backup: impl FnOnce(&Snapshot) -> std::io::Result<()>,
) -> Result<WriteOutcome> {
    let snap = snapshot(dev)?;
    let new = build_dpi_update(&snap.data, new_dpi)?;
    if new == snap.data {
        return Ok(WriteOutcome::Ok);
    }
    backup(&snap).map_err(|e| Error::Blocked(format!("could not write backup file, nothing changed: {e}")))?;
    write_verified(dev, snap.sector, snap.info.sector_size, &new, &snap.data)
}

/// Write a previously backed-up sector back. Validates size, CRC and target.
pub fn restore_sector(dev: &mut Device, sector: u16, data: &[u8]) -> Result<WriteOutcome> {
    let info = info(dev)?;
    if data.len() != info.sector_size as usize {
        return Err(Error::Blocked(format!(
            "backup is {} bytes but this mouse uses {}-byte sectors",
            data.len(),
            info.sector_size
        )));
    }
    if !sector_crc_ok(data) {
        return Err(Error::Blocked("backup has a bad CRC".into()));
    }
    parse_profile(data)?;
    let current = read_sector(dev, sector, info.sector_size)?;
    if current == data {
        return Ok(WriteOutcome::Ok);
    }
    write_verified(dev, sector, info.sector_size, data, &current)
}

/// ROM twin of a user sector. Devices with fewer ROM profiles than user profiles
/// (the Superlight has one) share ROM profile 1.
pub fn rom_sector_for(info: &ProfilesInfo, sector: u16) -> u16 {
    let n = sector & 0x00FF;
    if n >= 1 && n <= info.profile_count_oob as u16 {
        ROM_BASE | n
    } else {
        ROM_BASE | 1
    }
}

/// ROM sectors are stored without a CRC (trailing 0xFFFF). Return a copy with a
/// valid CRC, after checking that the content really looks like a profile.
pub fn rom_to_user_sector(rom: &[u8]) -> Result<Vec<u8>> {
    let n = rom.len();
    if n < 32 {
        return Err(Error::Verify("ROM sector too short".into()));
    }
    let mut out = rom.to_vec();
    if !sector_crc_ok(rom) {
        if rom[n - 2..] != [0xFF, 0xFF] {
            return Err(Error::Verify("factory ROM profile has a bad CRC; not copying it".into()));
        }
        set_crc(&mut out);
    }
    let v = parse_profile(&out)?;
    let idx = v.default_dpi_index as usize;
    if idx >= DPI_SLOTS || !(50..=50000).contains(&v.dpi[idx]) {
        return Err(Error::Verify("factory ROM profile does not look like a valid profile".into()));
    }
    Ok(out)
}

/// Copy the factory ROM profile over the active (or first) user profile sector.
pub fn restore_factory_profile(dev: &mut Device) -> Result<(u16, WriteOutcome)> {
    let info = info(dev)?;
    let mut sector = current_profile(dev).unwrap_or(1);
    if sector == 0 || sector >= ROM_BASE {
        sector = 1;
    }
    let rom = read_sector(dev, rom_sector_for(&info, sector), info.sector_size)?;
    let data = rom_to_user_sector(&rom)?;
    Ok((sector, restore_sector(dev, sector, &data)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::tests::fake_mouse;
    use crate::feature::*;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    const SIZE: u16 = 255;

    fn profile(dpi: [u16; 5], default: u8) -> Vec<u8> {
        let mut d = vec![0xAAu8; SIZE as usize];
        d[0] = 1;
        d[1] = default;
        d[2] = 0xFF;
        for (i, v) in dpi.iter().enumerate() {
            d[3 + 2 * i..5 + 2 * i].copy_from_slice(&v.to_le_bytes());
        }
        set_crc(&mut d);
        d
    }

    #[derive(Default)]
    struct Mem {
        sectors: HashMap<u16, Vec<u8>>,
        pending: Option<(u16, Vec<u8>, usize)>,
        mode: u8,
        current: u16,
        /// corrupt the next N writes
        corrupt_writes: usize,
        writes: Vec<u16>,
    }

    fn mouse(mem: Arc<Mutex<Mem>>) -> Device {
        fake_mouse(
            vec![(ADJUSTABLE_DPI, 5), (ONBOARD_PROFILES, 9)],
            Box::new(move |id, func, p| {
                let mut m = mem.lock().unwrap();
                if id != ONBOARD_PROFILES {
                    return Some(vec![]);
                }
                match func {
                    0 => Some(vec![1, 4, 1, 1, 0, 6, 16, (SIZE >> 8) as u8, SIZE as u8, 1, 0]),
                    2 => Some(vec![m.mode]),
                    4 => Some(m.current.to_be_bytes().to_vec()),
                    5 => {
                        let s = u16::from_be_bytes([p[0], p[1]]);
                        let o = u16::from_be_bytes([p[2], p[3]]) as usize;
                        let d = m.sectors.get(&s)?;
                        Some(d[o..o + 16].to_vec())
                    }
                    6 => {
                        let s = u16::from_be_bytes([p[0], p[1]]);
                        let n = u16::from_be_bytes([p[4], p[5]]) as usize;
                        m.pending = Some((s, Vec::new(), n));
                        Some(vec![])
                    }
                    7 => {
                        m.pending.as_mut()?.1.extend_from_slice(&p[..16]);
                        Some(vec![])
                    }
                    8 => {
                        let (s, mut d, n) = m.pending.take()?;
                        d.truncate(n);
                        if m.corrupt_writes > 0 {
                            m.corrupt_writes -= 1;
                            d[100] ^= 0xFF;
                        }
                        m.writes.push(s);
                        m.sectors.insert(s, d);
                        Some(vec![])
                    }
                    _ => None,
                }
            }),
        )
    }

    fn setup() -> (Arc<Mutex<Mem>>, Device) {
        let mut m = Mem { mode: 1, current: 1, ..Default::default() };
        m.sectors.insert(1, profile([800, 1600, 3200, 0, 0], 0));
        // ROM sector: no CRC stored, like the real Superlight
        let mut rom = profile([800, 1600, 3200, 6400, 0], 1);
        let n = rom.len();
        rom[n - 2] = 0xFF;
        rom[n - 1] = 0xFF;
        m.sectors.insert(0x0101, rom);
        let m = Arc::new(Mutex::new(m));
        let d = mouse(m.clone());
        (m, d)
    }

    #[test]
    fn crc_known_value() {
        assert_eq!(crc16(b"123456789"), 0x29B1);
    }

    #[test]
    fn dpi_update_touches_only_dpi_and_crc() {
        let old = profile([800, 1600, 0, 0, 0], 1);
        let new = build_dpi_update(&old, 1200).unwrap();
        assert!(sector_crc_ok(&new));
        assert_eq!(parse_profile(&new).unwrap().dpi, [800, 1200, 0, 0, 0]);
        let diffs: Vec<usize> = (0..old.len()).filter(|&i| old[i] != new[i]).collect();
        assert!(diffs.iter().all(|&i| i == 5 || i == 6 || i >= old.len() - 2), "{diffs:?}");
    }

    #[test]
    fn dpi_update_rejects_bad_input() {
        let mut bad_crc = profile([800, 0, 0, 0, 0], 0);
        bad_crc[50] ^= 1;
        assert!(build_dpi_update(&bad_crc, 800).is_err());
        assert!(build_dpi_update(&profile([800, 0, 0, 0, 0], 7), 800).is_err());
        assert!(build_dpi_update(&profile([0xFFFF, 0, 0, 0, 0], 0), 800).is_err());
        assert!(build_dpi_update(&profile([800, 0, 0, 0, 0], 0), 64000).is_err());
    }

    #[test]
    fn diff_guard() {
        let a = vec![0u8; 32];
        let mut b = a.clone();
        b[10] = 1;
        assert!(check_diff(&a, &b, &[3, 4]).is_err());
        assert!(check_diff(&a, &b, &[10]).is_ok());
    }

    #[test]
    fn directory_parse() {
        let d = [0, 1, 1, 0, 0, 2, 0, 0, 0xFF, 0xFF, 0, 0];
        assert_eq!(
            parse_directory(&d),
            vec![DirEntry { sector: 1, enabled: true }, DirEntry { sector: 2, enabled: false }]
        );
    }

    #[test]
    fn save_dpi_happy_path_backs_up_first() {
        let (mem, mut d) = setup();
        let mut backed_up = None;
        let out = save_dpi(&mut d, 1200, |s| {
            backed_up = Some(s.data.clone());
            Ok(())
        })
        .unwrap();
        assert_eq!(out, WriteOutcome::Ok);
        let m = mem.lock().unwrap();
        assert_eq!(parse_profile(&m.sectors[&1]).unwrap().dpi[0], 1200);
        assert_eq!(parse_profile(&backed_up.unwrap()).unwrap().dpi[0], 800);
        assert_eq!(m.writes, vec![1]);
    }

    #[test]
    fn failed_backup_means_no_write() {
        let (mem, mut d) = setup();
        let r = save_dpi(&mut d, 1200, |_| Err(std::io::Error::other("disk full")));
        assert!(matches!(r, Err(Error::Blocked(_))));
        assert!(mem.lock().unwrap().writes.is_empty());
    }

    #[test]
    fn corrupted_write_rolls_back() {
        let (mem, mut d) = setup();
        mem.lock().unwrap().corrupt_writes = 1;
        let out = save_dpi(&mut d, 1200, |_| Ok(())).unwrap();
        assert!(matches!(out, WriteOutcome::RolledBack(_)));
        let m = mem.lock().unwrap();
        assert_eq!(parse_profile(&m.sectors[&1]).unwrap().dpi[0], 800);
        assert!(sector_crc_ok(&m.sectors[&1]));
    }

    #[test]
    fn double_failure_is_reported_and_factory_restore_works() {
        let (mem, mut d) = setup();
        mem.lock().unwrap().corrupt_writes = 2;
        assert!(matches!(save_dpi(&mut d, 1200, |_| Ok(())), Err(Error::Verify(_))));
        assert!(!sector_crc_ok(&mem.lock().unwrap().sectors[&1]));
        let (sector, out) = restore_factory_profile(&mut d).unwrap();
        assert_eq!((sector, out), (1, WriteOutcome::Ok));
        let m = mem.lock().unwrap();
        let (a, b) = (&m.sectors[&1], &m.sectors[&0x0101]);
        assert!(sector_crc_ok(a));
        assert_eq!(a[..a.len() - 2], b[..b.len() - 2]);
    }

    #[test]
    fn refuses_host_mode_and_rom_profile() {
        let (mem, mut d) = setup();
        mem.lock().unwrap().mode = 2;
        assert!(matches!(save_dpi(&mut d, 1200, |_| Ok(())), Err(Error::Blocked(_))));
        {
            let mut m = mem.lock().unwrap();
            m.mode = 1;
            m.current = 0x0101;
        }
        assert!(matches!(save_dpi(&mut d, 1200, |_| Ok(())), Err(Error::Blocked(_))));
        assert!(mem.lock().unwrap().writes.is_empty());
    }

    #[test]
    fn rom_and_directory_never_written() {
        let (_mem, mut d) = setup();
        let p = profile([800, 0, 0, 0, 0], 0);
        assert!(write_verified(&mut d, 0x0101, SIZE, &p, &p).is_err());
        assert!(write_verified(&mut d, 0x0000, SIZE, &p, &p).is_err());
    }

    #[test]
    fn rom_without_crc_is_accepted_only_if_blank() {
        let mut rom = profile([800, 0, 0, 0, 0], 0);
        let n = rom.len();
        rom[n - 2] = 0xFF;
        rom[n - 1] = 0xFF;
        assert!(sector_crc_ok(&rom_to_user_sector(&rom).unwrap()));
        rom[n - 1] = 0x12;
        assert!(rom_to_user_sector(&rom).is_err());
        let mut junk = vec![0xFFu8; SIZE as usize];
        junk[1] = 0;
        assert!(rom_to_user_sector(&junk).is_err());
    }

    #[test]
    fn restore_validates_backup() {
        let (_mem, mut d) = setup();
        let mut p = profile([800, 0, 0, 0, 0], 0);
        p[20] ^= 1;
        assert!(restore_sector(&mut d, 1, &p).is_err());
        assert!(restore_sector(&mut d, 1, &p[..128]).is_err());
    }
}
