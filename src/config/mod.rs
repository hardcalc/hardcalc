//! Configuration files with a built-in schema, edited by `hardconf` (CONFIG-SPEC.md).
//!
//! Kept free of terminal UI code so it can move into a shared core crate later.

pub mod comments;
pub mod edit;
pub mod lines;
pub mod path;
pub mod schema;
pub mod validate;
pub mod view;

pub use path::{Path, Segment};
pub use schema::{Condition, Field, FieldType, Schema, SchemaError, Struct, Test, TypeDef};
pub use validate::{Diagnostic, Severity, Validator};

use crate::types::take_string;
use toml_edit::DocumentMut;

/// A diagnostic written into the file by another program (§6).
#[derive(Debug, Clone, PartialEq)]
pub struct FileDiagnostic {
    pub line: usize,
    pub diagnostic: Diagnostic,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LoadError(pub String);

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for LoadError {}

pub struct ConfigFile {
    /// The file as read; unchanged until saved.
    pub original: String,
    /// The data, without diagnostic lines. Saving writes this back.
    pub doc: DocumentMut,
    pub schema: Option<Schema>,
    pub schema_errors: Vec<SchemaError>,
    pub file_diagnostics: Vec<FileDiagnostic>,
}

impl ConfigFile {
    pub fn load(path: &std::path::Path) -> Result<ConfigFile, LoadError> {
        let text = std::fs::read_to_string(path).map_err(|e| LoadError(e.to_string()))?;
        ConfigFile::parse(&text)
    }

    pub fn parse(text: &str) -> Result<ConfigFile, LoadError> {
        text.parse::<DocumentMut>().map_err(|e| LoadError(e.to_string().trim_end().to_string()))?;
        let lines = lines::lines(text);
        let has_schema = is_hardconf(text);

        let mut stripped = String::with_capacity(text.len());
        let mut schema_lines = Vec::new();
        let mut file_diagnostics = Vec::new();
        for l in &lines {
            if let Some(c) = l.schema().filter(|_| has_schema) {
                if let Some((severity, rest)) = diagnostic_kind(c) {
                    file_diagnostics.push(FileDiagnostic { line: l.number, diagnostic: parse_diagnostic(severity, rest) });
                    continue;
                }
                schema_lines.push((l.number, c));
            }
            stripped.push_str(l.text);
        }
        let doc = stripped.parse::<DocumentMut>().map_err(|e| LoadError(e.to_string()))?;
        let (schema, schema_errors) = if has_schema { Schema::parse(&schema_lines) } else { (None, Vec::new()) };
        Ok(ConfigFile { original: text.to_string(), doc, schema, schema_errors, file_diagnostics })
    }

    /// The file as it would be saved: the data with every edit, the schema
    /// unchanged and the diagnostic lines removed (§7.3).
    pub fn to_text(&self) -> String {
        self.doc.to_string()
    }

    /// Findings of the schema check; empty without schema.
    pub fn validate(&self) -> Vec<Diagnostic> {
        self.schema.as_ref().map(|s| Validator::validate(s, &self.doc)).unwrap_or_default()
    }
}

/// Whether the text carries a `#%hardconf` schema block. Without it all `#%`
/// lines are ordinary comments (§1).
pub fn is_hardconf(text: &str) -> bool {
    lines::lines(text)
        .iter()
        .filter_map(|l| l.schema())
        .find(|c| diagnostic_kind(c).is_none())
        .is_some_and(|c| c.starts_with("hardconf"))
}

/// `error …` / `warning …` with the rest of the line.
fn diagnostic_kind(c: &str) -> Option<(Severity, &str)> {
    [("error", Severity::Error), ("warning", Severity::Warning)].into_iter().find_map(|(kw, sev)| {
        let rest = c.strip_prefix(kw)?;
        (rest.is_empty() || rest.starts_with(char::is_whitespace)).then(|| (sev, rest.trim()))
    })
}

/// `[path] "message"`; anything malformed is kept as message for the whole file.
fn parse_diagnostic(severity: Severity, rest: &str) -> Diagnostic {
    let parsed = (|| {
        let (path, rest) = if rest.starts_with('"') {
            (Path::root(), rest)
        } else {
            let (p, used) = Path::parse_prefix(rest).ok()?;
            (p, rest[used..].trim_start())
        };
        let (message, used) = take_string(rest).ok()?;
        rest[used..].trim().is_empty().then_some((path, message))
    })();
    let (path, message) = parsed.unwrap_or_else(|| (Path::root(), rest.to_string()));
    Diagnostic { severity, path, message }
}
