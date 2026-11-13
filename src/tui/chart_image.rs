//! Charts as pixel images for terminals with a graphics protocol (Kitty,
//! iTerm2): the plot area of line, area and scatter charts is drawn here,
//! the title and the axis labels stay text in the terminal's font.

use crate::chart::{ChartData, ChartType};
use ratatui::layout::Rect;

/// A chart's plot area to be shown as an image after the frame is drawn.
#[derive(Debug, Clone, PartialEq)]
pub struct ChartImage {
    /// Where on screen, in cells.
    pub rect: Rect,
    pub data: ChartData,
    /// On the white paper of a layout (else on the terminal's background).
    pub paper: bool,
}

impl ChartImage {
    /// Identifies what the image shows, to send it only when it changes.
    pub fn key(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        format!("{:?}", self.data).hash(&mut h);
        (self.rect.x, self.rect.y, self.rect.width, self.rect.height, self.paper).hash(&mut h);
        h.finish()
    }
}

/// Screen colours of the series, as close to the terminal palette the text
/// charts use (cyan, yellow, magenta, green, light blue, light red).
const SCREEN: [[u8; 3]; 6] = [[0, 190, 210], [225, 185, 0], [205, 85, 205], [60, 185, 80], [95, 150, 255], [240, 95, 95]];

fn series_rgb(i: usize, paper: bool) -> [u8; 3] {
    if paper {
        let (r, g, b) = super::chart_view::PAPER[i % super::chart_view::PAPER.len()];
        [r, g, b]
    } else {
        SCREEN[i % SCREEN.len()]
    }
}

/// An RGBA picture, straight alpha.
pub struct Canvas {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<[u8; 4]>,
}

impl Canvas {
    pub fn new(width: usize, height: usize, background: [u8; 4]) -> Canvas {
        Canvas { width, height, pixels: vec![background; width * height] }
    }

    /// Paints `color` over pixel (x, y) with coverage `a` (0…1).
    fn blend(&mut self, x: i64, y: i64, color: [u8; 3], a: f64) {
        if x < 0 || y < 0 || x >= self.width as i64 || y >= self.height as i64 || a <= 0.0 {
            return;
        }
        let p = &mut self.pixels[y as usize * self.width + x as usize];
        let (sa, da) = (a.min(1.0), p[3] as f64 / 255.0);
        let oa = sa + da * (1.0 - sa);
        if oa <= 0.0 {
            return;
        }
        for k in 0..3 {
            let v = (color[k] as f64 * sa + p[k] as f64 * da * (1.0 - sa)) / oa;
            p[k] = v.round().clamp(0.0, 255.0) as u8;
        }
        p[3] = (oa * 255.0).round() as u8;
    }

    /// An anti-aliased line of `width` pixels: every pixel is covered by
    /// how near it is to the segment.
    fn line(&mut self, (x0, y0): (f64, f64), (x1, y1): (f64, f64), width: f64, color: [u8; 3], alpha: f64) {
        let r = width / 2.0;
        let (lo_x, hi_x) = ((x0.min(x1) - r - 1.0).floor() as i64, (x0.max(x1) + r + 1.0).ceil() as i64);
        let (lo_y, hi_y) = ((y0.min(y1) - r - 1.0).floor() as i64, (y0.max(y1) + r + 1.0).ceil() as i64);
        let (dx, dy) = (x1 - x0, y1 - y0);
        let len2 = dx * dx + dy * dy;
        for y in lo_y.max(0)..=hi_y.min(self.height as i64 - 1) {
            for x in lo_x.max(0)..=hi_x.min(self.width as i64 - 1) {
                let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
                let t = if len2 == 0.0 { 0.0 } else { (((px - x0) * dx + (py - y0) * dy) / len2).clamp(0.0, 1.0) };
                let (cx, cy) = (x0 + t * dx, y0 + t * dy);
                let d = ((px - cx).powi(2) + (py - cy).powi(2)).sqrt();
                let cover = (r + 0.5 - d).clamp(0.0, 1.0);
                if cover > 0.0 {
                    self.blend(x, y, color, cover * alpha);
                }
            }
        }
    }

