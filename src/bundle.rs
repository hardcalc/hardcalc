//! TextBundle and TextPack (textbundle.org, version 2), and hardcalc's own
//! subformat `.hcbundle` / `.hcpack`.
//!
//! ```text
//! Beam.textbundle/            (Beam.textpack: the same as ZIP)
//! ├── info.json                   version, type, creator, hardcalc block
//! ├── text.md                     the hardcalc document – the only truth
//! └── assets/
//!     ├── raster/Loads.tsv       each named table, raw cells (for Git, other tools)
//!     └── …                       images and other files, kept as they are
//! ```
//!
//! `text.md` holds the whole document. The tables are also written as TSV;
//! info.json remembers a checksum of each, so a TSV changed by another
//! program is noticed and its rows are taken over when the bundle opens.
//! Bundles of other programs open normally but are saved as a new file
//! (rule of the spec), unless they are `transient`.

use crate::model::{Block, Document};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub const CREATOR: &str = "org.hardcalc";
/// Version of the hardcalc block in info.json.
pub const FORMAT_VERSION: u64 = 1;
const TEXT: &str = "text.md";
const RASTER: &str = "assets/raster/";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// `.textbundle` directory.
    TextBundle,
    /// `.textpack` ZIP.
    TextPack,
    /// `.hcbundle` directory (UTI org.hardcalc.bundle, inherits org.textbundle.package).
    HcBundle,
    /// `.hcpack` ZIP (UTI org.hardcalc.package, inherits org.textbundle.compressed).
    HcPack,
}

impl Kind {
    pub fn from_path(path: &Path) -> Option<Kind> {
        match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
            "textbundle" => Some(Kind::TextBundle),
            "textpack" => Some(Kind::TextPack),
            "hcbundle" => Some(Kind::HcBundle),
            "hcpack" => Some(Kind::HcPack),
            _ => None,
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Kind::TextBundle => "textbundle",
            Kind::TextPack => "textpack",
            Kind::HcBundle => "hcbundle",
            Kind::HcPack => "hcpack",
        }
    }

    pub fn parse(s: &str) -> Option<Kind> {
        Kind::from_path(Path::new(&format!("x.{s}")))
    }

    pub fn is_zip(self) -> bool {
        matches!(self, Kind::TextPack | Kind::HcPack)
    }
}

/// A bundle in memory: its text, info.json and all other files.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Bundle {
    pub text: String,
    pub info: Map<String, Value>,
    /// Files besides text.md and info.json, by path (`assets/…`).
    pub files: BTreeMap<String, Vec<u8>>,
}

fn invalid(e: impl std::fmt::Display) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())
}

impl Bundle {
    pub fn creator(&self) -> Option<&str> {
        self.info.get("creatorIdentifier").and_then(Value::as_str)
    }

    /// Written by hardcalc (or nobody said who).
    pub fn is_own(&self) -> bool {
        matches!(self.creator(), None | Some(CREATOR))
    }

    pub fn transient(&self) -> bool {
        self.info.get("transient").and_then(Value::as_bool).unwrap_or(false)
    }

    /// May hardcalc write over this bundle? (Spec: only its creator, or when
    /// transient.) Not one of a newer hardcalc: what this version does not
    /// know would be lost.
    pub fn may_overwrite(&self) -> bool {
        (self.is_own() || self.transient()) && !self.newer_format()
    }

    /// Written by a newer hardcalc, in a format this version does not know.
    pub fn newer_format(&self) -> bool {
        self.creator() == Some(CREATOR) && self.hardcalc().and_then(|h| h.get("formatVersion")).and_then(Value::as_u64).is_some_and(|v| v > FORMAT_VERSION)
    }

    fn hardcalc(&self) -> Option<&Map<String, Value>> {
        self.info.get(CREATOR).and_then(Value::as_object)
    }

