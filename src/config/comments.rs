//! Comments of the data part (CONFIG-SPEC §4): which table or key they
//! belong to, and writing them.
//!
//! The text above a `[header]` or a key splits at its last blank line: the
//! block directly above belongs to the header or key, anything further up to
//! what comes before. Comments after the last table belong to that table.

use super::edit::header_mut;
use super::path::Path;
use super::view::{collect_keys, collect_tables};
use std::collections::HashMap;
use toml_edit::DocumentMut;

/// Where comment text sits in the document.
#[derive(Debug, Clone, PartialEq)]
enum Loc {
    /// Above the key `key` of the table at the path.
    Key(Path, String),
    /// Above a `[header]`.
    Table(Path),
    /// After the last table, before the schema block.
    Trailing,
}

/// Which part of the text at a location.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Half {
    /// Before the last blank line.
    Before,
    /// After the last blank line (directly above the header or key).
    Attached,
    /// All of it (only for `Trailing`).
    All,
}

struct Part {
    owner: Path,
    loc: Loc,
    half: Half,
    lines: Vec<String>,
}

/// The text before the indentation of the line that follows, and that indentation.
fn split_indent(s: &str) -> (&str, &str) {
    let at = s.rfind('\n').map_or(0, |i| i + 1);
    s.split_at(at)
}

/// Splits at the last blank line: (before, the blank line, after).
fn split3(text: &str) -> (String, String, String) {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    match lines.iter().rposition(|l| l.trim().is_empty()) {
        Some(i) => (lines[..i].concat(), lines[i].to_string(), lines[i + 1..].concat()),
        None => (String::new(), String::new(), text.to_string()),
    }
}

/// Comment lines without `#`; `#%` lines are not comments.
fn text_of(s: &str) -> Vec<String> {
    s.lines()
        .map(str::trim)
        .filter(|l| l.starts_with('#') && !l.starts_with("#%"))
        .map(|l| {
            let t = l.trim_start_matches('#');
            t.strip_prefix(' ').unwrap_or(t).to_string()
        })
        .collect()
}

fn read(doc: &DocumentMut, loc: &Loc) -> String {
    let decor_prefix = |d: &toml_edit::Decor| d.prefix().and_then(|p| p.as_str()).unwrap_or("").to_string();
    match loc {
        Loc::Key(table, key) => super::edit::get(doc.as_table(), table)
            .and_then(|n| match n {
                super::validate::Node::Table(t) => t.key(key).map(|k| decor_prefix(k.leaf_decor())),
                _ => None,
            })
            .unwrap_or_default(),
        Loc::Table(p) => match super::edit::get(doc.as_table(), p) {
            Some(super::validate::Node::Table(t)) => decor_prefix(t.decor()),
            _ => String::new(),
        },
        Loc::Trailing => {
            let t = doc.trailing().as_str().unwrap_or("");
            t.split_inclusive('\n').take_while(|l| !l.starts_with("#%")).collect()
        }
    }
}

fn write(doc: &mut DocumentMut, loc: &Loc, text: String) {
    match loc {
        Loc::Key(table, key) => {
            if let Some(mut k) = header_mut(doc.as_table_mut(), &table.0).and_then(|t| t.key_mut(key)) {
                k.leaf_decor_mut().set_prefix(text);
            }
        }
        Loc::Table(p) => {
            if let Some(t) = header_mut(doc.as_table_mut(), &p.0) {
                t.decor_mut().set_prefix(text);
            }
        }
        Loc::Trailing => {
            let all = doc.trailing().as_str().unwrap_or("").to_string();
            let at = all.split_inclusive('\n').take_while(|l| !l.starts_with("#%")).map(str::len).sum::<usize>();
            doc.set_trailing(format!("{text}{}", &all[at..]));
        }
    }
}

