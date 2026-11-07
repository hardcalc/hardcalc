//! Paths to nodes and fields: `agents."mein bot".whitelist.both[2]` (CONFIG-SPEC §5).

use crate::types::take_string;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Segment {
    Key(String),
    Index(usize),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct Path(pub Vec<Segment>);

impl Path {
    pub fn root() -> Path {
        Path(Vec::new())
    }

    pub fn is_root(&self) -> bool {
        self.0.is_empty()
    }

    pub fn key(&self, k: &str) -> Path {
        let mut p = self.clone();
        p.0.push(Segment::Key(k.to_string()));
        p
    }

    pub fn index(&self, i: usize) -> Path {
        let mut p = self.clone();
        p.0.push(Segment::Index(i));
        p
    }

    /// Whether `self` is `other` or lies below it.
    pub fn starts_with(&self, other: &Path) -> bool {
        self.0.starts_with(&other.0)
    }

    pub fn parse(s: &str) -> Result<Path, String> {
        match Path::parse_prefix(s) {
            Ok((p, used)) if s[used..].trim().is_empty() => Ok(p),
            Ok((_, used)) => Err(format!("unexpected '{}' in path", &s[used..])),
            Err(e) => Err(e),
        }
    }

    /// Reads a path at the start of `s`, up to whitespace. Returns it and the bytes consumed.
    pub fn parse_prefix(s: &str) -> Result<(Path, usize), String> {
        let mut segs = Vec::new();
        let mut pos = 0;
        loop {
            let rest = &s[pos..];
            if let Some(r) = rest.strip_prefix('[') {
                let end = r.find(']').ok_or("missing ']' in path")?;
                let n = r[..end].trim().parse().map_err(|_| format!("bad index '{}'", &r[..end]))?;
                segs.push(Segment::Index(n));
                pos += end + 2;
            } else {
                if !segs.is_empty() {
                    match rest.strip_prefix('.') {
                        Some(_) => pos += 1,
                        None => break,
                    }
                }
                let rest = &s[pos..];
                if rest.starts_with('"') {
                    let (k, used) = take_string(rest)?;
                    segs.push(Segment::Key(k));
                    pos += used;
                } else {
                    let end = rest.find(|c: char| !is_bare(c)).unwrap_or(rest.len());
                    if end == 0 {
                        return Err(format!("expected a key in path: '{s}'"));
                    }
                    segs.push(Segment::Key(rest[..end].to_string()));
                    pos += end;
                }
            }
            if !s[pos..].starts_with(['.', '[']) {
                break;
            }
        }
        Ok((Path(segs), pos))
    }
}

/// Characters of a bare TOML key.
pub fn is_bare(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-'
}

/// A key as written in a path or schema: bare if possible, otherwise quoted.
pub fn quote_key(k: &str) -> String {
    if !k.is_empty() && k.chars().all(is_bare) {
        k.to_string()
    } else {
        format!("\"{}\"", k.replace('"', "\\\""))
    }
}

impl fmt::Display for Path {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, seg) in self.0.iter().enumerate() {
            match seg {
                Segment::Key(k) if i == 0 => write!(f, "{}", quote_key(k))?,
                Segment::Key(k) => write!(f, ".{}", quote_key(k))?,
                Segment::Index(n) => write!(f, "[{n}]")?,
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_display() {
        for s in ["file_servers.nas1.tls", "agents.\"mein bot\".whitelist.both[2]", "a[0][1].b"] {
            assert_eq!(Path::parse(s).unwrap().to_string(), s);
        }
        let p = Path::parse("agents.\"mein bot\".x").unwrap();
        assert_eq!(p.0[1], Segment::Key("mein bot".into()));
        let (p, used) = Path::parse_prefix("agents.a.auto_cc \"msg\"").unwrap();
        assert_eq!((p.to_string().as_str(), used), ("agents.a.auto_cc", 16));
        assert!(Path::parse("a..b").is_err());
    }
}
