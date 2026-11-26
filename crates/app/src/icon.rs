// Tray icon renderer: a battery with the percentage drawn inside, in pure Rust
// (no font files). Output is RGBA, `size`×`size`.

/// 3×5 bitmap glyphs: five rows of three bits, MSB = left.
fn glyph(c: char) -> [u8; 5] {
    match c {
        '0' => [0b111, 0b101, 0b101, 0b101, 0b111],
        '1' => [0b010, 0b110, 0b010, 0b010, 0b111],
        '2' => [0b111, 0b001, 0b111, 0b100, 0b111],
        '3' => [0b111, 0b001, 0b111, 0b001, 0b111],
        '4' => [0b101, 0b101, 0b111, 0b001, 0b001],
        '5' => [0b111, 0b100, 0b111, 0b001, 0b111],
        '6' => [0b111, 0b100, 0b111, 0b101, 0b111],
        '7' => [0b111, 0b001, 0b001, 0b001, 0b001],
        '8' => [0b111, 0b101, 0b111, 0b101, 0b111],
        '9' => [0b111, 0b101, 0b111, 0b001, 0b111],
        '?' => [0b111, 0b001, 0b011, 0b000, 0b010],
        _ => [0b000, 0b000, 0b111, 0b000, 0b000], // '-'
    }
}

#[derive(Clone, Copy)]
struct Rgba([u8; 4]);

const RED_TEXT: Rgba = Rgba([255, 120, 110, 255]);
const WHITE: Rgba = Rgba([255, 255, 255, 255]);
const SHADOW: Rgba = Rgba([0, 0, 0, 230]);
const FRAME: Rgba = Rgba([235, 235, 235, 255]);
const EMPTY: Rgba = Rgba([40, 40, 40, 200]);
const GREEN: Rgba = Rgba([46, 160, 67, 255]);
const YELLOW: Rgba = Rgba([212, 160, 23, 255]);
const RED: Rgba = Rgba([218, 54, 51, 255]);
const BLUE: Rgba = Rgba([31, 111, 235, 255]);
const GRAY: Rgba = Rgba([110, 110, 110, 255]);

struct Canvas {
    s: usize,
    px: Vec<u8>,
}

impl Canvas {
    fn put(&mut self, x: i32, y: i32, c: Rgba) {
        if x >= 0 && y >= 0 && (x as usize) < self.s && (y as usize) < self.s {
            let i = (y as usize * self.s + x as usize) * 4;
            self.px[i..i + 4].copy_from_slice(&c.0);
        }
    }
    fn rect(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, c: Rgba) {
        for y in y0..y1 {
            for x in x0..x1 {
                self.put(x, y, c);
            }
        }
    }
}

/// `percent`: None = mouse not connected.
///
/// Layout: the percentage as large digits across the top, a small battery gauge
/// (track + fill + tip) along the bottom whose color shows the level.
pub fn render(percent: Option<u8>, charging: bool, size: u32) -> Vec<u8> {
    let s = size.max(16) as usize;
    let mut cv = Canvas {
        s,
        px: vec![0; s * s * 4],
    };
    let si = s as i32;

    // --- battery gauge at the bottom ---
    let bar_h = (si / 5).max(4);
    let tip_w = (si / 16).max(1);
    let (gx0, gy0, gx1, gy1) = (0, si - bar_h, si - tip_w, si);
    cv.rect(gx0, gy0, gx1, gy1, FRAME);
    cv.rect(gx1, gy0 + bar_h / 4, si, gy1 - bar_h / 4, FRAME);
    cv.rect(gx0 + 1, gy0 + 1, gx1 - 1, gy1 - 1, EMPTY);
    let level_color = match percent {
        Some(p) => {
            let p = p.min(100) as i32;
            let c = if charging {
                BLUE
            } else if p <= 15 {
                RED
            } else if p <= 35 {
                YELLOW
            } else {
                GREEN
            };
            let inner = gx1 - 1 - (gx0 + 1);
            let w = ((inner * p + 50) / 100).max(1);
            cv.rect(gx0 + 1, gy0 + 1, gx0 + 1 + w, gy1 - 1, c);
            c
        }
        None => GRAY,
    };

    // --- digits ---
    let text = match percent {
        Some(p) => p.min(100).to_string(),
        None => "-".to_string(),
    };
    let n = text.len() as i32;
    let area_h = si - bar_h - 2; // 1px outline above + 1px gap above the gauge
    let sy = (area_h / 5).max(1);
    // horizontal scale may be smaller than vertical (tall digits) so "100" fits at 16px
    let mut sx = 1;
    for k in (1..=sy).rev() {
        let gap = if k <= 2 { 1 } else { 2 };
        if n * 3 * k + (n - 1) * gap <= si - 2 {
            sx = k;
            break;
        }
    }
    let gap = if sx <= 2 { 1 } else { 2 };
    let tw = n * 3 * sx + (n - 1) * gap;
    let th = 5 * sy;
    let ox = (si - tw) / 2;
    let oy = 1 + (area_h - th) / 2;
    let text_color = if percent.is_none() {
        FRAME
    } else if level_color.0 == RED.0 {
        RED_TEXT
    } else {
        WHITE
    };
    for pass in 0..2 {
        for (ci, ch) in text.chars().enumerate() {
            let gx = ox + ci as i32 * (3 * sx + gap);
            for (row, bits) in glyph(ch).iter().enumerate() {
                for col in 0..3 {
                    if bits & (0b100 >> col) == 0 {
                        continue;
                    }
                    let x = gx + col * sx;
                    let y = oy + row as i32 * sy;
                    if pass == 0 {
                        cv.rect(x - 1, y - 1, x + sx + 1, y + sy + 1, SHADOW);
                    } else {
                        cv.rect(x, y, x + sx, y + sy, text_color);
                    }
                }
            }
        }
    }
    cv.px
}