    fn dot(&mut self, (cx, cy): (f64, f64), r: f64, color: [u8; 3]) {
        for y in (cy - r - 1.0).floor() as i64..=(cy + r + 1.0).ceil() as i64 {
            for x in (cx - r - 1.0).floor() as i64..=(cx + r + 1.0).ceil() as i64 {
                let d = ((x as f64 + 0.5 - cx).powi(2) + (y as f64 + 0.5 - cy).powi(2)).sqrt();
                self.blend(x, y, color, (r + 0.5 - d).clamp(0.0, 1.0));
            }
        }
    }

    /// A PNG of the picture (8-bit RGBA).
    pub fn png(&self) -> Vec<u8> {
        use std::io::Write;
        let mut raw = Vec::with_capacity(self.height * (self.width * 4 + 1));
        for row in self.pixels.chunks(self.width) {
            // Filter "up" would compress a little better; "none" keeps it simple
            // and plain backgrounds compress well anyway.
            raw.push(0);
            for p in row {
                raw.extend_from_slice(p);
            }
        }
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
        let _ = z.write_all(&raw);
        let idat = z.finish().unwrap_or_default();
        let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
        let chunk = |out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]| {
            out.extend_from_slice(&(data.len() as u32).to_be_bytes());
            let mut crc = flate2::Crc::new();
            crc.update(kind);
            crc.update(data);
            out.extend_from_slice(kind);
            out.extend_from_slice(data);
            out.extend_from_slice(&crc.sum().to_be_bytes());
        };
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&(self.width as u32).to_be_bytes());
        ihdr.extend_from_slice(&(self.height as u32).to_be_bytes());
        // 8 bit, colour type 6 (RGBA), deflate, no filter method, no interlace.
        ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
        chunk(&mut out, b"IHDR", &ihdr);
        chunk(&mut out, b"IDAT", &idat);
        chunk(&mut out, b"IEND", &[]);
        out
    }
}

