//! The font of the PDF pages: DejaVu Sans Mono (assets/fonts, license in
//! assets/fonts/THIRD-PARTY-FONTS.txt), embedded as a Unicode font – Greek letters, ≤, ✓,
//! arrows print as they are. Only the glyphs a document uses are embedded.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

/// The four styles: regular, bold, italic, bold italic.
const FILES: [&[u8]; 4] = [
    include_bytes!("../assets/fonts/DejaVuSansMono.ttf"),
    include_bytes!("../assets/fonts/DejaVuSansMono-Bold.ttf"),
    include_bytes!("../assets/fonts/DejaVuSansMono-Oblique.ttf"),
    include_bytes!("../assets/fonts/DejaVuSansMono-BoldOblique.ttf"),
];
const NAMES: [&str; 4] = ["DejaVuSansMono", "DejaVuSansMono-Bold", "DejaVuSansMono-Oblique", "DejaVuSansMono-BoldOblique"];
/// The tag a subset font's name carries (`ABCDEF+Name`).
const TAGS: [&str; 4] = ["HCMONO", "HCMONB", "HCMONI", "HCMONJ"];

/// Style index for bold / italic.
pub fn style(bold: bool, italic: bool) -> usize {
    usize::from(bold) + 2 * usize::from(italic)
}

/// The glyphs used per style, with the character each one shows.
pub type Used = [BTreeMap<u16, char>; 4];

fn faces() -> &'static [ttf_parser::Face<'static>] {
    static FACES: OnceLock<Vec<ttf_parser::Face<'static>>> = OnceLock::new();
    FACES.get_or_init(|| FILES.iter().map(|f| ttf_parser::Face::parse(f, 0).expect("bundled font")).collect())
}

/// Advance of a glyph in thousandths of the font size.
fn advance(style: usize, gid: u16) -> i32 {
    let f = &faces()[style];
    let units = f.glyph_hor_advance(ttf_parser::GlyphId(gid)).unwrap_or(0) as f32;
    (units * 1000.0 / f.units_per_em() as f32).round() as i32
}

/// The width of a character cell in thousandths of the font size (the
/// advance of the glyphs of a monospaced font).
pub fn cell_width() -> f32 {
    advance(0, faces()[0].glyph_index('0').map_or(0, |g| g.0)) as f32
}

/// `text` as runs of (style, hex string of glyph ids for `Tj`); the glyphs
/// are noted in `used`. A character the style lacks comes from the regular
/// style (the italic ones have no ✓); one no style has prints as `?`.
pub fn encode(style: usize, text: &str, used: &mut Used) -> Vec<(usize, String)> {
    let mut runs: Vec<(usize, String)> = Vec::new();
    for c in text.chars() {
        let (s, gid, shown) = match (faces()[style].glyph_index(c), faces()[0].glyph_index(c)) {
            (Some(g), _) => (style, g.0, c),
            (None, Some(g)) => (0, g.0, c),
            (None, None) => (style, faces()[style].glyph_index('?').map_or(0, |g| g.0), '?'),
        };
        used[s].insert(gid, shown);
        match runs.last_mut() {
            Some((last, hex)) if *last == s => hex.push_str(&format!("{gid:04X}")),
            _ => runs.push((s, format!("{gid:04X}"))),
        }
    }
    runs.into_iter().map(|(s, hex)| (s, format!("<{hex}>"))).collect()
}

/// The PDF objects of the fonts in use: for each style, the objects (in
/// order, numbered from `first`) and its resource name with the number of
/// its font dictionary.
pub fn objects(used: &Used, first: usize) -> (Vec<Vec<u8>>, Vec<(String, usize)>) {
    let mut objects = Vec::new();
    let mut fonts = Vec::new();
    for (s, glyphs) in used.iter().enumerate() {
        if glyphs.is_empty() {
            continue;
        }
        let id = first + objects.len();
        let (type0, cid, desc, file, unicode) = (id, id + 1, id + 2, id + 3, id + 4);
        let face = &faces()[s];
        let scale = 1000.0 / face.units_per_em() as f32;
        let name = format!("{}+{}", TAGS[s], NAMES[s]);
        let dw = cell_width() as i32;
        // Widths only for glyphs that are not one cell wide (combining marks).
        let odd: Vec<String> = glyphs.keys().filter(|&&g| advance(s, g) != dw).map(|&g| format!("{g} [{}]", advance(s, g))).collect();
        let bbox = face.global_bounding_box();
        let b = |v: i16| (v as f32 * scale).round() as i32;
        let italic = s >= 2;
        objects.push(format!("<< /Type /Font /Subtype /Type0 /BaseFont /{name} /Encoding /Identity-H /DescendantFonts [{cid} 0 R] /ToUnicode {unicode} 0 R >>").into_bytes());
        objects.push(
            format!(
                "<< /Type /Font /Subtype /CIDFontType2 /BaseFont /{name} /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> \
                 /FontDescriptor {desc} 0 R /DW {dw} /W [{}] /CIDToGIDMap /Identity >>",
                odd.join(" ")
            )
            .into_bytes(),
        );
        // Flags: fixed pitch (1), nonsymbolic (32), italic (64).
        objects.push(
            format!(
                "<< /Type /FontDescriptor /FontName /{name} /Flags {} /FontBBox [{} {} {} {}] /ItalicAngle {} /Ascent {} /Descent {} /CapHeight {} /StemV {} /FontFile2 {file} 0 R >>",
                1 + 32 + if italic { 64 } else { 0 },
                b(bbox.x_min),
                b(bbox.y_min),
                b(bbox.x_max),
                b(bbox.y_max),
                if italic { -11 } else { 0 },
                b(face.ascender()),
                b(face.descender()),
                b(face.capital_height().unwrap_or(face.ascender())),
                if s % 2 == 1 { 120 } else { 80 },
            )
            .into_bytes(),
        );
        let raw = subset(FILES[s], &glyphs.keys().copied().collect());
        objects.push(stream(&format!("/Length1 {}", raw.len()), &raw));
        objects.push(stream("", to_unicode(glyphs).as_bytes()));
        fonts.push((format!("F{}", s + 1), type0));
    }
    (objects, fonts)
}