/// Replaces one half of the text at a location with `lines` (already `# `-formatted).
fn replace(doc: &mut DocumentMut, loc: &Loc, half: Half, lines: &str) {
    let current = read(doc, loc);
    let (body, indent) = split_indent(&current);
    // New lines take the indentation of the header or key they stand above.
    let indented: String;
    let lines = if indent.is_empty() || half != Half::Attached {
        lines
    } else {
        indented = lines.lines().map(|l| format!("{indent}{l}\n")).collect();
        &indented
    };
    let (before, blank, attached) = split3(body);
    let new = match half {
        Half::Before if blank.is_empty() && lines.is_empty() => body.to_string(),
        Half::Before if blank.is_empty() => format!("{lines}\n{attached}"),
        Half::Before => format!("{lines}{blank}{attached}"),
        Half::Attached => format!("{before}{blank}{lines}"),
        Half::All => {
            let gap = if body.lines().any(|l| l.trim().is_empty()) || matches!(loc, Loc::Trailing) { "\n" } else { "" };
            if lines.is_empty() { gap.to_string() } else { format!("{gap}{lines}{gap}") }
        }
    };
    write(doc, loc, format!("{new}{indent}"));
}

fn parts(doc: &DocumentMut) -> Vec<Part> {
    let mut out = Vec::new();
    let mut tables = Vec::new();
    collect_tables(doc.as_table(), &Path::root(), &mut tables);
    tables.sort_by_key(|(pos, _, _)| *pos);
    let mut prev = Path::root();
    for (_, path, prefix) in &tables {
        let (before, _, attached) = split3(split_indent(prefix).0);
        out.push(Part { owner: prev.clone(), loc: Loc::Table(path.clone()), half: Half::Before, lines: text_of(&before) });
        out.push(Part { owner: path.clone(), loc: Loc::Table(path.clone()), half: Half::Attached, lines: text_of(&attached) });
        prev = path.clone();
    }
    out.push(Part { owner: prev, loc: Loc::Trailing, half: Half::All, lines: text_of(&read(doc, &Loc::Trailing)) });
    let mut keys = Vec::new();
    collect_keys(doc.as_table(), &Path::root(), &mut keys);
    for (table, key, prefix) in keys {
        let Some(super::path::Segment::Key(k)) = key.0.last().cloned() else { continue };
        let (before, _, attached) = split3(split_indent(&prefix).0);
        let loc = Loc::Key(table.clone(), k);
        out.push(Part { owner: table, loc: loc.clone(), half: Half::Before, lines: text_of(&before) });
        out.push(Part { owner: key, loc, half: Half::Attached, lines: text_of(&attached) });
    }
    out
}

/// Comments by the table or key they belong to.
pub fn comments(doc: &DocumentMut) -> HashMap<Path, String> {
    let mut out: HashMap<Path, Vec<String>> = HashMap::new();
    for p in parts(doc) {
        out.entry(p.owner).or_default().extend(p.lines);
    }
    out.into_iter().filter(|(_, v)| !v.is_empty()).map(|(k, v)| (k, v.join("\n"))).collect()
}

/// Where the comment of `path` is written: above its header or key; for
/// the root, at the top of the file.
fn home(doc: &DocumentMut, path: &Path) -> Result<(Loc, Half), String> {
    let mut keys = Vec::new();
    collect_keys(doc.as_table(), &Path::root(), &mut keys);
    let mut tables = Vec::new();
    collect_tables(doc.as_table(), &Path::root(), &mut tables);
    tables.sort_by_key(|(pos, _, _)| *pos);
    if path.is_root() {
        // Keys of the root come before all tables.
        if let Some((_, key, _)) = keys.iter().find(|(t, _, _)| t.is_root()) {
            let Some(super::path::Segment::Key(k)) = key.0.last() else { unreachable!("keys end in a key") };
            return Ok((Loc::Key(Path::root(), k.clone()), Half::Before));
        }
        return Ok(match tables.first() {
            Some((_, p, _)) => (Loc::Table(p.clone()), Half::Before),
            None => (Loc::Trailing, Half::All),
        });
    }
    if tables.iter().any(|(_, p, _)| p == path) {
        return Ok((Loc::Table(path.clone()), Half::Attached));
    }
    if let Some((table, key, _)) = keys.iter().find(|(_, k, _)| k == path) {
        let Some(super::path::Segment::Key(k)) = key.0.last() else { unreachable!("keys end in a key") };
        return Ok((Loc::Key(table.clone(), k.clone()), Half::Attached));
    }
    match super::edit::get(doc.as_table(), path) {
        None => Err(format!("{path} is not in the file: set a value first")),
        Some(_) => Err(format!("{path} has no line of its own in the file (inline or dotted)")),
    }
}