/// Draws the plot area of `data` (line, area or scatter) on a canvas of
/// `cols` × `rows` cells of `cell` pixels. The value range runs from the
/// middle of the top row to the middle of the bottom row, where the text
/// labels stand.
pub fn plot(data: &ChartData, cols: u16, rows: u16, cell: (u16, u16), paper: bool) -> Canvas {
    let (cw, ch) = (cell.0.max(1) as f64, cell.1.max(1) as f64);
    let (w, h) = ((cols as f64 * cw) as usize, (rows as f64 * ch) as usize);
    let background = if paper { [255, 255, 255, 255] } else { [0, 0, 0, 0] };
    let mut c = Canvas::new(w.max(1), h.max(1), background);
    let b = super::chart_view::bounds(data);
    let scale = (ch / 16.0).max(1.0);
    let top = ch / 2.0;
    let bottom = h as f64 - ch / 2.0;
    let left = 1.5 * scale;
    let right = w as f64 - 2.0 * scale;
    let py = |v: f64| top + (b.hi - v) / (b.hi - b.lo) * (bottom - top);
    let px = |x: f64| left + (x - b.x_lo) / (b.x_hi - b.x_lo) * (right - left);
    let axis = if paper { [90, 90, 90] } else { [140, 140, 140] };
    // Grid at the three labelled values, the axes at the left and the bottom.
    for v in [b.lo, (b.lo + b.hi) / 2.0, b.hi] {
        let y = py(v);
        let mut x = left;
        while x < right {
            c.line((x, y), ((x + 3.0 * scale).min(right), y), scale, axis, 0.35);
            x += 7.0 * scale;
        }
    }
    if b.lo < 0.0 && b.hi > 0.0 {
        c.line((left, py(0.0)), (right, py(0.0)), scale, axis, 0.8);
    }
    c.line((left, top - ch / 2.0), (left, h as f64), 1.2 * scale, axis, 0.9);
    c.line((left, h as f64 - 0.5 * scale), (w as f64, h as f64 - 0.5 * scale), 1.2 * scale, axis, 0.9);
    let zero = py(b.lo.max(0.0).min(b.hi));
    for (k, s) in data.series.iter().enumerate() {
        let color = series_rgb(k, paper);
        let points: Vec<(f64, f64)> = s
            .values
            .iter()
            .enumerate()
            .filter_map(|(i, v)| {
                let x = match &data.xs {
                    Some(xs) => xs.get(i).copied().filter(|x| x.is_finite())?,
                    None => i as f64,
                };
                v.filter(|v| v.is_finite()).map(|v| (px(x), py(v)))
            })
            .collect();
        match data.kind {
            ChartType::Scatter => points.iter().for_each(|p| c.dot(*p, 2.5 * scale, color)),
            _ => {
                if data.kind == ChartType::Area
                    && let (Some(first), Some(last)) = (points.first(), points.last())
                {
                    // Filled from the zero line (or the lower edge) to the curve,
                    // each pixel column once.
                    for x in first.0.floor() as i64..=last.0.ceil() as i64 {
                        let cx = (x as f64 + 0.5).clamp(first.0, last.0);
                        let Some(pair) = points.windows(2).find(|p| cx >= p[0].0 && cx <= p[1].0) else { continue };
                        let ((x0, y0), (x1, y1)) = (pair[0], pair[1]);
                        let t = if x1 > x0 { (cx - x0) / (x1 - x0) } else { 0.0 };
                        let y = y0 + t * (y1 - y0);
                        let (a, b2) = if y < zero { (y, zero) } else { (zero, y) };
                        for yy in a.round() as i64..b2.round() as i64 {
                            c.blend(x, yy, color, 0.28);
                        }
                    }
                }
                for pair in points.windows(2) {
                    c.line(pair[0], pair[1], 2.0 * scale, color, 1.0);
                }
                if points.len() == 1 {
                    c.dot(points[0], 2.5 * scale, color);
                }
            }
        }
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chart::Series;
    use crate::units::Unit;

    fn data(kind: ChartType) -> ChartData {
        ChartData {
            title: "t".into(),
            kind,
            categories: vec!["0".into(), "1".into(), "2".into()],
            xs: Some(vec![0.0, 1.0, 2.0]),
            series: vec![Series { name: "y".into(), values: vec![Some(0.0), Some(4.0), Some(-2.0)] }],
            unit: Unit::none(),
            x_unit: Unit::none(),
            sheet: usize::MAX,
            rows_are_categories: false,
            x_column: None,
            decimal_comma: false,
        }
    }

    #[test]
    fn plot_draws_the_curve_in_its_colour() {
        let c = plot(&data(ChartType::Line), 20, 6, (8, 16), false);
        assert_eq!((c.width, c.height), (160, 96));
        // The peak (x = 1, y = 4) is the top value: in the middle of the top row.
        let (x, y) = (c.width / 2, 8);
        let near: Vec<[u8; 4]> = (y - 2..=y + 2).flat_map(|yy| (x - 2..=x + 2).map(move |xx| (xx, yy))).map(|(xx, yy)| c.pixels[yy * c.width + xx]).collect();
        assert!(near.iter().any(|p| p[3] > 200 && p[0] < 60 && p[2] > 150), "cyan line at the peak");
        // Far from the curve the screen background stays transparent.
        assert_eq!(c.pixels[c.width * 90 + c.width - 30][3], 0);
    }

    #[test]
    fn png_has_a_valid_frame() {
        let c = plot(&data(ChartType::Area), 10, 4, (8, 16), true);
        let png = c.png();
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert_eq!(&png[12..16], b"IHDR");
        assert_eq!(u32::from_be_bytes([png[16], png[17], png[18], png[19]]), 80);
        assert!(png.ends_with(&[0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82]), "IEND with its CRC");
        // Decompressed, the data are one filter byte and 4 bytes per pixel a row.
        let idat_len = u32::from_be_bytes([png[33], png[34], png[35], png[36]]) as usize;
        let mut raw = Vec::new();
        use std::io::Read;
        flate2::read::ZlibDecoder::new(&png[41..41 + idat_len]).read_to_end(&mut raw).unwrap();
        assert_eq!(raw.len(), 64 * (80 * 4 + 1));
    }
}
