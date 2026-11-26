//! Dev tool: writes raw RGBA tray icons so they can be inspected.
//! cargo run -p superlight-tray --example icon_preview -- <out_dir>
#[allow(dead_code)]
mod icon {
    include!("../src/icon.rs");
}

fn main() {
    let out = std::path::PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| ".".into()));
    for size in [16u32, 24, 32] {
        for (name, p, ch) in [("100", Some(100), false), ("56", Some(56), false), ("23", Some(23), false), ("7", Some(7), false), ("80c", Some(80), true), ("off", None, false)] {
            let px = icon::render(p, ch, size);
            std::fs::write(out.join(format!("icon_{size}_{name}.rgba")), px).unwrap();
        }
    }
}
