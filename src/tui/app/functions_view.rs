//! The "ƒ functions" tab: own functions (editable) and the built-in ones.

use super::{App, CellView, ViewKind};
use crate::engine::{col_letters, Cell};
use crate::builtins::BUILTINS;
use crate::functions::{is_builtin, FnDef};
use crate::layers::{CellStyle, Color};
use crate::model::{Align, Block, Document};

/// Columns of the function overview.
pub const FN_COLS: [&str; 5] = ["Function", "Parameters", "Definition", "Description", "Used in"];

/// One row of the function overview.
#[derive(Debug, Clone, PartialEq)]
pub enum FnRow {
    /// Definition line `line` of the ```functions block at `block`.
    User { block: usize, line: usize, def: FnDef },
    /// Placeholder for a new function.
    New,
    Builtin { name: &'static str, signature: &'static str, desc: &'static str },
}

/// Appends a definition to the last ```functions block, creating one after
/// the ```types block (or the title) if there is none.
fn add_function_line(doc: &mut Document, line: String) {
    if let Some(Block::Functions(lines)) = doc.blocks.iter_mut().rev().find(|b| matches!(b, Block::Functions(_))) {
        lines.push(line);
        return;
    }
    let at = doc
        .blocks
        .iter()
        .rposition(|b| matches!(b, Block::Types(_)))
        .map(|i| i + 1)
        .unwrap_or(usize::from(matches!(doc.blocks.first(), Some(Block::Text(_)))));
    doc.blocks.insert(at, Block::Functions(vec![line]));
}

fn valid_name(s: &str) -> bool {
    s.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_') && s.chars().all(|c| c.is_alphanumeric() || c == '_')
}

impl App {
    pub fn functions_view(&self) -> bool {
        self.view.kind == ViewKind::Functions
    }

    pub fn function_rows(&self) -> Vec<FnRow> {
        let mut rows = Vec::new();
        for (b, block) in self.doc.blocks.iter().enumerate() {
            let Block::Functions(lines) = block else { continue };
            let (defs, _) = crate::functions::parse_block(lines);
            rows.extend(defs.into_iter().map(|(line, def)| FnRow::User { block: b, line, def }));
        }
        rows.push(FnRow::New);
        rows.extend(BUILTINS.iter().map(|b| FnRow::Builtin { name: b.name, signature: b.signature, desc: b.desc }));
        rows
    }

    /// Cells and functions that call `name`.
    fn function_usage(&self, name: &str) -> String {
        let call = format!("{name}(");
        let mut uses = Vec::new();
        for (s, sheet) in self.wb.sheets.iter().enumerate() {
            for (r, row) in sheet.cells.iter().enumerate() {
                for (c, cell) in row.iter().enumerate() {
                    if let Cell::Formula { src, .. } = cell
                        && src.to_lowercase().contains(&call)
                    {
                        let table = sheet.name.clone().unwrap_or_else(|| format!("#{}", s + 1));
                        uses.push(format!("{table}!{}{}", col_letters(c), r + 1));
                    }
                }
            }
        }
        for row in self.function_rows() {
            if let FnRow::User { def, .. } = row
                && def.name != name
                && def.body.to_lowercase().contains(&call)
            {
                uses.push(format!("ƒ {}", def.name));
            }
        }
        const MAX: usize = 6;
        let more = uses.len().saturating_sub(MAX);
        uses.truncate(MAX);
        if more > 0 {
            uses.push(format!("… +{more}"));
        }
        uses.join(", ")
    }

    pub(super) fn functions_cell_view(&self, r: usize, c: usize) -> CellView {
        let mut style = CellStyle::default();
        let mut error = false;
        let text = match self.function_rows().get(r) {
            Some(FnRow::User { def, .. }) => match c {
                0 => {
                    style.bold = true;
                    def.name.clone()
                }
                1 => def.params.join(", "),
                2 => {
                    error = def.compile().is_err();
                    def.body.clone()
                }
                3 => def.description.clone().unwrap_or_default(),
                _ => self.function_usage(&def.name),
            },
            Some(FnRow::Builtin { name, signature, desc }) => {
                style.fg = Some(Color::Named("darkgray".into()));
                match c {
                    0 => name.to_string(),
                    1 => signature.trim_start_matches(name).trim_matches(['(', ')']).to_string(),
                    3 => desc.to_string(),
                    _ => String::new(),
                }
            }
            _ => String::new(),
        };
        CellView { text, align: Align::Left, style, error, violation: false }
    }