/// RGBA → ARGB32 big-endian, as StatusNotifierItem wants.
#[allow(dead_code)]
pub fn rgba_to_argb(rgba: &[u8]) -> Vec<u8> {
    rgba.chunks(4).flat_map(|p| [p[3], p[0], p[1], p[2]]).collect()
}

/// Build a Windows .ico (PNG-less, 32-bit BMP entries) for the exe resource.
#[allow(dead_code)]
pub fn ico(sizes: &[u32]) -> Vec<u8> {
    let images: Vec<(u32, Vec<u8>)> = sizes
        .iter()
        .map(|&s| {
            let rgba = render(Some(100), false, s);
            let mut bmp = Vec::new();
            bmp.extend(40u32.to_le_bytes());
            bmp.extend((s as i32).to_le_bytes());
            bmp.extend((s as i32 * 2).to_le_bytes());
            bmp.extend(1u16.to_le_bytes());
            bmp.extend(32u16.to_le_bytes());
            bmp.extend([0u8; 24]);
            for y in (0..s as usize).rev() {
                for x in 0..s as usize {
                    let i = (y * s as usize + x) * 4;
                    bmp.extend([rgba[i + 2], rgba[i + 1], rgba[i], rgba[i + 3]]);
                }
            }
            let mask_row = (s as usize).div_ceil(32) * 4;
            bmp.extend(vec![0u8; mask_row * s as usize]);
            (s, bmp)
        })
        .collect();
    let mut out = Vec::new();
    out.extend([0, 0, 1, 0]);
    out.extend((images.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * images.len() as u32;
    for (s, data) in &images {
        out.push(if *s >= 256 { 0 } else { *s as u8 });
        out.push(if *s >= 256 { 0 } else { *s as u8 });
        out.extend([0, 0]);
        out.extend(1u16.to_le_bytes());
        out.extend(32u16.to_le_bytes());
        out.extend((data.len() as u32).to_le_bytes());
        out.extend(offset.to_le_bytes());
        offset += data.len() as u32;
    }
    for (_, data) in images {
        out.extend(data);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_all_sizes() {
        for s in [16, 20, 24, 32, 48, 64] {
            for p in [None, Some(0), Some(7), Some(56), Some(100)] {
                let px = render(p, false, s);
                assert_eq!(px.len(), (s * s * 4) as usize);
                // some white text pixels exist
                // digits are drawn: opaque, non-black pixels in the upper half
                let top = &px[..px.len() / 2];
                assert!(top.chunks(4).any(|c| c[3] == 255 && c[0] > 150), "size {s} {p:?}");
            }
        }
    }

    #[test]
    fn ico_header() {
        let i = ico(&[16, 32]);
        assert_eq!(&i[..6], &[0, 0, 1, 0, 2, 0]);
    }
}
