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

fn main() {
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
