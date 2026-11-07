//! Reading and writing documents: plain Markdown (the default, `.hcmd`) and
//! compressed Markdown (`.hcmd.gz`, `.hcmd.zst`, `.hcmd.xz`). Plain `.md` is read
//! as well.
//!
//! Compression is recognised by the magic bytes as well as the name, so a
//! misnamed file still opens. A compressed file holds an ordinary Markdown
//! document – no extra structure; documents with assets belong in a
//! TextBundle. A `.zip` holding a single `.md` file is read too.

use std::io::{Read, Write};
use std::path::Path;

/// File extension of hardcalc documents (without the dot).
pub const EXTENSION: &str = "hcmd";

/// Suffixes of documents: `.hcmd` and plain `.md`.
pub const DOCUMENT_SUFFIXES: [&str; 2] = [".hcmd", ".md"];

/// Whether `name` is a (possibly compressed) document: `a.hcmd`, `a.hcmd.gz`, `a.md`.
pub fn is_document_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let base = [".gz", ".zst", ".xz"].iter().find_map(|c| lower.strip_suffix(c)).unwrap_or(&lower);
    DOCUMENT_SUFFIXES.iter().any(|s| base.ends_with(s))
}

/// The name without document and compression suffixes:
/// `Report.hcmd` / `Report.hcmd.gz` / `Report.md` → `Report`.
pub fn document_stem(name: &str) -> &str {
    let lower = name.to_ascii_lowercase();
    let mut end = name.len();
    if let Some(c) = [".gz", ".zst", ".xz"].iter().find(|c| lower.ends_with(*c)) {
        end -= c.len();
    }
    if let Some(s) = DOCUMENT_SUFFIXES.iter().find(|s| lower[..end].ends_with(*s)) {
        end -= s.len();
    }
    &name[..end]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Compression {
    #[default]
    None,
    Gzip,
    Zstd,
    Xz,
}

impl Compression {
    pub const ALL: [Compression; 4] = [Compression::None, Compression::Gzip, Compression::Zstd, Compression::Xz];

    /// File name suffix, `""` for none.
    pub fn suffix(self) -> &'static str {
        match self {
            Compression::None => "",
            Compression::Gzip => ".gz",
            Compression::Zstd => ".zst",
            Compression::Xz => ".xz",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Compression::None => "none",
            Compression::Gzip => "gzip",
            Compression::Zstd => "zstd",
            Compression::Xz => "xz",
        }
    }

    pub fn parse(s: &str) -> Option<Compression> {
        match s {
            "none" | "off" => Some(Compression::None),
            "gzip" | "gz" => Some(Compression::Gzip),
            "zstd" | "zst" => Some(Compression::Zstd),
            "xz" => Some(Compression::Xz),
            _ => None,
        }
    }

    /// By the file name: `budget.hcmd.gz` → gzip.
    pub fn from_path(path: &Path) -> Compression {
        match path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
            Some("gz") => Compression::Gzip,
            Some("zst") => Compression::Zstd,
            Some("xz") => Compression::Xz,
            _ => Compression::None,
        }
    }

    /// By the first bytes of the file.
    pub fn detect(bytes: &[u8]) -> Compression {
        if bytes.starts_with(&[0x1f, 0x8b]) {
            Compression::Gzip
        } else if bytes.starts_with(&[0x28, 0xb5, 0x2f, 0xfd]) {
            Compression::Zstd
        } else if bytes.starts_with(&[0xfd, 0x37, 0x7a, 0x58, 0x5a, 0x00]) {
            Compression::Xz
        } else {
            Compression::None
        }
    }
}

/// `budget.hcmd` with the suffix of `c` (any other compression suffix replaced).
pub fn with_compression(path: &Path, c: Compression) -> std::path::PathBuf {
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let base = Compression::ALL.iter().fold(name.clone(), |n, k| match k.suffix() {
        "" => n,
        s if n.to_ascii_lowercase().ends_with(s) => n[..n.len() - s.len()].to_string(),
        _ => n,
    });
    path.with_file_name(format!("{base}{}", c.suffix()))
}

fn invalid(e: impl std::fmt::Display) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())
}

pub fn decompress(bytes: &[u8]) -> std::io::Result<(Vec<u8>, Compression)> {
    let c = Compression::detect(bytes);
    let mut out = Vec::new();
    match c {
        Compression::None => out.extend_from_slice(bytes),
        Compression::Gzip => {
            flate2::read::MultiGzDecoder::new(bytes).read_to_end(&mut out)?;
        }
        Compression::Zstd => {
            let mut dec = ruzstd::decoding::StreamingDecoder::new(bytes).map_err(invalid)?;
            dec.read_to_end(&mut out)?;
        }
        Compression::Xz => lzma_rs::xz_decompress(&mut std::io::BufReader::new(bytes), &mut out).map_err(invalid)?,
    }
    Ok((out, c))
}

pub fn compress(data: &[u8], c: Compression) -> std::io::Result<Vec<u8>> {
    Ok(match c {
        Compression::None => data.to_vec(),
        Compression::Gzip => {
            let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
            enc.write_all(data)?;
            enc.finish()?
        }
        Compression::Zstd => ruzstd::encoding::compress_to_vec(data, ruzstd::encoding::CompressionLevel::Fastest),
        Compression::Xz => {
            let mut out = Vec::new();
            lzma_rs::xz_compress(&mut std::io::BufReader::new(data), &mut out)?;
            out
        }
    })
}

