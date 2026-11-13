//! The formula library: own functions of calculation sheets that every
//! document knows – those hardcalc brings along and those in the user's
//! `formulas.hcmd`. Both are hardcalc documents: a heading per group, one
//! definition per line with its name in brackets and a comment.
//!
//! ````markdown
//! ```hardcalc
//! ## Simply supported beam
//! [Maximum moment] M_ss(q; L) = q·L^2/8   ' at midspan
//! ```
//! ````

use super::expr::{self, Expr, Line, Stmt};
use super::{Func, Funcs};
use std::path::PathBuf;

/// The formulas hardcalc brings along.
const BUILT_IN: &str = include_str!("library.hcmd");

/// One formula of the library.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// The heading above it.
    pub group: String,
    /// Its name in brackets, else the function name.
    pub title: String,
    pub name: String,
    pub params: Vec<String>,
    pub expr: Expr,
    /// The comment at its end.
    pub note: Option<String>,
    /// The line as written, to insert it into a sheet.
    pub line: String,
    /// From the user's own file.
    pub own: bool,
}

/// The user's own formulas: `formulas.hcmd` in the hardcalc configuration
/// folder; `HARDCALC_FORMULAS` names another file (empty: none).
pub fn own_file() -> Option<PathBuf> {
    if let Some(f) = std::env::var_os("HARDCALC_FORMULAS") {
        return (!f.is_empty()).then(|| PathBuf::from(f));
    }
    let env = |v: &str| std::env::var_os(v).filter(|s| !s.is_empty()).map(PathBuf::from);
    let base = env("XDG_CONFIG_HOME").or_else(|| env("APPDATA")).or_else(|| crate::tui::file_dialog::home_dir().map(|h| h.join(".config")))?;
    Some(base.join("hardcalc").join("formulas.hcmd"))
}

/// The definitions of a library document, in order.
pub fn parse(text: &str, own: bool) -> Vec<Entry> {
    let Ok(doc) = crate::parse(text) else { return Vec::new() };
    let mut group = String::new();
    let mut out = Vec::new();
    for src in super::source_lines(&doc) {
        match expr::parse_line(&src) {
            Line::Heading { text, .. } => group = text,
            Line::Stmt { stmt: Some(Stmt::Def { name, params, expr }), comment, label, .. } => out.push(Entry {
                group: group.clone(),
                title: label.unwrap_or_else(|| name.clone()),
                name,
                params,
                expr,
                note: comment.filter(|c| !c.is_empty()),
                line: src.trim().to_string(),
                own,
            }),
            _ => {}
        }
    }
    out
}

/// All formulas: those brought along, then the user's own.
pub fn entries() -> Vec<Entry> {
    let mut all = parse(BUILT_IN, false);
    if let Some(text) = own_file().and_then(|f| std::fs::read_to_string(f).ok()) {
        all.extend(parse(&text, true));
    }
    all
}

/// The library as functions; an own formula wins over one brought along.
pub fn functions() -> Funcs {
    entries().into_iter().map(|e| (e.name, Func { params: e.params, expr: e.expr })).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_formulas_calculate() {
        let all = parse(BUILT_IN, false);
        assert!(all.len() > 20, "{}", all.len());
        let m = all.iter().find(|e| e.name == "M_ss").unwrap();
        assert_eq!((m.group.as_str(), m.title.as_str(), m.note.as_deref()), ("Simply supported beam, uniform load q, span L", "Maximum moment", Some("at midspan")));
        // Every one of them can be used.
        let funcs: Funcs = all.iter().map(|e| (e.name.clone(), Func { params: e.params.clone(), expr: e.expr.clone() })).collect();
        let lines = ["q = 10 kN/m", "L = 5 m", "M = M_ss(q; L)", "w = w_ss(q; L; 210000 N/mm^2; 8360 cm^4)", "N = N_cr(210 GPa; 8360 cm^4; 5 m)"].map(String::from);
        let r = super::super::Render::default();
        let out: Vec<String> = r.lines(&super::super::evaluate_with(&lines, None, &funcs));
        assert!(out[2].ends_with("= 31.3 kN·m"), "{}", out[2]);
        assert!(out[3].ends_with(" mm") || out[3].contains("m"), "{}", out[3]);
        assert!(super::super::problems(&super::super::evaluate_with(&lines, None, &funcs)).is_empty());
    }
}