/// A deflated stream object.
fn stream(extra: &str, data: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    let _ = z.write_all(data);
    let data = z.finish().unwrap_or_default();
    let mut out = format!("<< /Length {} /Filter /FlateDecode {extra} >>\nstream\n", data.len()).into_bytes();
    out.extend(data);
    out.extend(b"\nendstream");
    out
}

/// The CMap that maps the glyphs back to text (copying, searching).
fn to_unicode(glyphs: &BTreeMap<u16, char>) -> String {
    let mut s = String::from(
        "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n\
         /CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n",
    );
    let entries: Vec<(&u16, &char)> = glyphs.iter().collect();
    for chunk in entries.chunks(100) {
        s.push_str(&format!("{} beginbfchar\n", chunk.len()));
        for (g, c) in chunk {
            let utf16: String = c.encode_utf16(&mut [0; 2]).iter().map(|u| format!("{u:04X}")).collect();
            s.push_str(&format!("<{g:04X}> <{utf16}>\n"));
        }
        s.push_str("endbfchar\n");
    }
    s.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
    s
}

fn u16_at(d: &[u8], at: usize) -> usize {
    u16::from_be_bytes([d[at], d[at + 1]]) as usize
}

fn u32_at(d: &[u8], at: usize) -> usize {
    u32::from_be_bytes([d[at], d[at + 1], d[at + 2], d[at + 3]]) as usize
}

