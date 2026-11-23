//! Inline images (charts, typeset formulas, the print preview): shown with
//! the Kitty or iTerm2 graphics protocol, or as Sixel (Windows Terminal,
//! foot, mlterm); the PDF page of the preview is rasterized by an external tool.

use std::path::Path;
use std::process::{Command, Stdio};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Protocol {
    /// Kitty graphics protocol (Kitty, Ghostty).
    Kitty,
    /// iTerm2 inline images (iTerm2, WezTerm).
    Iterm,
    /// Sixel graphics (Windows Terminal 1.22+, foot, mlterm, xterm -ti vt340).
    Sixel,
}

/// Picks the protocol from the environment. `HARDCALC_GRAPHICS=kitty|iterm|sixel|none`
/// overrides the detection. Inside tmux images are off unless forced.
pub fn detect(var: impl Fn(&str) -> Option<String>) -> Option<Protocol> {
    if let Some(forced) = var("HARDCALC_GRAPHICS") {
        return match forced.to_lowercase().as_str() {
            "kitty" => Some(Protocol::Kitty),
            "iterm" | "iterm2" => Some(Protocol::Iterm),
            "sixel" => Some(Protocol::Sixel),
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
    } else if var("WT_SESSION").is_some() || term == "foot" || term.starts_with("foot-") || term.starts_with("mlterm") {
        // Windows Terminal (also under WSL) understands Sixel since 1.22.
        Some(Protocol::Sixel)
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
        // Sixel needs the pixels: see [`sixel`].
        Protocol::Sixel => String::new(),
    }
}

/// Removes all images (Kitty keeps them until told; iTerm2 images vanish
/// when their cells are redrawn).
pub fn clear(protocol: Protocol) -> &'static str {
    match protocol {
        Protocol::Kitty => "\x1b_Ga=d,d=A,q=2\x1b\\",
        Protocol::Iterm | Protocol::Sixel => "",
    }
}

/// Decodes a PNG (8 bit, not interlaced – what pdftoppm, mutool, sips and
/// hardcalc write) to RGBA pixels.
pub fn decode_png(data: &[u8]) -> Result<(usize, usize, Vec<[u8; 4]>), String> {
    use std::io::Read;
    let bad = |what: &str| format!("PNG: {what}");
    if !data.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err(bad("not a PNG"));
    }
    let (mut pos, mut header, mut palette, mut idat) = (8, None, Vec::new(), Vec::new());
    while pos + 8 <= data.len() {
        let len = u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
        let kind = &data[pos + 4..pos + 8];
        let body = data.get(pos + 8..pos + 8 + len).ok_or_else(|| bad("cut off"))?;
        match kind {
            b"IHDR" if len >= 13 => header = Some((u32::from_be_bytes([body[0], body[1], body[2], body[3]]) as usize, u32::from_be_bytes([body[4], body[5], body[6], body[7]]) as usize, body[8], body[9], body[12])),
            b"PLTE" => palette = body.to_vec(),
            b"IDAT" => idat.extend_from_slice(body),
            b"IEND" => break,
            _ => {}
        }
        pos += 12 + len;
    }
    let (w, h, depth, ctype, interlace) = header.ok_or_else(|| bad("no header"))?;
    if depth != 8 || interlace != 0 {
        return Err(bad("only 8 bit, not interlaced"));
    }
    let bpp = match ctype {
        0 | 3 => 1,
        2 => 3,
        4 => 2,
        6 => 4,
        _ => return Err(bad("colour type")),
    };
    let mut raw = Vec::new();
    flate2::read::ZlibDecoder::new(&idat[..]).read_to_end(&mut raw).map_err(|e| bad(&e.to_string()))?;
    let stride = w * bpp;
    if raw.len() < h * (stride + 1) {
        return Err(bad("too little data"));
    }
    let mut rows: Vec<u8> = vec![0; h * stride];
    for y in 0..h {
        let filter = raw[y * (stride + 1)];
        let line = &raw[y * (stride + 1) + 1..(y + 1) * (stride + 1)];
        for x in 0..stride {
            let a = if x >= bpp { rows[y * stride + x - bpp] as i32 } else { 0 };
            let b = if y > 0 { rows[(y - 1) * stride + x] as i32 } else { 0 };
            let c = if y > 0 && x >= bpp { rows[(y - 1) * stride + x - bpp] as i32 } else { 0 };
            let add = match filter {
                0 => 0,
                1 => a,
                2 => b,
                3 => (a + b) / 2,
                4 => {
                    let p = a + b - c;
                    let (pa, pb, pc) = ((p - a).abs(), (p - b).abs(), (p - c).abs());
                    if pa <= pb && pa <= pc { a } else if pb <= pc { b } else { c }
                }
                _ => return Err(bad("filter")),
            };
            rows[y * stride + x] = (line[x] as i32 + add) as u8;
        }
    }
    let pixels = rows
        .chunks(bpp)
        .map(|p| match ctype {
            0 => [p[0], p[0], p[0], 255],
            2 => [p[0], p[1], p[2], 255],
            3 => {
                let i = p[0] as usize * 3;
                match palette.get(i..i + 3) {
                    Some(c) => [c[0], c[1], c[2], 255],
                    None => [0, 0, 0, 255],
                }
            }
            4 => [p[0], p[0], p[0], p[1]],
            _ => [p[0], p[1], p[2], p[3]],
        })
        .collect();
    Ok((w, h, pixels))
}

