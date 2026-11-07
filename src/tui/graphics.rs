//! Inline images for the print preview: the PDF page is rasterized by an
//! external tool and shown with the Kitty or iTerm2 graphics protocol.

use std::path::Path;
use std::process::{Command, Stdio};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Protocol {
    /// Kitty graphics protocol (Kitty, Ghostty).
    Kitty,
    /// iTerm2 inline images (iTerm2, WezTerm).
    Iterm,
}

/// Picks the protocol from the environment. `HARDCALC_GRAPHICS=kitty|iterm|none`
/// overrides the detection. Inside tmux images are off unless forced.
pub fn detect(var: impl Fn(&str) -> Option<String>) -> Option<Protocol> {
    if let Some(forced) = var("HARDCALC_GRAPHICS") {
        return match forced.to_lowercase().as_str() {
            "kitty" => Some(Protocol::Kitty),
            "iterm" | "iterm2" => Some(Protocol::Iterm),
            _ => None,
        };
    }
    if var("TMUX").is_some() {
        return None;
    }
    let term = var("TERM").unwrap_or_default();
    let program = var("TERM_PROGRAM").unwrap_or_default();
    if var("KITTY_WINDOW_ID").is_some() || term.contains("kitty") || term.contains("ghostty") || program == "ghostty" {
        Some(Protocol::Kitty)
    } else if program == "iTerm.app" || program == "WezTerm" || var("WEZTERM_EXECUTABLE").is_some() {
        Some(Protocol::Iterm)
    } else {
        None
    }
}