    /// Editable text of the cell at the cursor.
    pub(super) fn function_raw(&self) -> String {
        let (r, c) = self.view.cursor;
        match self.function_rows().get(r) {
            Some(FnRow::User { def, .. }) => match c {
                0 => def.name.clone(),
                1 => def.params.join(", "),
                2 => def.body.clone(),
                3 => def.description.clone().unwrap_or_default(),
                _ => def.to_line(),
            },
            Some(FnRow::Builtin { signature, desc, .. }) => format!("{signature}  –  {desc}"),
            _ => String::new(),
        }
    }

    pub(super) fn function_problem(&self) -> Option<String> {
        match self.function_rows().get(self.view.cursor.0) {
            Some(FnRow::User { def, .. }) => def.compile().err(),
            _ => None,
        }
    }

    /// Why the cell at the cursor cannot be edited, if it cannot.
    pub(super) fn function_edit_problem(&self) -> Option<&'static str> {
        let (r, c) = self.view.cursor;
        match self.function_rows().get(r) {
            Some(FnRow::User { .. }) if c <= 3 => None,
            Some(FnRow::User { .. }) => Some("this column is computed – edit name, parameters, definition or description"),
            Some(FnRow::New) if c == 0 => None,
            Some(FnRow::New) => Some("enter the name of the new function in column Function first"),
            _ => Some("built-in functions are read-only"),
        }
    }

    fn check_function_name(&self, name: &str, current: Option<&str>) -> Result<(), String> {
        if !valid_name(name) {
            return Err(format!("'{name}' is not a valid function name (letters, digits, _)"));
        }
        if is_builtin(name) {
            return Err(format!("'{name}' is a built-in function"));
        }
        let taken = self.function_rows().iter().any(|r| matches!(r, FnRow::User { def, .. } if def.name == name));
        if taken && current != Some(name) {
            return Err(format!("function '{name}' already exists"));
        }
        Ok(())
    }

    pub(super) fn commit_function(&mut self, text: String) {
        let (r, c) = self.view.cursor;
        let text = text.trim().to_string();
        match self.function_rows().get(r).cloned() {
            Some(FnRow::User { block, line, def }) => {
                let mut new = def.clone();
                match c {
                    0 => {
                        let name = text.to_lowercase();
                        if let Err(e) = self.check_function_name(&name, Some(&def.name)) {
                            return self.error(e);
                        }
                        new.name = name;
                    }
                    1 => {
                        let params: Vec<String> = text.split(';').map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect();
                        if let Some(bad) = params.iter().find(|p| !valid_name(p)) {
                            return self.error(format!("'{bad}' is not a valid parameter name"));
                        }
                        new.params = params;
                    }
                    2 if text.is_empty() => return self.error("a function needs a definition (F8 deletes it)"),
                    2 => new.body = text.trim_start_matches('=').trim().to_string(),
                    3 => new.description = (!text.is_empty()).then_some(text),
                    _ => return,
                }
                let line_text = new.to_line();
                self.with_doc(|doc| {
                    if let Block::Functions(lines) = &mut doc.blocks[block] {
                        lines[line] = line_text;
                    }
                });
                if c == 0 && new.name != def.name {
                    self.info(format!("renamed {} → {} (formulas calling {} are not updated)", def.name, new.name, def.name));
                } else if let Err(e) = new.compile() {
                    self.error(e);
                }
            }
            Some(FnRow::New) if !text.is_empty() => {
                let name = text.to_lowercase();
                if let Err(e) = self.check_function_name(&name, None) {
                    return self.error(e);
                }
                let def = FnDef { name: name.clone(), params: Vec::new(), body: "0".into(), description: None };
                self.with_doc(|doc| add_function_line(doc, def.to_line()));
                // Continue with the parameters (Tab moves on to the definition).
                self.view.cursor = (r, 1);
                self.skip_move = true;
                self.info(format!("function {name} added – now its parameters (comma-separated), Tab, then the definition"));
            }
            _ => {}
        }
    }

    pub(super) fn delete_function(&mut self) {
        match self.function_rows().get(self.view.cursor.0).cloned() {
            Some(FnRow::User { block, line, def }) => {
                self.with_doc(|doc| {
                    if let Block::Functions(lines) = &mut doc.blocks[block] {
                        lines.remove(line);
                    }
                });
                self.info(format!("deleted function {}", def.name));
            }
            _ => self.error("only your own functions can be deleted"),
        }
    }
}