    pub fn read(path: &Path) -> std::io::Result<Bundle> {
        let mut entries: BTreeMap<String, Vec<u8>> = BTreeMap::new();
        if path.is_dir() {
            read_dir(path, path, &mut entries)?;
        } else {
            let file = std::fs::File::open(path)?;
            let mut zip = zip::ZipArchive::new(file).map_err(invalid)?;
            for i in 0..zip.len() {
                let mut f = zip.by_index(i).map_err(invalid)?;
                if f.is_dir() {
                    continue;
                }
                let name = f.name().map_err(invalid)?.to_string();
                // Never a path that leads out of the bundle (`../x`, `/x`).
                if !inside(&name) {
                    continue;
                }
                let mut data = Vec::new();
                f.read_to_end(&mut data)?;
                entries.insert(name, data);
            }
            // A TextPack may hold the bundle as its single top folder.
            let prefix = common_folder(&entries);
            if let Some(p) = prefix {
                entries = entries.into_iter().map(|(k, v)| (k[p.len()..].to_string(), v)).collect();
            }
        }
        let info = match entries.remove("info.json") {
            Some(data) => match serde_json::from_slice::<Value>(&data).map_err(invalid)? {
                Value::Object(m) => m,
                _ => return Err(invalid("info.json is not an object")),
            },
            None => Map::new(),
        };
        // Exactly one text.* file; hardcalc writes text.md.
        let text_name = entries
            .keys()
            .find(|k| !k.contains('/') && k.starts_with("text."))
            .cloned()
            .ok_or_else(|| invalid(format!("{}: no text file in the bundle", path.display())))?;
        let data = entries.remove(&text_name).unwrap_or_default();
        let text = String::from_utf8(data).map_err(|_| invalid("the text file is not UTF-8"))?;
        Ok(Bundle { text, info, files: entries })
    }

    pub fn write(&self, path: &Path, kind: Kind) -> std::io::Result<()> {
        let info = serde_json::to_vec_pretty(&Value::Object(self.info.clone())).map_err(invalid)?;
        let mut all: Vec<(&str, &[u8])> = vec![("info.json", &info), (TEXT, self.text.as_bytes())];
        all.extend(self.files.iter().map(|(k, v)| (k.as_str(), v.as_slice())));
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let tmp = path.with_file_name(format!(".{name}.tmp"));
        let _ = remove(&tmp);
        let result = if kind.is_zip() {
            write_zip(&tmp, &all)
        } else {
            all.iter().filter(|(rel, _)| inside(rel)).try_for_each(|(rel, data)| {
                let p = tmp.join(rel);
                if let Some(parent) = p.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(p, data)
            })
        };
        if let Err(e) = result {
            let _ = remove(&tmp);
            return Err(e);
        }
        // Replace the old bundle only once the new one is complete. A file is
        // replaced in one step; a folder is moved aside first and comes back
        // if the new one cannot take its place.
        if !path.is_dir() {
            return std::fs::rename(&tmp, path).inspect_err(|_| {
                let _ = remove(&tmp);
            });
        }
        let old = path.with_file_name(format!(".{name}.old"));
        let _ = remove(&old);
        std::fs::rename(path, &old)?;
        match std::fs::rename(&tmp, path) {
            Ok(()) => {
                let _ = remove(&old);
                Ok(())
            }
            Err(e) => {
                let _ = std::fs::rename(&old, path);
                let _ = remove(&tmp);
                Err(e)
            }
        }
    }
}

/// Whether a path of a bundle's file stays inside the bundle: relative, with
/// `/` between its parts and no `..`.
fn inside(rel: &str) -> bool {
    !rel.is_empty() && !rel.starts_with('/') && !rel.contains('\\') && rel.split('/').all(|part| part != ".." && !part.ends_with(':'))
}

fn remove(p: &Path) -> std::io::Result<()> {
    if p.is_dir() { std::fs::remove_dir_all(p) } else { std::fs::remove_file(p) }
}

fn write_zip(path: &Path, files: &[(&str, &[u8])]) -> std::io::Result<()> {
    let mut zip = zip::ZipWriter::new(std::fs::File::create(path)?);
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (name, data) in files {
        zip.start_file(*name, opts).map_err(invalid)?;
        zip.write_all(data)?;
    }
    zip.finish().map_err(invalid)?;
    Ok(())
}

fn read_dir(root: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let p = entry?.path();
        if p.is_dir() {
            read_dir(root, &p, out)?;
        } else if let Ok(rel) = p.strip_prefix(root) {
            let rel = rel.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/");
            if rel != ".DS_Store" && !rel.ends_with("/.DS_Store") {
                out.insert(rel, std::fs::read(&p)?);
            }
        }
    }
    Ok(())
}

/// `Name.textbundle/` when every entry lies in that one folder.
fn common_folder(entries: &BTreeMap<String, Vec<u8>>) -> Option<String> {
    let first = entries.keys().next()?;
    let (top, _) = first.split_once('/')?;
    let prefix = format!("{top}/");
    (entries.keys().all(|k| k.starts_with(&prefix)) && !entries.contains_key("info.json")).then_some(prefix)
}

