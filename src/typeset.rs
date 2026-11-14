//! Formulas typeset with Typst, as pixel images: for the calculation sheet
//! on screen (terminals with a graphics protocol) and for printing.
//!
//! Uses the built-in Typst compiler (cargo feature `typst`), else the
//! `typst` program if it is installed (`HARDCALC_TYPST` names another
//! one); with neither [`available`] is false and the formulas stay text.

/// A picture in straight (not premultiplied) 8-bit RGBA, row-major.
#[derive(Debug, Clone, PartialEq)]
pub struct Image {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<[u8; 4]>,
}

impl Image {
    /// The picture scaled by `f` (≤ 1), each new pixel the mean of those it covers.
    pub fn shrink(&self, f: f64) -> Image {
        if f >= 1.0 {
            return self.clone();
        }
        let w = ((self.width as f64 * f).round() as usize).max(1);
        let h = ((self.height as f64 * f).round() as usize).max(1);
        let mut pixels = Vec::with_capacity(w * h);
        for y in 0..h {
            let (y0, y1) = (y * self.height / h, ((y + 1) * self.height / h).max(y * self.height / h + 1));
            for x in 0..w {
                let (x0, x1) = (x * self.width / w, ((x + 1) * self.width / w).max(x * self.width / w + 1));
                // Colours weighted by their cover, so edges do not darken.
                let (mut sum, mut alpha, mut n) = ([0f64; 3], 0f64, 0f64);
                for yy in y0..y1.min(self.height) {
                    for xx in x0..x1.min(self.width) {
                        let p = self.pixels[yy * self.width + xx];
                        let a = p[3] as f64;
                        for k in 0..3 {
                            sum[k] += p[k] as f64 * a;
                        }
                        alpha += a;
                        n += 1.0;
                    }
                }
                let c = |k: usize| if alpha > 0.0 { (sum[k] / alpha).round() as u8 } else { 0 };
                pixels.push([c(0), c(1), c(2), (alpha / n.max(1.0)).round() as u8]);
            }
        }
        Image { width: w, height: h, pixels }
    }

    /// The picture on a transparent area of `width` × `height` pixels: at
    /// the left, centred from top to bottom (cut where it does not fit).
    pub fn placed(&self, width: usize, height: usize) -> Image {
        let mut pixels = vec![[0u8; 4]; width * height];
        let top = height.saturating_sub(self.height) / 2;
        for y in 0..self.height.min(height) {
            for x in 0..self.width.min(width) {
                pixels[(top + y) * width + x] = self.pixels[y * self.width + x];
            }
        }
        Image { width, height, pixels }
    }

    /// The picture over a plain background colour (no transparency left).
    pub fn over(&self, bg: [u8; 3]) -> Vec<[u8; 3]> {
        self.pixels
            .iter()
            .map(|p| {
                let a = p[3] as u32;
                let mix = |k: usize| ((p[k] as u32 * a + bg[k] as u32 * (255 - a)) / 255) as u8;
                [mix(0), mix(1), mix(2)]
            })
            .collect()
    }
}

/// How formulas are set.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Style {
    /// Font size in points.
    pub size: f32,
    /// Pixels per point.
    pub scale: f32,
    pub color: [u8; 3],
    /// Display style (large fractions, as in the report), else inline style.
    pub display: bool,
}

/// Whether formulas can be typeset: Typst is built in or installed.
pub fn available() -> bool {
    cfg!(feature = "typst") || program().is_some()
}

/// The installed `typst` program (`HARDCALC_TYPST` overrides the name),
/// if it runs; looked up once.
pub fn program() -> Option<&'static std::path::Path> {
    static PROGRAM: std::sync::OnceLock<Option<std::path::PathBuf>> = std::sync::OnceLock::new();
    PROGRAM
        .get_or_init(|| {
            let name = std::env::var_os("HARDCALC_TYPST").filter(|n| !n.is_empty()).unwrap_or_else(|| "typst".into());
            let ok = std::process::Command::new(&name)
                .arg("--version")
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .is_ok_and(|s| s.success());
            ok.then(|| name.into())
        })
        .as_deref()
}