/// Sets the comment of the table or key at `path`; empty text removes it.
/// Parts of it elsewhere (e.g. at the end of a table) are moved above it.
pub fn set_comment(doc: &mut DocumentMut, path: &Path, text: &str) -> Result<(), String> {
    let (loc, half) = home(doc, path)?;
    for p in parts(doc).into_iter().filter(|p| &p.owner == path && !p.lines.is_empty()) {
        replace(doc, &p.loc, p.half, "");
    }
    let lines: String = text
        .trim_end()
        .lines()
        .map(|l| if l.trim().is_empty() { "#\n".to_string() } else { format!("# {}\n", l.trim_end()) })
        .collect();
    replace(doc, &loc, half, &lines);
    Ok(())
}

/// Whether the comment of `path` can be edited.
pub fn can_comment(doc: &DocumentMut, path: &Path) -> Result<(), String> {
    home(doc, path).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(src: &str, path: &str, text: &str) -> String {
        let mut doc: DocumentMut = src.parse().unwrap();
        let p = if path.is_empty() { Path::root() } else { Path::parse(path).unwrap() };
        set_comment(&mut doc, &p, text).unwrap();
        doc.to_string()
    }

    #[test]
    fn tables_and_keys() {
        let src = "a = 1\n\n[t]\nk = 1\n\n[u]\nx = 2\n";
        assert_eq!(set(src, "t", "Note\n\nzweiter Absatz"), "a = 1\n\n# Note\n#\n# zweiter Absatz\n[t]\nk = 1\n\n[u]\nx = 2\n");
        assert_eq!(set(src, "t.k", "the port"), "a = 1\n\n[t]\n# the port\nk = 1\n\n[u]\nx = 2\n");
        // The blank line keeps it the comment of the file, not of key `a`.
        assert_eq!(set(src, "", "Kopf"), "# Kopf\n\na = 1\n\n[t]\nk = 1\n\n[u]\nx = 2\n");
        // Replacing and removing.
        let with = "a = 1\n\n# alt\n[t]\nk = 1\n";
        assert_eq!(set(with, "t", "new"), "a = 1\n\n# new\n[t]\nk = 1\n");
        assert_eq!(set(with, "t", ""), "a = 1\n\n[t]\nk = 1\n");
    }

    #[test]
    fn end_of_table_comments_move_above_the_header() {
        let src = "[a]\nk = 1\n# end of a\n\n# above b\n[b]\nk = 2\n# end of b\n\n#%hardconf 1\n";
        let doc: DocumentMut = src.parse().unwrap();
        let c = comments(&doc);
        assert_eq!(c[&Path::parse("a").unwrap()], "end of a");
        assert_eq!(c[&Path::parse("b").unwrap()], "above b\nend of b");
        assert_eq!(set(src, "a", "new a"), "# new a\n[a]\nk = 1\n\n# above b\n[b]\nk = 2\n# end of b\n\n#%hardconf 1\n");
        assert_eq!(set(src, "b", ""), "[a]\nk = 1\n# end of a\n\n[b]\nk = 2\n\n#%hardconf 1\n");
    }

    #[test]
    fn indentation_and_missing_places() {
        let src = "[t]\n  # alt\n  k = 1\n";
        let doc: DocumentMut = src.parse().unwrap();
        assert_eq!(comments(&doc)[&Path::parse("t.k").unwrap()], "alt");
        assert_eq!(set(src, "t.k", "new"), "[t]\n  # new\n  k = 1\n");
        let mut doc: DocumentMut = "[t]\nk = 1\n".parse().unwrap();
        assert!(set_comment(&mut doc, &Path::parse("t.missing").unwrap(), "x").unwrap_err().contains("not in the file"));
        let mut doc: DocumentMut = "[t]\nw.both = []\n".parse().unwrap();
        assert!(set_comment(&mut doc, &Path::parse("t.w").unwrap(), "x").is_err());
    }
}