/// FNV-1a, to notice a TSV changed outside hardcalc.
fn checksum(data: &[u8]) -> String {
    let h = data.iter().fold(0xcbf29ce484222325u64, |h, b| (h ^ *b as u64).wrapping_mul(0x100000001b3));
    format!("{h:016x}")
}

fn tsv_field(s: &str) -> String {
    if s.contains(['\t', '\n', '\r', '"']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_string() }
}

/// A table as TSV: column names, then the raw cells (formulas as typed).
pub fn table_tsv(t: &crate::model::Table) -> String {
    let mut out = String::new();
    let head: Vec<String> = t.columns.iter().map(|c| tsv_field(&c.name)).collect();
    out.push_str(&head.join("\t"));
    out.push('\n');
    for row in &t.rows {
        let cells: Vec<String> = (0..t.columns.len()).map(|c| tsv_field(row.get(c).map(String::as_str).unwrap_or(""))).collect();
        out.push_str(&cells.join("\t"));
        out.push('\n');
    }
    out
}

/// File name for a table's TSV: the name without characters that trouble
/// file systems, lower-case is not forced (Loads!C12 → Loads.tsv).
fn raster_name(name: &str) -> String {
    let clean: String = name.chars().map(|c| if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_') { c } else { '_' }).collect();
    format!("{RASTER}{clean}.tsv")
}

/// What opening a bundle found besides the document.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Opened {
    /// Tables whose rows were taken from a TSV changed outside hardcalc.
    pub from_tsv: Vec<String>,
}

/// The document of a bundle; tables whose TSV was changed elsewhere take
/// its rows (and new columns keep their place by name).
pub fn document(bundle: &Bundle) -> Result<(Document, Opened), String> {
    let mut doc = crate::parse(&bundle.text).map_err(|e| e.to_string())?;
    let mut opened = Opened::default();
    let raster = bundle.hardcalc().and_then(|h| h.get("raster")).and_then(Value::as_object).cloned().unwrap_or_default();
    for block in &mut doc.blocks {
        let Block::Table(t) = block else { continue };
        let Some(name) = t.name.clone() else { continue };
        let Some(entry) = raster.get(&name) else { continue };
        let file = entry.get("file").and_then(Value::as_str).map(String::from).unwrap_or_else(|| raster_name(&name));
        let Some(data) = bundle.files.get(&file) else { continue };
        if entry.get("checksum").and_then(Value::as_str) == Some(checksum(data).as_str()) {
            continue;
        }
        let text = crate::delimited::decode(data);
        let mut records = crate::delimited::parse(&text, '\t');
        if records.is_empty() {
            continue;
        }
        let header = records.remove(0);
        // Columns by name; columns unknown to the table are ignored.
        let map: Vec<Option<usize>> = t.columns.iter().map(|c| header.iter().position(|h| h.trim() == c.name)).collect();
        if map.iter().all(Option::is_none) {
            continue;
        }
        t.rows = records.iter().map(|r| map.iter().map(|m| m.and_then(|i| r.get(i).cloned()).unwrap_or_default()).collect()).collect();
        opened.from_tsv.push(name);
    }
    Ok((doc, opened))
}

/// The bundle for a document: text.md, TSVs of the named tables, info.json.
/// `base` is the bundle it was opened from – its other files and foreign
/// info.json keys are kept.
pub fn build(doc: &Document, base: Option<&Bundle>, kind: Kind, transient: bool) -> Bundle {
    let mut files: BTreeMap<String, Vec<u8>> = base.map(|b| b.files.clone()).unwrap_or_default();
    // Old TSVs and the formula cache are made anew.
    files.retain(|k, _| !k.starts_with(RASTER) && !k.starts_with("assets/cache/"));
    let mut raster = Map::new();
    // Every table its own file, also where upper and lower case are the same
    // (`Plan` and `plan`) or the names differ only in characters a file name
    // cannot hold (`A/B` and `A_B`). Tables of the same name share one entry.
    let mut taken: Vec<String> = Vec::new();
    for block in &doc.blocks {
        if let Block::Table(t) = block
            && let Some(name) = &t.name
            && !raster.contains_key(name)
        {
            let first = raster_name(name);
            let file = (2..)
                .map(|k| format!("{} {k}.tsv", first.trim_end_matches(".tsv")))
                .find(|f| !taken.contains(&f.to_lowercase()))
                .filter(|_| taken.contains(&first.to_lowercase()))
                .unwrap_or(first);
            taken.push(file.to_lowercase());
            let data = table_tsv(t).into_bytes();
            raster.insert(name.clone(), json!({ "file": file, "checksum": checksum(&data) }));
            files.insert(file, data);
        }
    }
    let mut info = base.map(|b| b.info.clone()).unwrap_or_default();
    info.insert("version".into(), json!(2));
    info.insert("type".into(), json!("net.daringfireball.markdown"));
    info.insert("creatorIdentifier".into(), json!(CREATOR));
    info.insert("creatorURL".into(), json!("https://hardcalc.org"));
    if transient {
        info.insert("transient".into(), json!(true));
    } else {
        info.remove("transient");
    }
    let own = json!({
        "formatVersion": FORMAT_VERSION,
        "kind": kind.extension(),
        "raster": raster,
    });
    info.insert(CREATOR.into(), own);
    Bundle { text: crate::write(doc), info, files }
}