pub fn base64(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(TABLE[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Escape sequence that shows a PNG over `cols` × `rows` cells at the cursor.
pub fn escape(protocol: Protocol, png: &[u8], cols: u16, rows: u16) -> String {
    let data = base64(png);
    match protocol {
        Protocol::Kitty => {
            // Chunks of at most 4096 bytes; q=2 keeps the terminal from answering
            // (answers would arrive as key input), C=1 leaves the cursor in place.
            let chunks: Vec<&str> = data.as_bytes().chunks(4096).map(|c| std::str::from_utf8(c).unwrap()).collect();
            let mut out = String::new();
            for (i, chunk) in chunks.iter().enumerate() {
                let more = u8::from(i + 1 < chunks.len());
                if i == 0 {
                    out.push_str(&format!("\x1b_Gf=100,a=T,t=d,q=2,C=1,c={cols},r={rows},m={more};{chunk}\x1b\\"));
                } else {
                    out.push_str(&format!("\x1b_Gm={more};{chunk}\x1b\\"));
                }
            }
            out
        }
        Protocol::Iterm => format!(
            "\x1b]1337;File=inline=1;size={};width={cols};height={rows};preserveAspectRatio=1:{data}\x07",
            png.len()
        ),
    }
}

/// Removes all images (Kitty keeps them until told; iTerm2 images vanish
/// when their cells are redrawn).
pub fn clear(protocol: Protocol) -> &'static str {
    match protocol {
        Protocol::Kitty => "\x1b_Ga=d,d=A,q=2\x1b\\",
        Protocol::Iterm => "",
    }
}

fn run(cmd: &mut Command) -> bool {
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
}

/// Renders the first page of `pdf` to a PNG `width` pixels wide, using
/// pdftoppm, mutool or (on macOS) sips – whichever is installed.
pub fn rasterize(pdf: &[u8], width: u32) -> Result<Vec<u8>, String> {
    let dir = std::env::temp_dir().join(format!("hardcalc-preview-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let input = dir.join("page.pdf");
    let output = dir.join("page.png");
    std::fs::write(&input, pdf).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&output);
    let w = width.to_string();
    let command = |program: &str, args: Vec<std::ffi::OsString>| {
        let mut cmd = Command::new(program);
        cmd.args(args);
        cmd
    };
    let os = |s: &str| std::ffi::OsString::from(s);
    let tried = [
        command("pdftoppm", vec![os("-png"), os("-singlefile"), os("-scale-to-x"), os(&w), os("-scale-to-y"), os("-1"), input.clone().into(), dir.join("page").into()]),
        command("mutool", vec![os("draw"), os("-q"), os("-w"), os(&w), os("-o"), output.clone().into(), input.clone().into(), os("1")]),
        command("sips", vec![os("-s"), os("format"), os("png"), os("--resampleWidth"), os(&w), input.clone().into(), os("--out"), output.clone().into()]),
    ];
    for mut cmd in tried {
        if run(&mut cmd) && Path::new(&output).exists() {
            return std::fs::read(&output).map_err(|e| e.to_string());
        }
    }
    Err("no PDF rasterizer found – install poppler (pdftoppm) or MuPDF (mutool)".into())
}

/// An RGB image (row-major, top row first).
#[derive(Debug, Clone, PartialEq)]
pub struct RgbImage {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<[u8; 3]>,
}

impl RgbImage {
    pub fn get(&self, x: usize, y: usize) -> [u8; 3] {
        self.pixels[y * self.width + x]
    }

    /// Scales to `w` × `h` by averaging each target pixel's source area, so
    /// thin lines (text) show up as gray instead of disappearing.
    pub fn resize(&self, w: usize, h: usize) -> RgbImage {
        let (w, h) = (w.max(1), h.max(1));
        let mut pixels = Vec::with_capacity(w * h);
        for ty in 0..h {
            let (y0, y1) = (ty * self.height / h, ((ty + 1) * self.height / h).max(ty * self.height / h + 1));
            for tx in 0..w {
                let (x0, x1) = (tx * self.width / w, ((tx + 1) * self.width / w).max(tx * self.width / w + 1));
                let mut sum = [0u32; 3];
                let mut n = 0;
                for y in y0..y1.min(self.height) {
                    for x in x0..x1.min(self.width) {
                        let p = self.get(x, y);
                        for k in 0..3 {
                            sum[k] += p[k] as u32;
                        }
                        n += 1;
                    }
                }
                let n = n.max(1);
                pixels.push([(sum[0] / n) as u8, (sum[1] / n) as u8, (sum[2] / n) as u8]);
            }
        }
        RgbImage { width: w, height: h, pixels }
    }
}

/// Binary PPM (`P6`, 8 bit), as written by pdftoppm and mutool.
pub fn parse_ppm(data: &[u8]) -> Result<RgbImage, String> {
    let mut fields = Vec::new();
    let mut i = 0;
    while fields.len() < 4 {
        while i < data.len() && data[i].is_ascii_whitespace() {
            i += 1;
        }
        if data.get(i) == Some(&b'#') {
            while i < data.len() && data[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        let start = i;
        while i < data.len() && !data[i].is_ascii_whitespace() {
            i += 1;
        }
        if start == i {
            return Err("truncated PPM header".into());
        }
        fields.push(String::from_utf8_lossy(&data[start..i]).to_string());
    }
    if fields[0] != "P6" || fields[3] != "255" {
        return Err("only 8-bit binary PPM (P6) is supported".into());
    }
    let parse = |s: &str| s.parse::<usize>().map_err(|_| "bad PPM size".to_string());
    let (width, height) = (parse(&fields[1])?, parse(&fields[2])?);
    let body = &data[i + 1..];
    if body.len() < width * height * 3 {
        return Err("truncated PPM data".into());
    }
    let pixels = body.chunks(3).take(width * height).map(|p| [p[0], p[1], p[2]]).collect();
    Ok(RgbImage { width, height, pixels })
}

/// Uncompressed BMP with 24 or 32 bits per pixel (with or without colour
/// masks), as written by macOS `sips`.
pub fn parse_bmp(data: &[u8]) -> Result<RgbImage, String> {
    let u32_at = |o: usize| data.get(o..o + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])).ok_or("truncated BMP");
    let u16_at = |o: usize| data.get(o..o + 2).map(|b| u16::from_le_bytes([b[0], b[1]])).ok_or("truncated BMP");
    if !data.starts_with(b"BM") {
        return Err("not a BMP file".into());
    }
    let offset = u32_at(10)? as usize;
    let width = u32_at(18)? as i32;
    let height = u32_at(22)? as i32;
    let bpp = u16_at(28)?;
    let compression = u32_at(30)?;
    if width <= 0 || height == 0 || !(bpp == 24 || bpp == 32) || !(compression == 0 || compression == 3) {
        return Err(format!("unsupported BMP ({bpp} bit, compression {compression})"));
    }
    // Channel masks (BI_BITFIELDS); default byte order is B, G, R.
    let masks = if compression == 3 { [u32_at(54)?, u32_at(58)?, u32_at(62)?] } else { [0xff0000, 0x00ff00, 0x0000ff] };
    // Alpha: from the V4/V5 header's mask, or the fourth byte of 32-bit pixels.
    let header = u32_at(14)?;
    let alpha_mask = match (bpp, compression, header >= 56) {
        (32, 3, true) => u32_at(66)?,
        (32, 0, _) => 0xff00_0000,
        _ => 0,
    };
    let channel = |px: u32, mask: u32| if mask == 0 { 0 } else { ((px & mask) >> mask.trailing_zeros()) as u8 };
    let (w, h) = (width as usize, height.unsigned_abs() as usize);
    let bytes = bpp as usize / 8;
    let stride = (w * bytes).div_ceil(4) * 4;
    let mut pixels = Vec::with_capacity(w * h);
    for y in 0..h {
        // Positive height: bottom row first.
        let row = if height > 0 { h - 1 - y } else { y };
        for x in 0..w {
            let o = offset + row * stride + x * bytes;
            let b = data.get(o..o + bytes).ok_or("truncated BMP data")?;
            let px = if bytes == 4 { u32::from_le_bytes([b[0], b[1], b[2], b[3]]) } else { u32::from_le_bytes([b[0], b[1], b[2], 0]) };
            // Composite transparent pixels onto white paper.
            let a = if alpha_mask == 0 { 255 } else { channel(px, alpha_mask) as u32 };
            let over = |c: u8| ((c as u32 * a + 255 * (255 - a)) / 255) as u8;
            pixels.push([over(channel(px, masks[0])), over(channel(px, masks[1])), over(channel(px, masks[2]))]);
        }
    }
    Ok(RgbImage { width: w, height: h, pixels })
}

/// Renders the first page of `pdf` as an RGB image `width` pixels wide
/// (PPM from pdftoppm / mutool, BMP from sips).
pub fn rasterize_rgb(pdf: &[u8], width: u32) -> Result<RgbImage, String> {
    let dir = std::env::temp_dir().join(format!("hardcalc-preview-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let input = dir.join("page.pdf");
    std::fs::write(&input, pdf).map_err(|e| e.to_string())?;
    let (ppm, bmp) = (dir.join("page.ppm"), dir.join("page.bmp"));
    let _ = std::fs::remove_file(&ppm);
    let _ = std::fs::remove_file(&bmp);
    let w = width.to_string();
    let os = |s: &str| std::ffi::OsString::from(s);
    type Parser = fn(&[u8]) -> Result<RgbImage, String>;
    let attempts: [(&str, Vec<std::ffi::OsString>, &Path, Parser); 3] = [
        ("pdftoppm", vec![os("-singlefile"), os("-scale-to-x"), os(&w), os("-scale-to-y"), os("-1"), input.clone().into(), dir.join("page").into()], &ppm, parse_ppm),
        ("mutool", vec![os("draw"), os("-q"), os("-w"), os(&w), os("-o"), ppm.clone().into(), input.clone().into(), os("1")], &ppm, parse_ppm),
        ("sips", vec![os("-s"), os("format"), os("bmp"), os("--resampleWidth"), os(&w), input.clone().into(), os("--out"), bmp.clone().into()], &bmp, parse_bmp),
    ];
    for (program, args, out, parse) in attempts {
        let mut cmd = Command::new(program);
        cmd.args(args);
        if run(&mut cmd) && out.exists() {
            return parse(&std::fs::read(out).map_err(|e| e.to_string())?);
        }
    }
    Err("no PDF rasterizer found – install poppler (pdftoppm) or MuPDF (mutool)".into())
}

/// Nearest colour of the xterm 256-colour palette (for terminals without 24-bit colour).
pub fn ansi256([r, g, b]: [u8; 3]) -> u8 {
    const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
    let level = |v: u8| LEVELS.iter().enumerate().min_by_key(|(_, l)| (**l as i32 - v as i32).abs()).unwrap().0;
    let (ri, gi, bi) = (level(r), level(g), level(b));
    let cube = [LEVELS[ri], LEVELS[gi], LEVELS[bi]];
    let gray_level = ((r as u32 + g as u32 + b as u32) / 3) as u8;
    let gi_ = ((gray_level as i32 - 8).max(0) / 10).min(23) as u8;
    let gray = 8 + 10 * gi_;
    let dist = |c: [u8; 3]| (0..3).map(|k| (c[k] as i32 - [r, g, b][k] as i32).pow(2)).sum::<i32>();
    if dist([gray; 3]) < dist(cube) { 232 + gi_ } else { 16 + 36 * ri as u8 + 6 * gi as u8 + bi as u8 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_rfc4648() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn detection() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |k: &str| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string())
        };
        assert_eq!(detect(env(&[("TERM", "xterm-kitty")])), Some(Protocol::Kitty));
        assert_eq!(detect(env(&[("TERM_PROGRAM", "ghostty")])), Some(Protocol::Kitty));
        assert_eq!(detect(env(&[("TERM_PROGRAM", "iTerm.app")])), Some(Protocol::Iterm));
        assert_eq!(detect(env(&[("TERM_PROGRAM", "Apple_Terminal")])), None);
        assert_eq!(detect(env(&[("TERM_PROGRAM", "iTerm.app"), ("TMUX", "x")])), None);
        assert_eq!(detect(env(&[("TERM_PROGRAM", "Apple_Terminal"), ("HARDCALC_GRAPHICS", "kitty")])), Some(Protocol::Kitty));
        assert_eq!(detect(env(&[("TERM", "xterm-kitty"), ("HARDCALC_GRAPHICS", "none")])), None);
    }

    #[test]
    fn rasterizes_when_a_tool_is_installed() {
        let layout = crate::print::paginate("Test", &[], &crate::print::PageSetup::default(), "x");
        let pdf = crate::print::to_pdf(&layout, "t");
        match rasterize(&pdf, 400) {
            Ok(png) => assert!(png.starts_with(b"\x89PNG")),
            Err(e) => assert!(e.contains("no PDF rasterizer"), "{e}"),
        }
    }

    #[test]
    fn image_formats() {
        let ppm = b"P6\n# c\n2 1\n255\n\xff\x00\x00\x00\x00\xff".to_vec();
        let img = parse_ppm(&ppm).unwrap();
        assert_eq!((img.width, img.height, img.pixels.clone()), (2, 1, vec![[255, 0, 0], [0, 0, 255]]));

        // 2×2, 24 bit, bottom-up: rows are padded to 4 bytes.
        let mut bmp = vec![0u8; 54];
        bmp[0..2].copy_from_slice(b"BM");
        bmp[10..14].copy_from_slice(&54u32.to_le_bytes());
        bmp[18..22].copy_from_slice(&2u32.to_le_bytes());
        bmp[22..26].copy_from_slice(&2u32.to_le_bytes());
        bmp[28..30].copy_from_slice(&24u16.to_le_bytes());
        bmp.extend([0, 0, 255, 0, 255, 0, 0, 0]); // bottom row: red, green (B, G, R order)
        bmp.extend([255, 0, 0, 255, 255, 255, 0, 0]); // top row: blue, white
        let img = parse_bmp(&bmp).unwrap();
        assert_eq!(img.pixels, vec![[0, 0, 255], [255, 255, 255], [255, 0, 0], [0, 255, 0]]);

        let half = img.resize(1, 1);
        assert_eq!(half.pixels, vec![[127, 127, 127]]);
        assert_eq!(ansi256([255, 255, 255]), 231);
        assert_eq!(ansi256([0, 0, 0]), 16);
        assert_eq!(ansi256([128, 128, 128]), 244);
    }

    #[test]
    fn rasterizes_rgb_when_a_tool_is_installed() {
        let layout = crate::print::paginate("Test", &[], &crate::print::PageSetup::default(), "x");
        let pdf = crate::print::to_pdf(&layout, "t");
        match rasterize_rgb(&pdf, 120) {
            Ok(img) => {
                assert_eq!(img.width, 120);
                assert!(img.pixels.iter().filter(|p| **p == [255, 255, 255]).count() > img.pixels.len() / 2, "mostly white paper");
            }
            Err(e) => assert!(e.contains("no PDF rasterizer"), "{e}"),
        }
    }

    #[test]
    fn escapes() {
        let png = vec![7u8; 5000];
        let kitty = escape(Protocol::Kitty, &png, 40, 20);
        assert!(kitty.starts_with("\x1b_Gf=100,a=T,t=d,q=2,C=1,c=40,r=20,m=1;"));
        assert_eq!(kitty.matches("\x1b_G").count(), 2, "6668 base64 bytes → two chunks");
        assert!(kitty.contains("\x1b_Gm=0;"));
        let iterm = escape(Protocol::Iterm, &png, 40, 20);
        assert!(iterm.starts_with("\x1b]1337;File=inline=1;size=5000;width=40;height=20;preserveAspectRatio=1:"));
        assert!(iterm.ends_with('\x07'));
    }
}