/// A PNG as Sixel for a box of `cols` × `rows` cells of `cell` pixels: laid
/// over the background colour `bg` (Sixel has no transparency we can rely
/// on) and fitted into the box.
pub fn sixel(png: &[u8], cols: u16, rows: u16, cell: (u16, u16), bg: [u8; 3]) -> Result<String, String> {
    let (w, h, pixels) = decode_png(png)?;
    let over = crate::typeset::Image { width: w, height: h, pixels }.over(bg);
    let mut img = RgbImage { width: w, height: h, pixels: over };
    let (bw, bh) = (cols as usize * cell.0 as usize, rows as usize * cell.1 as usize);
    if bw > 0 && bh > 0 && (w > bw || h > bh || (w < bw && h < bh)) {
        let f = (bw as f64 / w as f64).min(bh as f64 / h as f64);
        img = img.resize(((w as f64 * f) as usize).max(1), ((h as f64 * f) as usize).max(1));
    }
    Ok(sixel_encode(&img))
}

/// The image as a Sixel escape sequence: a palette of the 255 most common
/// colours (at 4 bits per channel), bands of six pixel rows.
pub fn sixel_encode(img: &RgbImage) -> String {
    use std::fmt::Write;
    let bin = |p: [u8; 3]| ((p[0] as usize >> 4) << 8) | ((p[1] as usize >> 4) << 4) | (p[2] as usize >> 4);
    let mut count = vec![0u32; 4096];
    let mut sum = vec![[0u32; 3]; 4096];
    for p in &img.pixels {
        let b = bin(*p);
        count[b] += 1;
        for k in 0..3 {
            sum[b][k] += p[k] as u32;
        }
    }
    let mut used: Vec<usize> = (0..4096).filter(|&b| count[b] > 0).collect();
    used.sort_by_key(|&b| std::cmp::Reverse(count[b]));
    let mean = |b: usize| [sum[b][0] / count[b], sum[b][1] / count[b], sum[b][2] / count[b]];
    let palette: Vec<[u32; 3]> = used.iter().take(255).map(|&b| mean(b)).collect();
    // The palette entry nearest to each colour bin.
    let mut map = vec![0u8; 4096];
    for (i, &b) in used.iter().enumerate() {
        map[b] = if i < palette.len() {
            i as u8
        } else {
            let m = mean(b);
            let dist = |c: &[u32; 3]| (0..3).map(|k| (c[k] as i64 - m[k] as i64).pow(2)).sum::<i64>();
            palette.iter().enumerate().min_by_key(|(_, c)| dist(c)).map_or(0, |(j, _)| j as u8)
        };
    }
    let idx: Vec<u8> = img.pixels.iter().map(|p| map[bin(*p)]).collect();
    let mut out = format!("\x1bPq\"1;1;{};{}", img.width, img.height);
    for (i, c) in palette.iter().enumerate() {
        let _ = write!(out, "#{i};2;{};{};{}", c[0] * 100 / 255, c[1] * 100 / 255, c[2] * 100 / 255);
    }
    let w = img.width;
    for top in (0..img.height).step_by(6) {
        let mut masks: Vec<Option<Vec<u8>>> = vec![None; palette.len()];
        for k in 0..6.min(img.height - top) {
            for x in 0..w {
                let c = idx[(top + k) * w + x] as usize;
                masks[c].get_or_insert_with(|| vec![0; w])[x] |= 1 << k;
            }
        }
        let mut first = true;
        for (c, mask) in masks.iter().enumerate() {
            let Some(mask) = mask else { continue };
            // Back to the left edge before the next colour paints the same band.
            if !first {
                out.push('$');
            }
            first = false;
            let _ = write!(out, "#{c}");
            let end = mask.iter().rposition(|m| *m != 0).map_or(0, |i| i + 1);
            let mut x = 0;
            while x < end {
                let m = mask[x];
                let run = mask[x..end].iter().take_while(|v| **v == m).count();
                let ch = (m + 63) as char;
                if run > 3 {
                    let _ = write!(out, "!{run}{ch}");
                } else {
                    (0..run).for_each(|_| out.push(ch));
                }
                x += run;
            }
        }
        out.push('-');
    }
    out.push_str("\x1b\\");
    out
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

    #[test]
    fn sixel_detection_and_encoding() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |k: &str| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string())
        };
        assert_eq!(detect(env(&[("WT_SESSION", "x")])), Some(Protocol::Sixel));
        assert_eq!(detect(env(&[("HARDCALC_GRAPHICS", "sixel")])), Some(Protocol::Sixel));
        assert_eq!(detect(env(&[("WT_SESSION", "x"), ("HARDCALC_GRAPHICS", "none")])), None);
        // 2 × 8 pixels: left column red, right column blue; two bands.
        let img = RgbImage { width: 2, height: 8, pixels: (0..16).map(|i| if i % 2 == 0 { [255, 0, 0] } else { [0, 0, 255] }).collect() };
        let s = sixel_encode(&img);
        assert!(s.starts_with("\x1bPq\"1;1;2;8#"));
        assert!(s.contains(";2;100;0;0") && s.contains(";2;0;0;100"));
        // Band 1: blue in the right pixel of all six rows (~), red in the left one;
        // band 2 has two rows (B). The empty tail of a colour is left out.
        assert!(s.contains("#0?~$#1~-"), "{s}");
        assert!(s.contains("#0?B$#1B-"), "{s}");
        assert_eq!(s.matches('-').count(), 2);
        assert!(s.ends_with("\x1b\\"));
    }

    #[test]
    fn png_round_trip_through_the_decoder() {
        let mut c = crate::tui::chart_image::Canvas::new(3, 2, [0, 0, 0, 0]);
        c.pixels[1] = [10, 20, 30, 255];
        c.pixels[5] = [200, 100, 50, 128];
        let (w, h, px) = decode_png(&c.png()).unwrap();
        assert_eq!((w, h), (3, 2));
        assert_eq!(px, c.pixels);
        assert!(sixel(&c.png(), 1, 1, (3, 2), [0, 0, 0]).unwrap().starts_with("\x1bPq"));
    }
}