/// The name without document suffixes: `Report.hcmd.gz`, `Report.textpack` → `Report`.
pub fn stem(name: &str) -> String {
    let mut s = name.to_string();
    for suffix in [".gz", ".zst", ".xz", ".hcmd", ".md", ".textbundle", ".textpack", ".hcbundle", ".hcpack"] {
        if s.to_ascii_lowercase().ends_with(suffix) {
            s.truncate(s.len() - suffix.len());
        }
    }
    s
}

/// Where to save changes to a bundle hardcalc may not overwrite:
/// `Note.textbundle` → `Note (hardcalc).textbundle` (or `… 2` if taken).
pub fn new_file_for(path: &Path) -> PathBuf {
    let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let ext = path.extension().map(|e| e.to_string_lossy().to_string()).unwrap_or_default();
    (1..)
        .map(|k| {
            let name = if k == 1 { format!("{stem} (hardcalc).{ext}") } else { format!("{stem} (hardcalc {k}).{ext}") };
            path.with_file_name(name)
        })
        .find(|p| !p.exists())
        .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = "# Beam\n\n## Loads :: matrix\n\n| Case | n_Ed |\n|---|---|\n| G | 12.4 |\n| G+S | =B1*3 |\n";

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("hardcalc-bundle-{}-{name}", std::process::id()));
        let _ = remove(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn stems() {
        assert_eq!(stem("Beam.hcmd.gz"), "Beam");
        assert_eq!(stem("Beam.textpack"), "Beam");
        assert_eq!(stem("Beam"), "Beam");
    }

    #[test]
    fn kinds() {
        assert_eq!(Kind::from_path(Path::new("a.textpack")), Some(Kind::TextPack));
        assert_eq!(Kind::from_path(Path::new("a/B.HCBUNDLE")), Some(Kind::HcBundle));
        assert_eq!(Kind::from_path(Path::new("a.md")), None);
        assert!(Kind::HcPack.is_zip() && !Kind::TextBundle.is_zip());
    }

    #[test]
    fn roundtrip_dir_and_zip() {
        let doc = crate::parse(DOC).unwrap();
        let d = tmp("roundtrip");
        for kind in [Kind::TextBundle, Kind::TextPack, Kind::HcBundle, Kind::HcPack] {
            let path = d.join(format!("Beam.{}", kind.extension()));
            let mut b = build(&doc, None, kind, false);
            b.files.insert("assets/bilder/skizze.png".into(), vec![1, 2, 3]);
            b.write(&path, kind).unwrap();
            assert_eq!(path.is_dir(), !kind.is_zip());
            let back = Bundle::read(&path).unwrap();
            assert_eq!(back.info["version"], json!(2));
            assert_eq!(back.creator(), Some(CREATOR));
            assert_eq!(back.files["assets/raster/Loads.tsv"], b"Case\tn_Ed\nG\t12.4\nG+S\t=B1*3\n");
            assert_eq!(back.files["assets/bilder/skizze.png"], vec![1, 2, 3]);
            let (doc2, opened) = document(&back).unwrap();
            assert_eq!(doc2, doc);
            assert!(opened.from_tsv.is_empty());
        }
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn tsv_changed_elsewhere_wins() {
        let doc = crate::parse(DOC).unwrap();
        let mut b = build(&doc, None, Kind::TextBundle, false);
        b.files.insert("assets/raster/Loads.tsv".into(), b"n_Ed\tCase\n13\tG\n40\tG+S\n1\tW\n".to_vec());
        let (doc2, opened) = document(&b).unwrap();
        assert_eq!(opened.from_tsv, ["Loads"]);
        let Block::Table(t) = &doc2.blocks[1] else { panic!() };
        assert_eq!(t.rows, [["G", "13"], ["G+S", "40"], ["W", "1"]]);
    }

    #[test]
    fn foreign_bundles_are_not_overwritten() {
        let mut b = Bundle { text: "# Note".into(), ..Default::default() };
        assert!(b.may_overwrite());
        b.info.insert("creatorIdentifier".into(), json!("net.shinyfrog.bear"));
        assert!(!b.may_overwrite());
        b.info.insert("transient".into(), json!(true));
        assert!(b.may_overwrite());
        // hardcalc keeps the other program's keys, takes over as creator.
        let built = build(&Document::default(), Some(&b), Kind::TextPack, false);
        assert_eq!(built.creator(), Some(CREATOR));
        assert!(!built.transient());
        let d = tmp("foreign");
        assert_eq!(new_file_for(&d.join("Note.textbundle")), d.join("Note (hardcalc).textbundle"));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn textpack_with_top_folder() {
        let d = tmp("folder");
        let path = d.join("x.textpack");
        write_zip(&path, &[("Note.textbundle/info.json", b"{\"version\":2}"), ("Note.textbundle/text.markdown", b"# Hello\n")]).unwrap();
        let b = Bundle::read(&path).unwrap();
        assert_eq!(b.text, "# Hello\n");
        assert_eq!(b.info["version"], json!(2));
        std::fs::remove_dir_all(&d).unwrap();
    }
}

#[cfg(test)]
mod safety_tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("hardcalc-bundle-safety-{}-{name}", std::process::id()));
        let _ = remove(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn tables_with_similar_names_keep_their_rows() {
        let src = "## Plan :: matrix\n\n| k | v |\n|---|---|\n| a | 1 |\n\n## plan :: matrix\n\n| k | v |\n|---|---|\n| z | 9 |\n\n## A/B :: matrix\n\n| k |\n|---|\n| x |\n\n## A_B :: matrix\n\n| k |\n|---|\n| y |\n";
        let doc = crate::parse(src).unwrap();
        let d = tmp("names");
        for kind in [Kind::TextBundle, Kind::TextPack] {
            let path = d.join(format!("x.{}", kind.extension()));
            build(&doc, None, kind, false).write(&path, kind).unwrap();
            let back = Bundle::read(&path).unwrap();
            let (doc2, opened) = document(&back).unwrap();
            assert_eq!(doc2, doc);
            assert!(opened.from_tsv.is_empty());
            // Written over itself: the old bundle is replaced as a whole.
            build(&doc2, Some(&back), kind, false).write(&path, kind).unwrap();
            assert_eq!(document(&Bundle::read(&path).unwrap()).unwrap().0, doc);
        }
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn paths_out_of_the_bundle_are_left_out() {
        let d = tmp("slip");
        let pack = d.join("in").join("x.textpack");
        std::fs::create_dir_all(pack.parent().unwrap()).unwrap();
        write_zip(&pack, &[("text.md", b"# Hello\n"), ("../evil.txt", b"x"), ("/abs.txt", b"x"), ("assets/ok.txt", b"ok")]).unwrap();
        let b = Bundle::read(&pack).unwrap();
        assert_eq!(b.files.keys().collect::<Vec<_>>(), ["assets/ok.txt"]);
        // Also when such a path comes from elsewhere: nothing is written outside.
        let mut b = b;
        b.files.insert("../../evil.txt".into(), b"x".to_vec());
        let out = d.join("in").join("y.textbundle");
        b.write(&out, Kind::TextBundle).unwrap();
        assert!(out.join("assets/ok.txt").exists());
        assert!(!d.join("evil.txt").exists() && !d.join("in/evil.txt").exists());
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_bundle_of_a_newer_hardcalc_is_not_overwritten() {
        let mut b = build(&Document::default(), None, Kind::HcBundle, false);
        assert!(b.may_overwrite() && !b.newer_format());
        b.info.insert(CREATOR.into(), json!({ "formatVersion": FORMAT_VERSION + 1 }));
        assert!(b.newer_format() && !b.may_overwrite());
    }
}