/// The text of a `.zip` that holds exactly one document (`.hcmd` or `.md`).
fn single_md_in_zip(bytes: &[u8]) -> std::io::Result<String> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(invalid)?;
    let names: Vec<String> = zip
        .file_names()
        .filter_map(|n| n.ok().map(|n| n.to_string()))
        .filter(|n| is_document_name(n) && !n.starts_with("__MACOSX"))
        .collect();
    match names.as_slice() {
        [one] => {
            let mut text = String::new();
            zip.by_name(one).map_err(invalid)?.read_to_string(&mut text)?;
            Ok(text)
        }
        [] => Err(invalid("the zip holds no .hcmd or .md file")),
        _ => Err(invalid("the zip holds several .md files – unpack it first")),
    }
}

/// Reads a document's text and how it was compressed. For a TextBundle:
/// its document (with tables changed in its TSVs), in canonical form.
pub fn read_text(path: &Path) -> std::io::Result<(String, Compression)> {
    if crate::bundle::Kind::from_path(path).is_some() {
        let b = crate::bundle::Bundle::read(path)?;
        let (doc, _) = crate::bundle::document(&b).map_err(invalid)?;
        return Ok((crate::write(&doc), Compression::None));
    }
    let bytes = std::fs::read(path)?;
    if bytes.starts_with(b"PK\x03\x04") {
        return single_md_in_zip(&bytes).map(|t| (t, Compression::None));
    }
    let (data, c) = decompress(&bytes)?;
    let text = String::from_utf8(data).map_err(|_| invalid(format!("{} is not UTF-8 text", path.display())))?;
    Ok((text, c))
}

/// Writes a document's text, compressed as asked. Goes through a temporary
/// file, so a failed write never truncates the original.
pub fn write_text(path: &Path, text: &str, c: Compression) -> std::io::Result<usize> {
    let bytes = compress(text.as_bytes(), c)?;
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let tmp = path.with_file_name(format!(".{name}.tmp"));
    std::fs::write(&tmp, &bytes).and_then(|_| std::fs::rename(&tmp, path)).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })?;
    Ok(bytes.len())
}

#[cfg(test)]
mod tests {
    #[test]
    fn document_names() {
        for name in ["a.hcmd", "a.HCMD", "a.md", "a.hcmd.gz", "a.hcmd.zst", "a.md.xz"] {
            assert!(is_document_name(name), "{name}");
        }
        for name in ["a.txt", "a.gz", "a.xlsx", "hcmd"] {
            assert!(!is_document_name(name), "{name}");
        }
        assert_eq!(document_stem("Report.hcmd"), "Report");
        assert_eq!(document_stem("Report.hcmd.gz"), "Report");
        assert_eq!(document_stem("Report.md"), "Report");
        assert_eq!(document_stem("Report.HCMD.zst"), "Report");
        assert_eq!(document_stem("Report.pdf"), "Report.pdf");
    }

    use super::*;

    const TEXT: &str = "# Budget\n\n| Item | Amount |\n|---|---|\n| Rent | 1200 |\n";

    #[test]
    fn roundtrip_every_compression() {
        for c in Compression::ALL {
            let bytes = compress(TEXT.as_bytes(), c).unwrap();
            assert_eq!(Compression::detect(&bytes), c, "{c:?}");
            let (back, found) = decompress(&bytes).unwrap();
            assert_eq!((String::from_utf8(back).unwrap().as_str(), found), (TEXT, c));
        }
    }

    #[test]
    fn names() {
        assert_eq!(Compression::from_path(Path::new("a.hcmd.zst")), Compression::Zstd);
        assert_eq!(with_compression(Path::new("a.hcmd"), Compression::Gzip), Path::new("a.hcmd.gz"));
        assert_eq!(Compression::from_path(Path::new("a.md.zst")), Compression::Zstd);
        assert_eq!(Compression::from_path(Path::new("a.hcmd")), Compression::None);
        assert_eq!(with_compression(Path::new("d/a.hcmd.gz"), Compression::Xz), Path::new("d/a.hcmd.xz"));
        assert_eq!(with_compression(Path::new("a.md"), Compression::Gzip), Path::new("a.md.gz"));
        assert_eq!(with_compression(Path::new("a.hcmd.gz"), Compression::None), Path::new("a.hcmd"));
    }

    #[test]
    fn files_and_misnamed_files() {
        let dir = std::env::temp_dir().join(format!("hardcalc-storage-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // Written as xz, named .md: opens anyway.
        let wrong = dir.join("falsch.md");
        write_text(&wrong, TEXT, Compression::Xz).unwrap();
        assert_eq!(read_text(&wrong).unwrap(), (TEXT.to_string(), Compression::Xz));
        // A zip with one .md.
        let zipped = dir.join("eins.zip");
        let mut w = zip::ZipWriter::new(std::fs::File::create(&zipped).unwrap());
        w.start_file("budget.md", zip::write::SimpleFileOptions::default()).unwrap();
        w.write_all(TEXT.as_bytes()).unwrap();
        w.finish().unwrap();
        assert_eq!(read_text(&zipped).unwrap().0, TEXT);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