/// Typst source with each formula on a page of its own, just its size.
fn source(math: &[String], style: Style) -> String {
    let [r, g, b] = style.color;
    let mut src = format!(
        "#set page(width: auto, height: auto, margin: (x: 1pt, y: 0.5pt), fill: none)\n#set text(size: {}pt, fill: rgb({r}, {g}, {b}))\n",
        style.size
    );
    for (i, m) in math.iter().enumerate() {
        if i > 0 {
            src.push_str("#pagebreak()\n");
        }
        if style.display {
            src.push_str(&format!("$ {m} $\n"));
        } else {
            src.push_str(&format!("${m}$\n"));
        }
    }
    src
}

/// Each formula (Typst math, without `$`) typeset; `None` for one that
/// Typst could not set, and for all without Typst.
pub fn formulas(math: &[String], style: Style) -> Vec<Option<Image>> {
    if math.is_empty() {
        return Vec::new();
    }
    #[cfg(feature = "typst")]
    let render = imp::render;
    #[cfg(not(feature = "typst"))]
    let render = |math: &[String], style: Style| match program() {
        Some(p) => external(p, math, style),
        None => Err("no Typst".to_string()),
    };
    // All at once, one page each; if one fails, each on its own.
    match render(math, style) {
        Ok(images) => images.into_iter().map(Some).collect(),
        Err(_) if math.len() == 1 || !available() => vec![None; math.len()],
        Err(_) => math.iter().map(|m| render(std::slice::from_ref(m), style).ok().and_then(|mut v| v.pop())).collect(),
    }
}

/// The formulas set by the `typst` program: one PNG per page.
#[cfg_attr(feature = "typst", allow(dead_code))]
fn external(program: &std::path::Path, math: &[String], style: Style) -> Result<Vec<Image>, String> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static RUN: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!("hardcalc-typeset-{}-{}", std::process::id(), RUN.fetch_add(1, Ordering::Relaxed)));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let result = (|| {
        std::fs::write(dir.join("main.typ"), source(math, style)).map_err(|e| e.to_string())?;
        // Without looking through the system's fonts (a tenth of a second)
        // Typst sets with its own, as when built in; versions that do not
        // know that option run without it.
        let run = |fast: bool| {
            let mut cmd = std::process::Command::new(program);
            cmd.arg("compile");
            if fast {
                cmd.arg("--ignore-system-fonts");
            }
            cmd.arg(dir.join("main.typ"))
                .arg(dir.join("page-{p}.png"))
                .arg("--ppi")
                .arg(format!("{:.2}", style.scale * 72.0))
                .stdin(std::process::Stdio::null())
                .output()
                .map_err(|e| e.to_string())
        };
        let mut out = run(true)?;
        if !out.status.success() && String::from_utf8_lossy(&out.stderr).contains("ignore-system-fonts") {
            out = run(false)?;
        }
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
        }
        (1..=math.len())
            .map(|p| {
                let data = std::fs::read(dir.join(format!("page-{p}.png"))).map_err(|e| e.to_string())?;
                decode_png(&data)
            })
            .collect()
    })();
    let _ = std::fs::remove_dir_all(&dir);
    result
}

