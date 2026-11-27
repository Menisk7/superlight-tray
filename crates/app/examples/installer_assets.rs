//! Generates the installer artwork from the tray icon renderer (the "100" battery logo):
//!   superlight-tray.ico   (installer + Start menu icon)
//!   wizard-small.bmp      (logo in the installer's header)
//! cargo run -p superlight-tray --example installer_assets -- <out_dir>
#[allow(dead_code)]
mod icon {
    include!("../src/icon.rs");
}

/// 24-bit BMP, alpha blended onto `bg`.
fn bmp(size: u32, bg: [u8; 3]) -> Vec<u8> {
    let rgba = icon::render(Some(100), false, size);
    let s = size as usize;
    let row = (s * 3).div_ceil(4) * 4;
    let data_len = row * s;
    let mut out = Vec::with_capacity(54 + data_len);
    out.extend(b"BM");
    out.extend(((54 + data_len) as u32).to_le_bytes());
    out.extend([0u8; 4]);
    out.extend(54u32.to_le_bytes());
    out.extend(40u32.to_le_bytes());
    out.extend((s as i32).to_le_bytes());
    out.extend((s as i32).to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend(24u16.to_le_bytes());
    out.extend([0u8; 24]);
    for y in (0..s).rev() {
        let start = out.len();
        for x in 0..s {
            let i = (y * s + x) * 4;
            let a = rgba[i + 3] as u32;
            let mix = |c: u8, b: u8| ((c as u32 * a + b as u32 * (255 - a)) / 255) as u8;
            // BMP is BGR
            out.extend([mix(rgba[i + 2], bg[2]), mix(rgba[i + 1], bg[1]), mix(rgba[i], bg[0])]);
        }
        out.resize(start + row, 0);
    }
    out
}

/// Minimal PNG writer (RGBA, uncompressed deflate blocks) — no image crate needed.
fn png(size: u32) -> Vec<u8> {
    fn crc32(data: &[u8]) -> u32 {
        let mut c = 0xFFFF_FFFFu32;
        for &b in data {
            c ^= b as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            }
        }
        !c
    }
    fn chunk(out: &mut Vec<u8>, kind: &[u8], data: &[u8]) {
        out.extend((data.len() as u32).to_be_bytes());
        let mut body = kind.to_vec();
        body.extend(data);
        out.extend(&body);
        out.extend(crc32(&body).to_be_bytes());
    }
    let rgba = icon::render(Some(100), false, size);
    let s = size as usize;
    let mut raw = Vec::with_capacity((s * 4 + 1) * s);
    for y in 0..s {
        raw.push(0); // filter: none
        raw.extend(&rgba[y * s * 4..(y + 1) * s * 4]);
    }
    // zlib stream with stored blocks
    let mut z = vec![0x78, 0x01];
    for (i, block) in raw.chunks(65535).enumerate() {
        let last = (i + 1) * 65535 >= raw.len();
        z.push(last as u8);
        z.extend((block.len() as u16).to_le_bytes());
        z.extend((!(block.len() as u16)).to_le_bytes());
        z.extend(block);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &x in &raw {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    z.extend(((b << 16) | a).to_be_bytes());
    let mut out = vec![0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1A, b'\n'];
    let mut ihdr = Vec::new();
    ihdr.extend(size.to_be_bytes());
    ihdr.extend(size.to_be_bytes());
    ihdr.extend([8, 6, 0, 0, 0]); // 8-bit RGBA
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}

fn main() {
    if std::env::args().nth(1).as_deref() == Some("--logo") {
        // README logo: cargo run -p superlight-tray --example installer_assets -- --logo assets/logo.png
        let path = std::env::args().nth(2).unwrap_or_else(|| "logo.png".into());
        std::fs::write(&path, png(128)).unwrap();
        println!("wrote {path}");
        return;
    }
    let dir = std::path::PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| ".".into()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("superlight-tray.ico"),
        icon::ico(&[16, 20, 24, 32, 40, 48, 64, 256]),
    )
    .unwrap();
    // dark tile so the white digits read well in the installer header
    std::fs::write(dir.join("wizard-small.bmp"), bmp(58, [32, 32, 32])).unwrap();
    println!("wrote assets to {}", dir.display());
}