/// The font with the outlines of `keep` only (and of the glyphs they are
/// built from); the other glyphs stay, empty, so glyph ids do not change.
/// Tables for text layout (GSUB, GPOS, kern …) are left out.
pub fn subset(font: &[u8], keep: &BTreeSet<u16>) -> Vec<u8> {
    let n = u16_at(font, 4);
    let tables: Vec<([u8; 4], usize, usize)> = (0..n)
        .map(|i| {
            let r = 12 + 16 * i;
            ([font[r], font[r + 1], font[r + 2], font[r + 3]], u32_at(font, r + 8), u32_at(font, r + 12))
        })
        .collect();
    let table = |tag: &[u8; 4]| tables.iter().find(|t| &t.0 == tag).map(|&(_, o, l)| &font[o..o + l]);
    let (Some(head), Some(maxp), Some(loca), Some(glyf)) = (table(b"head"), table(b"maxp"), table(b"loca"), table(b"glyf")) else {
        return font.to_vec();
    };
    let glyphs = u16_at(maxp, 4);
    let long = u16_at(head, 50) == 1;
    let offset = |g: usize| if long { u32_at(loca, 4 * g) } else { 2 * u16_at(loca, 2 * g) };
    let data = |g: usize| &glyf[offset(g).min(glyf.len())..offset(g + 1).min(glyf.len())];
    // The glyphs to keep, with the parts of composite glyphs.
    let mut wanted: BTreeSet<usize> = keep.iter().map(|&g| g as usize).chain([0]).filter(|&g| g < glyphs).collect();
    let mut todo: Vec<usize> = wanted.iter().copied().collect();
    while let Some(g) = todo.pop() {
        let d = data(g);
        if d.len() < 10 || i16::from_be_bytes([d[0], d[1]]) >= 0 {
            continue;
        }
        let mut at = 10;
        loop {
            if at + 4 > d.len() {
                break;
            }
            let (flags, part) = (u16_at(d, at), u16_at(d, at + 2));
            if part < glyphs && wanted.insert(part) {
                todo.push(part);
            }
            at += 4 + if flags & 1 != 0 { 4 } else { 2 };
            at += if flags & 8 != 0 {
                2
            } else if flags & 0x40 != 0 {
                4
            } else if flags & 0x80 != 0 {
                8
            } else {
                0
            };
            if flags & 0x20 == 0 {
                break;
            }
        }
    }
    let mut new_glyf = Vec::new();
    let mut new_loca = Vec::with_capacity(4 * (glyphs + 1));
    for g in 0..glyphs {
        new_loca.extend((new_glyf.len() as u32).to_be_bytes());
        if wanted.contains(&g) {
            new_glyf.extend_from_slice(data(g));
            while new_glyf.len() % 4 != 0 {
                new_glyf.push(0);
            }
        }
    }
    new_loca.extend((new_glyf.len() as u32).to_be_bytes());
    let mut new_head = head.to_vec();
    // Long offsets in loca; the checksum adjustment is not recomputed.
    new_head[50..52].copy_from_slice(&1u16.to_be_bytes());
    new_head[8..12].copy_from_slice(&[0; 4]);
    const DROP: [&[u8; 4]; 8] = [b"GSUB", b"GPOS", b"GDEF", b"kern", b"DSIG", b"hdmx", b"VDMX", b"LTSH"];
    let mut out_tables: Vec<([u8; 4], Vec<u8>)> = Vec::new();
    for &(tag, o, l) in &tables {
        if DROP.contains(&&tag) {
            continue;
        }
        let body = match &tag {
            b"glyf" => new_glyf.clone(),
            b"loca" => new_loca.clone(),
            b"head" => new_head.clone(),
            _ => font[o..o + l].to_vec(),
        };
        out_tables.push((tag, body));
    }
    out_tables.sort_by_key(|t| t.0);
    let count = out_tables.len();
    let pow = (0..16).rev().find(|&p| 1usize << p <= count).unwrap_or(0);
    let mut out = Vec::new();
    out.extend_from_slice(&font[0..4]);
    out.extend((count as u16).to_be_bytes());
    out.extend(((1u16 << pow) * 16).to_be_bytes());
    out.extend((pow as u16).to_be_bytes());
    out.extend((count as u16 * 16 - (1u16 << pow) * 16).to_be_bytes());
    let mut at = 12 + 16 * count;
    let mut bodies = Vec::new();
    for (tag, body) in &out_tables {
        let checksum = body.chunks(4).fold(0u32, |sum, c| {
            let mut w = [0u8; 4];
            w[..c.len()].copy_from_slice(c);
            sum.wrapping_add(u32::from_be_bytes(w))
        });
        out.extend_from_slice(tag);
        out.extend(checksum.to_be_bytes());
        out.extend((at as u32).to_be_bytes());
        out.extend((body.len() as u32).to_be_bytes());
        let mut padded = body.clone();
        while padded.len() % 4 != 0 {
            padded.push(0);
        }
        at += padded.len();
        bodies.push(padded);
    }
    for b in bodies {
        out.extend(b);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unicode_text_is_encoded_and_noted() {
        let mut used: Used = Default::default();
        let runs = encode(0, "aγ≤✓", &mut used);
        assert_eq!(runs, vec![(0, runs[0].1.clone())]);
        assert_eq!(runs[0].1.len(), 2 + 4 * 4);
        assert_eq!(used[0].values().copied().collect::<BTreeSet<char>>(), ['a', 'γ', '≤', '✓'].into());
        // The italic style has no ✓: it comes from the regular one.
        let mut italic: Used = Default::default();
        let runs = encode(2, "ok ✓", &mut italic);
        assert_eq!(runs.iter().map(|r| r.0).collect::<Vec<_>>(), [2, 0]);
        // A character no style has: a question mark.
        let mut other: Used = Default::default();
        encode(1, "漢", &mut other);
        assert_eq!(other[1].values().copied().collect::<Vec<_>>(), ['?']);
        assert!((cell_width() - 602.0).abs() < 1.0, "{}", cell_width());
    }

    #[test]
    fn subset_keeps_the_glyphs_used_and_is_small() {
        let face = &faces()[0];
        let keep: BTreeSet<u16> = "Helloγ≤".chars().filter_map(|c| face.glyph_index(c)).map(|g| g.0).collect();
        let small = subset(FILES[0], &keep);
        assert!(small.len() < FILES[0].len() / 3, "{} of {}", small.len(), FILES[0].len());
        let parsed = ttf_parser::Face::parse(&small, 0).expect("a valid font");
        assert_eq!(parsed.number_of_glyphs(), face.number_of_glyphs());
        for &g in &keep {
            assert!(parsed.outline_glyph(ttf_parser::GlyphId(g), &mut Nothing).is_some(), "glyph {g}");
        }
        // Another glyph is still there, without its outline.
        let x = face.glyph_index('x').unwrap();
        assert!(parsed.outline_glyph(x, &mut Nothing).is_none());
    }

    struct Nothing;
    impl ttf_parser::OutlineBuilder for Nothing {
        fn move_to(&mut self, _: f32, _: f32) {}
        fn line_to(&mut self, _: f32, _: f32) {}
        fn quad_to(&mut self, _: f32, _: f32, _: f32, _: f32) {}
        fn curve_to(&mut self, _: f32, _: f32, _: f32, _: f32, _: f32, _: f32) {}
        fn close(&mut self) {}
    }
}