/// Reads a PNG: 8 bits per channel, not interlaced (as Typst writes them).
pub fn decode_png(data: &[u8]) -> Result<Image, String> {
    use std::io::Read;
    let bad = || "not a PNG Typst writes".to_string();
    if data.get(..8) != Some(b"\x89PNG\r\n\x1a\n".as_slice()) {
        return Err(bad());
    }
    let (mut width, mut height, mut channels) = (0usize, 0usize, 0usize);
    let mut palette: Vec<[u8; 3]> = Vec::new();
    let mut alpha: Vec<u8> = Vec::new();
    let mut idat = Vec::new();
    let mut at = 8;
    while at + 8 <= data.len() {
        let len = u32::from_be_bytes(data[at..at + 4].try_into().map_err(|_| bad())?) as usize;
        let kind = &data[at + 4..at + 8];
        let body = data.get(at + 8..at + 8 + len).ok_or_else(bad)?;
        match kind {
            b"IHDR" => {
                width = u32::from_be_bytes(body[0..4].try_into().map_err(|_| bad())?) as usize;
                height = u32::from_be_bytes(body[4..8].try_into().map_err(|_| bad())?) as usize;
                let (depth, color, interlace) = (body[8], body[9], body[12]);
                if depth != 8 || interlace != 0 {
                    return Err(bad());
                }
                // Channels per pixel; 0: a palette index.
                channels = match color {
                    0 => 1,
                    2 => 3,
                    3 => 0,
                    4 => 2,
                    6 => 4,
                    _ => return Err(bad()),
                };
            }
            b"PLTE" => palette = body.as_chunks::<3>().0.to_vec(),
            b"tRNS" => alpha = body.to_vec(),
            b"IDAT" => idat.extend_from_slice(body),
            b"IEND" => break,
            _ => {}
        }
        at += 12 + len;
    }
    let indexed = channels == 0;
    let bpp = if indexed { 1 } else { channels };
    if width == 0 || height == 0 {
        return Err(bad());
    }
    let mut raw = Vec::new();
    flate2::read::ZlibDecoder::new(idat.as_slice()).read_to_end(&mut raw).map_err(|e| e.to_string())?;
    let stride = width * bpp;
    if raw.len() < height * (stride + 1) {
        return Err(bad());
    }
    // Undo the filter of each row.
    let mut rows: Vec<u8> = vec![0; height * stride];
    for y in 0..height {
        let filter = raw[y * (stride + 1)];
        let src = &raw[y * (stride + 1) + 1..(y + 1) * (stride + 1)];
        for x in 0..stride {
            let a = if x >= bpp { rows[y * stride + x - bpp] as i32 } else { 0 };
            let b = if y > 0 { rows[(y - 1) * stride + x] as i32 } else { 0 };
            let c = if x >= bpp && y > 0 { rows[(y - 1) * stride + x - bpp] as i32 } else { 0 };
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
                _ => return Err(bad()),
            };
            rows[y * stride + x] = (src[x] as i32 + add) as u8;
        }
    }
    let pixels = rows
        .chunks_exact(bpp)
        .map(|p| match (indexed, bpp) {
            (true, _) => {
                let i = p[0] as usize;
                let [r, g, b] = palette.get(i).copied().unwrap_or([0; 3]);
                [r, g, b, alpha.get(i).copied().unwrap_or(255)]
            }
            (_, 1) => [p[0], p[0], p[0], 255],
            (_, 2) => [p[0], p[0], p[0], p[1]],
            (_, 3) => [p[0], p[1], p[2], 255],
            _ => [p[0], p[1], p[2], p[3]],
        })
        .collect();
    Ok(Image { width, height, pixels })
}

#[cfg(feature = "typst")]
pub use imp::compile;

#[cfg(feature = "typst")]
mod imp {
    use super::{Image, Style};
    use std::sync::OnceLock;
    use typst::diag::{FileError, FileResult};
    use typst::foundations::{Bytes, Datetime};
    use typst::layout::PagedDocument;
    use typst::syntax::{FileId, Source, VirtualPath};
    use typst::text::{Font, FontBook};
    use typst::utils::LazyHash;
    use typst::{Library, LibraryExt, World};

    /// Fonts and library, loaded once.
    struct Shared {
        library: LazyHash<Library>,
        book: LazyHash<FontBook>,
        fonts: Vec<Font>,
    }

    fn shared() -> &'static Shared {
        static SHARED: OnceLock<Shared> = OnceLock::new();
        SHARED.get_or_init(|| {
            let fonts: Vec<Font> = typst_assets::fonts().flat_map(|data| Font::iter(Bytes::new(data))).collect();
            Shared { library: LazyHash::new(Library::default()), book: LazyHash::new(FontBook::from_fonts(&fonts)), fonts }
        })
    }

    struct OneFile {
        shared: &'static Shared,
        main: Source,
    }

    impl World for OneFile {
        fn library(&self) -> &LazyHash<Library> {
            &self.shared.library
        }
        fn book(&self) -> &LazyHash<FontBook> {
            &self.shared.book
        }
        fn main(&self) -> FileId {
            self.main.id()
        }
        fn source(&self, id: FileId) -> FileResult<Source> {
            if id == self.main.id() { Ok(self.main.clone()) } else { Err(FileError::NotFound(id.vpath().as_rootless_path().into())) }
        }
        fn file(&self, id: FileId) -> FileResult<Bytes> {
            Err(FileError::NotFound(id.vpath().as_rootless_path().into()))
        }
        fn font(&self, index: usize) -> Option<Font> {
            self.shared.fonts.get(index).cloned()
        }
        fn today(&self, _offset: Option<i64>) -> Option<Datetime> {
            None
        }
    }

    /// Typst source compiled to pages.
    pub fn compile(source: &str) -> Result<PagedDocument, String> {
        let world = OneFile { shared: shared(), main: Source::new(FileId::new(None, VirtualPath::new("main.typ")), source.to_string()) };
        typst::compile::<PagedDocument>(&world).output.map_err(|errors| errors.iter().map(|e| e.message.to_string()).collect::<Vec<_>>().join("; "))
    }

    pub fn render(math: &[String], style: Style) -> Result<Vec<Image>, String> {
        let doc = compile(&super::source(math, style))?;
        if doc.pages.len() != math.len() {
            return Err("formulas and pages do not match".into());
        }
        Ok(doc
            .pages
            .iter()
            .map(|page| {
                let pixmap = typst_render::render(page, style.scale);
                let pixels = pixmap
                    .data()
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|p| {
                        // tiny-skia keeps the colours premultiplied by alpha.
                        let a = p[3];
                        let un = |c: u8| if a == 0 { 0 } else { ((c as u32 * 255 + a as u32 / 2) / a as u32).min(255) as u8 };
                        [un(p[0]), un(p[1]), un(p[2]), a]
                    })
                    .collect();
                Image { width: pixmap.width() as usize, height: pixmap.height() as usize, pixels }
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shrinking_and_placing() {
        let img = Image { width: 4, height: 2, pixels: vec![[255, 0, 0, 255]; 8] };
        let half = img.shrink(0.5);
        assert_eq!((half.width, half.height), (2, 1));
        assert_eq!(half.pixels[0], [255, 0, 0, 255]);
        let placed = half.placed(5, 3);
        assert_eq!(placed.pixels[5], [255, 0, 0, 255], "centred: second line");
        assert_eq!(placed.pixels[0][3], 0, "transparent around it");
        assert_eq!(placed.over([0, 0, 255])[0], [0, 0, 255]);
    }

    #[test]
    fn png_is_read_back() {
        // The PNG the charts are written with: filter "none".
        let mut pixels = vec![[0, 0, 0, 0]; 6];
        pixels[4] = [10, 20, 30, 200];
        let png = crate::tui::chart_image::Canvas { width: 3, height: 2, pixels: pixels.clone() }.png();
        let img = decode_png(&png).unwrap();
        assert_eq!((img.width, img.height), (3, 2));
        assert_eq!(img.pixels, pixels);
        assert!(decode_png(b"nope").is_err());
    }

    #[test]
    fn formulas_are_typeset() {
        if !available() {
            return;
        }
        let style = Style { size: 10.0, scale: 2.0, color: [0, 0, 0], display: false };
        let out = formulas(&["a = b/c".into(), "x^2".into(), "#oops(".into()], style);
        assert!(out[0].as_ref().is_some_and(|i| i.width > 10 && i.height > 10));
        assert!(out[1].is_some());
        assert!(out[2].is_none(), "a broken formula alone fails");
        // A fraction in display style is taller than one inline.
        let tall = formulas(&["a = b/c".into()], Style { display: true, ..style });
        assert!(tall[0].as_ref().unwrap().height > out[0].as_ref().unwrap().height);
    }
}
