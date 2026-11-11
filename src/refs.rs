//! Keeps references in step when rows or columns of a table move (moving
//! on the headers, sorting) and when a table is renamed.
//!
//! References by column name (`Amount`, `Expenses.Amount`) need no change;
//! A1 cells (`B2`), ranges (`B2:B4`), column letters (`B`) and the same
//! from other tables (`Expenses!B2`) are rewritten, as are the targets of
//! the table's layer rules (`B1: comment`). Indexes like `Expenses[2, 3]`
//! are left alone.

use crate::engine::RefParts;
use crate::model::{Block, Document, Rule};

/// Marks a deleted row or column in [`Moves`].
pub const GONE: usize = usize::MAX;

/// New positions: `rows[old] = new`, `cols[old] = new` ([`GONE`]: deleted).
/// Indexes beyond the maps stay where they are.
pub struct Moves<'a> {
    pub rows: &'a [usize],
    pub cols: &'a [usize],
}

impl Moves<'_> {
    fn row(&self, r: usize) -> usize {
        self.rows.get(r).copied().unwrap_or(r)
    }

    fn col(&self, c: usize) -> usize {
        self.cols.get(c).copied().unwrap_or(c)
    }

    fn has_gone(&self) -> bool {
        self.rows.contains(&GONE) || self.cols.contains(&GONE)
    }

    /// The moved form of a single reference (`B2` or `B`), if it is one:
    /// [`REF_ERROR`] if its cell, row or column is gone. A column name is no
    /// reference, unless its column is deleted.
    fn reference(&self, ident: &str, names: &[String]) -> Option<String> {
        if let Some(c) = names.iter().position(|n| n == ident) {
            return (self.cols.get(c) == Some(&GONE)).then(|| REF_ERROR.to_string());
        }
        // Moves take anchored references along too (as in Excel).
        let mut p = RefParts::parse(ident)?;
        if p.row.is_none() && p.col >= self.cols.len() {
            return None;
        }
        p.col = self.col(p.col);
        p.row = p.row.map(|(r, abs)| (self.row(r), abs));
        if p.col == GONE || p.row.is_some_and(|(r, _)| r == GONE) {
            return Some(REF_ERROR.to_string());
        }
        Some(p.write())
    }

    /// The moved form of a range `a:b` where lines are deleted: it shrinks
    /// to what is left of it ([`REF_ERROR`] if nothing is). `None`: not two
    /// references, or no line of it is deleted.
    fn range(&self, a: &str, b: &str, names: &[String]) -> Option<(String, String)> {
        if names.iter().any(|n| n == a || n == b) {
            return None;
        }
        let (mut pa, mut pb) = (RefParts::parse(a)?, RefParts::parse(b)?);
        let cols = |x: usize, y: usize| (x.min(y), x.max(y));
        let (c0, c1) = cols(pa.col, pb.col);
        let kept = |range: std::ops::RangeInclusive<usize>, map: &dyn Fn(usize) -> usize| -> Vec<usize> {
            range.map(map).filter(|&n| n != GONE).collect()
        };
        let any_gone = (c0..=c1).any(|c| self.col(c) == GONE)
            || match (pa.row, pb.row) {
                (Some((x, _)), Some((y, _))) => (x.min(y)..=x.max(y)).any(|r| self.row(r) == GONE),
                _ => false,
            };
        if !any_gone {
            return None;
        }
        let col_map = |c: usize| self.col(c);
        let row_map = |r: usize| self.row(r);
        let left = kept(c0..=c1, &col_map);
        if left.is_empty() {
            return Some((REF_ERROR.into(), REF_ERROR.into()));
        }
        let (first_c, last_c) = (left[0], left[left.len() - 1]);
        if pa.col <= pb.col {
            (pa.col, pb.col) = (first_c, last_c);
        } else {
            (pa.col, pb.col) = (last_c, first_c);
        }
        if let (Some((x, xa)), Some((y, ya))) = (pa.row, pb.row) {
            let rows = kept(x.min(y)..=x.max(y), &row_map);
            if rows.is_empty() {
                return Some((REF_ERROR.into(), REF_ERROR.into()));
            }
            let (first_r, last_r) = (rows[0], rows[rows.len() - 1]);
            if x <= y {
                (pa.row, pb.row) = (Some((first_r, xa)), Some((last_r, ya)));
            } else {
                (pa.row, pb.row) = (Some((last_r, xa)), Some((first_r, ya)));
            }
        }
        Some((pa.write(), pb.write()))
    }
}

/// What a reference to a deleted cell becomes in a formula.
pub const REF_ERROR: &str = "#REF!";

/// Rewrites the references to the moved table in one formula. `here`: the
/// formula belongs to that table; `table`: its name (for `Table!B2`);
/// `names`: its column names.
pub fn rewrite_formula(src: &str, here: bool, table: Option<&str>, names: &[String], moves: &Moves) -> String {
    // Short ranges (`B2:4c`) are moved in their long form; unmoved ones stay short.
    let long = expand_short_ranges(src);
    let out = rewrite_long(&long, here, table, names, moves);
    if out == long { src.to_string() } else { out }
}

/// `B2:4c` → `B2c:B4c`, `B2a:c` → `B2a:B2c`, `B2:4` → `B2:B4` (outside strings).
pub fn expand_short_ranges(src: &str) -> String {
    if !src.contains(':') {
        return src.to_string();
    }
    let mut toks = tokens(src);
    // A range end may lex as a number and a name (`4` `c`): glue them first.
    let mut k = 0;
    while k + 1 < toks.len() {
        if let (Tok::Raw(n), Tok::Name(l)) = (&toks[k], &toks[k + 1])
            && n.chars().all(|c| c.is_ascii_digit())
            && !n.is_empty()
            && k > 0
            && toks[k - 1] == Tok::Op(':')
        {
            toks[k] = Tok::Name(format!("{n}{l}"));
            toks.remove(k + 1);
        }
        k += 1;
    }
    for i in 0..toks.len() {
        if toks[i] != Tok::Op(':') || i == 0 || i + 1 >= toks.len() {
            continue;
        }
        let (Tok::Name(a) | Tok::Raw(a), Tok::Name(b) | Tok::Raw(b)) = (&toks[i - 1], &toks[i + 1]) else { continue };
        let (Some(start), Some((col, row, layer))) = (crate::engine::RefParts::parse(a), crate::engine::parse_partial(b)) else {
            continue;
        };
        if start.row.is_none() || crate::engine::RefParts::parse(b).is_some_and(|e| e.row.is_some()) {
            continue;
        }
        let mut first = start;
        first.layer = start.layer.or(layer);
        let mut end = start;
        end.col = col.map_or(start.col, |c| c.0);
        end.col_abs = col.map_or(start.col_abs, |c| c.1);
        end.row = row.or(start.row);
        end.layer = layer.or(start.layer);
        toks[i - 1] = Tok::Name(first.write());
        toks[i + 1] = Tok::Name(end.write());
    }
    join(&toks)
}

fn rewrite_long(src: &str, here: bool, table: Option<&str>, names: &[String], moves: &Moves) -> String {
    let chars: Vec<char> = src.chars().collect();
    let next_char = |mut i: usize| {
        while i < chars.len() && chars[i] == ' ' {
            i += 1;
        }
        chars.get(i).copied()
    };
    let mut out = String::new();
    let mut i = 0;
    // `Table!` before the next reference; it also covers the end of a range.
    let mut qualifier: Option<String> = None;
    let mut brackets = 0;
    let mut last = ' ';
    while i < chars.len() {
        let c = chars[i];
        if c == '"' {
            // String literal, with "" as an escaped quote.
            let start = i;
            i += 1;
            while i < chars.len() {
                if chars[i] == '"' && chars.get(i + 1) == Some(&'"') {
                    i += 2;
                } else if chars[i] == '"' {
                    i += 1;
                    break;
                } else {
                    i += 1;
                }
            }
            out.extend(&chars[start..i]);
            // "My sheet"!B2
            if next_char(i) == Some('!') {
                qualifier = Some(chars[start + 1..i - 1].iter().collect());
            }
        } else if c == '{' {
            // Unit literal.
            let end = chars[i..].iter().position(|&c| c == '}').map_or(chars.len(), |p| i + p + 1);
            out.extend(&chars[i..end]);
            i = end;
        } else if c.is_ascii_digit() {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            if i < chars.len() && matches!(chars[i], 'e' | 'E') {
                let mut j = i + 1;
                if j < chars.len() && matches!(chars[j], '+' | '-') {
                    j += 1;
                }
                if j < chars.len() && chars[j].is_ascii_digit() {
                    i = j;
                    while i < chars.len() && chars[i].is_ascii_digit() {
                        i += 1;
                    }
                }
            }
            out.extend(&chars[start..i]);
        } else if c.is_alphabetic() || c == '_' || c == '$' {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '$') {
                i += 1;
            }
            let ident: String = chars[start..i].iter().collect();
            let next = next_char(i);
            let replaced = match next {
                Some('!') => {
                    qualifier = Some(ident.clone());
                    None
                }
                // Function call; `B2[-1]` and `B2.February` are references.
                Some('(') => None,
                _ if last == '.' || brackets > 0 => None,
                _ => {
                    let applies = match &qualifier {
                        Some(q) => Some(q.as_str()) == table,
                        None => here,
                    };
                    // A range where lines are deleted shrinks as a whole.
                    if applies && next == Some(':') && moves.has_gone() {
                        let mut j = i;
                        while j < chars.len() && chars[j] == ' ' {
                            j += 1;
                        }
                        j += 1;
                        while j < chars.len() && chars[j] == ' ' {
                            j += 1;
                        }
                        let start = j;
                        while j < chars.len() && (chars[j].is_alphanumeric() || chars[j] == '$') {
                            j += 1;
                        }
                        let end: String = chars[start..j].iter().collect();
                        if let Some((a, b)) = moves.range(&ident, &end, names) {
                            qualifier = None;
                            if a == REF_ERROR {
                                out.push_str(REF_ERROR);
                            } else {
                                out.push_str(&format!("{a}:{b}"));
                            }
                            i = j;
                            last = 'a';
                            continue;
                        }
                    }
                    if next != Some(':') {
                        qualifier = None;
                    }
                    if applies { moves.reference(&ident, names) } else { None }
                }
            };
            out.push_str(replaced.as_deref().unwrap_or(&ident));
            last = 'a';
            continue;
        } else {
            match c {
                '[' => brackets += 1,
                ']' => brackets -= 1,
                _ => {}
            }
            out.push(c);
            i += 1;
        }
        if !c.is_whitespace() {
            last = c;
        }
    }
    out
}

/// The moved form of a layer rule target (`B1`, `B1:C3`, `3`, `B`); `None`
/// if what it names is deleted, the rule then goes.
fn rewrite_target(target: &str, names: &[String], moves: &Moves) -> Option<String> {
    let t = target.trim();
    if let Some((a, b)) = t.split_once(':') {
        let (a, b) = (a.trim(), b.trim());
        if let Some((x, y)) = moves.range(a, b, names) {
            // A range that shrank to one cell is that cell.
            return (x != REF_ERROR).then(|| if x == y { x } else { format!("{x}:{y}") });
        }
        if let (Some(x), Some(y)) = (moves.reference(a, names), moves.reference(b, names)) {
            return (x != REF_ERROR && y != REF_ERROR).then(|| format!("{x}:{y}"));
        }
    }
    if let Ok(n) = t.parse::<usize>()
        && n >= 1
    {
        let r = moves.row(n - 1);
        return (r != GONE).then(|| (r + 1).to_string());
    }
    match moves.reference(t, names) {
        Some(r) if r == REF_ERROR => None,
        Some(r) => Some(r),
        None => Some(target.to_string()),
    }
}

/// Rewrites all references to table `sheet` (formulas everywhere, the
/// table's layer rules) after its rows and columns moved as in `moves`.
pub fn update_document(doc: &mut Document, sheet: usize, moves: &Moves) {
    let Some(t) = doc.blocks.iter().filter_map(|b| if let Block::Table(t) = b { Some(t) } else { None }).nth(sheet) else {
        return;
    };
    let name = t.name.clone();
    let names: Vec<String> = t.columns.iter().map(|c| c.name.clone()).collect();
    let mut table = None;
    for block in &mut doc.blocks {
        match block {
            Block::Table(t) => {
                let index = table.map_or(0, |i| i + 1);
                table = Some(index);
                for cell in t.cells_mut().filter(|c| c.trim_start().starts_with('=')) {
                    *cell = rewrite_formula(cell, index == sheet, name.as_deref(), &names, moves);
                }
            }
            Block::Layer(l) => {
                let owner = match &l.target {
                    Some(target) => name.as_deref() == Some(target.as_str()),
                    None => table == Some(sheet),
                };
                if owner {
                    let rules = std::mem::take(&mut l.rules);
                    l.rules = rules
                        .into_iter()
                        .filter_map(|rule| match rule {
                            Rule::Rule { target, body } => rewrite_target(&target, &names, moves).map(|target| Rule::Rule { target, body }),
                            raw => Some(raw),
                        })
                        .collect();
                }
            }
            _ => {}
        }
    }
}

/// Moves single cells (`B3`) instead of whole lines, for sorting a part of a
/// table: the targets of the table's layer rules – formatting, comments … –
/// follow their cell. `to(row, col)`: the new place of a cell, `None`: it
/// stays. Ranges, rows and columns stay as they are.
pub fn move_cell_targets(doc: &mut Document, sheet: usize, to: &dyn Fn(usize, usize) -> Option<(usize, usize)>) {
    let Some(name) = doc.blocks.iter().filter_map(|b| if let Block::Table(t) = b { Some(t) } else { None }).nth(sheet).map(|t| t.name.clone()) else {
        return;
    };
    let mut table = None;
    for block in &mut doc.blocks {
        match block {
            Block::Table(_) => table = Some(table.map_or(0, |i| i + 1)),
            Block::Layer(l) => {
                let owner = match &l.target {
                    Some(target) => name.as_deref() == Some(target.as_str()),
                    None => table == Some(sheet),
                };
                if !owner {
                    continue;
                }
                for rule in &mut l.rules {
                    let Rule::Rule { target, .. } = rule else { continue };
                    let Some(mut p) = RefParts::parse(target.trim()) else { continue };
                    let Some((row, abs)) = p.row else { continue };
                    if let Some((r, c)) = to(row, p.col) {
                        p.col = c;
                        p.row = Some((r, abs));
                        *target = p.write();
                    }
                }
            }
            _ => {}
        }
    }
}

/// Whether an A1 reference (in a formula anywhere or a rule of the table's
/// layers) points into the first `rows` rows or `cols` columns of table
/// `sheet`.
pub fn refers_to_edge(doc: &Document, sheet: usize, rows: usize, cols: usize) -> bool {
    // Moving just those lines far away changes exactly what refers to them.
    let away = |n: usize| (0..n).map(|i| i + 1_000_000).collect::<Vec<_>>();
    let (rows, cols) = (away(rows), away(cols));
    let mut moved = doc.clone();
    update_document(&mut moved, sheet, &Moves { rows: &rows, cols: &cols });
    moved != *doc
}

/// How a formula names a table: bare if it is an identifier, else quoted
/// (`"My sheet"`).
pub fn table_ref(name: &str) -> String {
    let ident = name.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_')
        && name.chars().all(|c| c.is_alphanumeric() || c == '_');
    if ident { name.to_string() } else { format!("\"{name}\"") }
}

/// Replaces the table name `old` by `new` in a formula where it qualifies a
/// reference (`old!B2`, `old[…]`, also quoted).
fn rename_in_formula(text: &str, old: &str, new: &str) -> String {
    let mut toks = tokens(text);
    for i in 0..toks.len() {
        let qualifies = matches!(next(&toks, i), Some(Tok::Op('!' | '[')));
        let after = matches!(prev(&toks, i), Some(Tok::Op('.' | '!')));
        if toks[i].name() == Some(old) && qualifies && !after {
            toks[i] = named(new);
        }
    }
    join(&toks)
}

/// The same for the `{=…}` parts of a text.
fn rename_inline(text: &str, old: &str, new: &str) -> String {
    map_inline(text, |f| rename_in_formula(f, old, new))
}

/// Applies `f` to the `{=…}` parts of a text.
fn map_inline(text: &str, f: impl Fn(&str) -> String) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("{=") {
        let end = rest[start..].find('}').map_or(rest.len(), |e| start + e);
        out.push_str(&rest[..start]);
        out.push_str(&f(&rest[start..end]));
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

/// Renames column `old` of table `table` in a formula: bare names when the
/// formula belongs to that table (`here`), `table!old`, and names inside
/// `table[…]`; not data layer names (`.old`) or row labels (`Col[old]`).
fn rename_column_in_formula(src: &str, here: bool, table: Option<&str>, old: &str, new: &str) -> String {
    let mut toks = tokens(src);
    for i in 0..toks.len() {
        if toks[i].name() != Some(old) || matches!(toks[i], Tok::Quoted(_)) {
            continue;
        }
        if matches!(prev(&toks, i), Some(Tok::Op('.'))) || matches!(next(&toks, i), Some(Tok::Op('(' | '!'))) {
            continue;
        }
        let applies = match (qualifier(&toks, i), bracket_owner(&toks, i)) {
            (Some(q), _) => Some(q.as_str()) == table,
            (None, Some(owner)) => owner.as_deref().map_or(here, |o| Some(o) == table),
            (None, None) => here,
        };
        if applies {
            toks[i] = named(new);
        }
    }
    join(&toks)
}

/// Renames column `old` of table `sheet` to `new` wherever it is referred to
/// by name: formulas, `{=…}` in text, layer rules, charts and layouts.
/// The column header itself is changed by the caller.
pub fn rename_column(doc: &mut Document, sheet: usize, old: &str, new: &str) {
    let Some(t) = doc.blocks.iter().filter_map(|b| if let Block::Table(t) = b { Some(t) } else { None }).nth(sheet) else {
        return;
    };
    let name = t.name.clone();
    let f = |src: &str, here: bool| rename_column_in_formula(src, here, name.as_deref(), old, new);
    let is_this = |t: &str| Some(t.trim().trim_matches('"')) == name.as_deref();
    let mut table = None;
    for block in &mut doc.blocks {
        match block {
            Block::Table(t) => {
                let index = table.map_or(0, |i| i + 1);
                table = Some(index);
                for cell in t.cells_mut().filter(|c| c.trim_start().starts_with('=')) {
                    *cell = f(cell, index == sheet);
                }
            }
            Block::Text(lines) => {
                for line in lines {
                    *line = map_inline(line, |e| f(e, false));
                }
            }
            Block::Layer(l) => {
                let owner = match &l.target {
                    Some(target) => name.as_deref() == Some(target.as_str()),
                    None => table == Some(sheet),
                };
                if !owner {
                    continue;
                }
                for rule in &mut l.rules {
                    let Rule::Rule { target, .. } = rule else { continue };
                    if target.trim() == old {
                        *target = new.to_string();
                    } else if let Some(cond) = target.trim().strip_prefix("row[")
                        && cond.starts_with(old)
                        && cond[old.len()..].trim_start().starts_with(['=', '<', '>', '!'])
                    {
                        *target = format!("row[{new}{}", &cond[old.len()..]);
                    }
                }
            }
            Block::Chart { lines, .. } => {
                for line in lines {
                    let Some((key, items)) = line.split_once(':') else { continue };
                    if !matches!(key.trim(), "data" | "labels" | "x") {
                        continue;
                    }
                    let mut changed = false;
                    let renamed: Vec<String> = items
                        .split(',')
                        .map(|item| match crate::chart::split_ref(item) {
                            Some((t, col, sep)) if is_this(t) && col.trim() == old => {
                                changed = true;
                                format!("{}{sep}{new}", t.trim())
                            }
                            _ => item.trim().to_string(),
                        })
                        .collect();
                    if changed {
                        *line = format!("{key}: {}", renamed.join(", "));
                    }
                }
            }
            Block::Layout { lines, .. } => {
                for line in lines {
                    let Ok(mut e) = crate::layout::Element::parse(line) else { continue };
                    let before = e.clone();
                    match e.kind {
                        crate::layout::ElementKind::Table if is_this(&e.content) => {
                            for c in &mut e.columns {
                                if c == old {
                                    *c = new.to_string();
                                }
                            }
                        }
                        crate::layout::ElementKind::Value => e.content = f(&e.content, false),
                        _ => e.content = map_inline(&e.content, |x| f(x, false)),
                    }
                    if e != before {
                        *line = e.to_line();
                    }
                }
            }
            _ => {}
        }
    }
}

/// Renames footer row `old` of table `sheet` to `new` (the label itself is
/// changed by the caller) in formulas and `{=…}` in text:
/// `Table.old.Column`, and `old.Column` in the table's own formulas.
pub fn rename_footer(doc: &mut Document, sheet: usize, old: &str, new: &str) {
    let Some(t) = doc.blocks.iter().filter_map(|b| if let Block::Table(t) = b { Some(t) } else { None }).nth(sheet) else {
        return;
    };
    let name = t.name.clone();
    let f = |src: &str, here: bool| rename_footer_in_formula(src, here, name.as_deref(), old, new);
    let mut table = None;
    for block in &mut doc.blocks {
        match block {
            Block::Table(t) => {
                let index = table.map_or(0, |i| i + 1);
                table = Some(index);
                for cell in t.cells_mut().filter(|c| c.trim_start().starts_with('=')) {
                    *cell = f(cell, index == sheet);
                }
            }
            Block::Text(lines) => {
                for line in lines {
                    *line = map_inline(line, |e| f(e, false));
                }
            }
            _ => {}
        }
    }
}

/// `Col[old]` → `Col[new]` where `Col` is a column of the table (`here`,
/// or `table!Col[old]`).
fn rename_footer_in_formula(src: &str, here: bool, table: Option<&str>, old: &str, new: &str) -> String {
    let mut toks = tokens(src);
    for i in 0..toks.len() {
        if toks[i].name() != Some(old) {
            continue;
        }
        let (Some(open), Some(Tok::Op(']'))) = (prev_index(&toks, i), next(&toks, i)) else { continue };
        if toks[open] != Tok::Op('[') {
            continue;
        }
        let Some(col) = prev_index(&toks, open).filter(|&k| toks[k].name().is_some()) else { continue };
        let applies = match qualifier(&toks, col) {
            Some(q) => Some(q.as_str()) == table,
            None => here && toks[col].name() != table,
        };
        if applies {
            toks[i] = if matches!(toks[i], Tok::Quoted(_)) { Tok::Quoted(new.to_string()) } else { named(new) };
        }
    }
    join(&toks)
}

/// `ref.old` → `ref.new` for references to the table (`here`, or `table!…`).
fn rename_layer_in_formula(src: &str, here: bool, table: Option<&str>, old: &str, new: &str) -> String {
    let mut toks = tokens(src);
    for i in 0..toks.len() {
        if toks[i].name() != Some(old) {
            continue;
        }
        let Some(dot) = prev_index(&toks, i).filter(|&k| toks[k] == Tok::Op('.')) else { continue };
        let Some(start) = chain_start(&toks, dot) else { continue };
        let applies = match qualifier(&toks, start) {
            Some(q) => Some(q.as_str()) == table,
            None => here,
        };
        if applies {
            toks[i] = named(new);
        }
    }
    join(&toks)
}

/// Renames data layer `old` of table `sheet` to `new` in formulas and `{=…}`
/// in text (the layer itself is renamed by the caller).
pub fn rename_layer(doc: &mut Document, sheet: usize, old: &str, new: &str) {
    let Some(t) = doc.blocks.iter().filter_map(|b| if let Block::Table(t) = b { Some(t) } else { None }).nth(sheet) else {
        return;
    };
    let name = t.name.clone();
    let f = |src: &str, here: bool| rename_layer_in_formula(src, here, name.as_deref(), old, new);
    let mut table = None;
    for block in &mut doc.blocks {
        match block {
            Block::Table(t) => {
                let index = table.map_or(0, |i| i + 1);
                table = Some(index);
                for cell in t.cells_mut().filter(|c| c.trim_start().starts_with('=')) {
                    *cell = f(cell, index == sheet);
                }
            }
            Block::Text(lines) => {
                for line in lines {
                    *line = map_inline(line, |e| f(e, false));
                }
            }
            _ => {}
        }
    }
}

/// Slice or scenario `z` of table `sheet` is removed: references by letter
/// to the later ones move up one letter, those to `z` become `#REF`.
pub fn remove_layer(doc: &mut Document, sheet: usize, z: usize) {
    remap_layers(doc, sheet, &|lz| match lz {
        _ if lz < z => Some(lz),
        _ if lz == z => None,
        _ => Some(lz - 1),
    });
}

/// Slices / scenarios `a` and `b` of table `sheet` swap places: references
/// by letter follow them.
pub fn swap_layers(doc: &mut Document, sheet: usize, a: usize, b: usize) {
    remap_layers(doc, sheet, &|lz| Some(if lz == a { b } else if lz == b { a } else { lz }));
}

/// Rewrites the letters of references to table `sheet`'s z axis: `map`
/// gives the new letter (`None`: the level is gone, `#REF`).
fn remap_layers(doc: &mut Document, sheet: usize, map: &dyn Fn(usize) -> Option<usize>) {
    let Some(t) = doc.blocks.iter().filter_map(|b| if let Block::Table(t) = b { Some(t) } else { None }).nth(sheet) else {
        return;
    };
    let name = t.name.clone();
    let f = |src: &str, here: bool| {
        let long = expand_short_ranges(src);
        let mut toks = tokens(&long);
        let mut changed = false;
        for i in 0..toks.len() {
            let Tok::Name(n) = &toks[i] else { continue };
            let Some(mut p) = crate::engine::RefParts::parse(n) else { continue };
            let Some((lz, abs)) = p.layer else { continue };
            let applies = match qualifier(&toks, i) {
                Some(q) => Some(q.as_str()) == name.as_deref(),
                None => here,
            };
            if !applies || map(lz) == Some(lz) {
                continue;
            }
            toks[i] = match map(lz) {
                None => Tok::Raw("#REF".into()),
                Some(new) => {
                    p.layer = Some((new, abs));
                    Tok::Name(p.write())
                }
            };
            changed = true;
        }
        if changed { join(&toks) } else { src.to_string() }
    };
    let mut table = None;
    for block in &mut doc.blocks {
        match block {
            Block::Table(t) => {
                let index = table.map_or(0, |i| i + 1);
                table = Some(index);
                for cell in t.cells_mut().filter(|c| c.trim_start().starts_with('=')) {
                    *cell = f(cell, index == sheet);
                }
            }
            Block::Text(lines) => {
                for line in lines {
                    *line = map_inline(line, |e| f(e, false));
                }
            }
            _ => {}
        }
    }
}

/// `,` between the arguments of functions (the form before format version
/// 1) → `;`. Commas in brackets (`Table[Row, Column]`) and in texts stay.
pub fn semicolons(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut open: Vec<char> = Vec::new();
    let mut quoted = false;
    for c in src.chars() {
        match c {
            '"' => quoted = !quoted,
            '(' | '[' | '{' if !quoted => open.push(c),
            ')' | ']' | '}' if !quoted => {
                open.pop();
            }
            ',' if !quoted && open.last() == Some(&'(') => {
                out.push(';');
                continue;
            }
            _ => {}
        }
        out.push(c);
    }
    out
}

/// Applies [`semicolons`] to every formula of a document: cells, `{=…}`
/// in texts and layouts, value elements and the functions of its own.
pub fn legacy_separators(doc: &mut Document) {
    for block in &mut doc.blocks {
        match block {
            Block::Table(t) => {
                for cell in t.cells_mut().filter(|c| c.trim_start().starts_with('=') && c.contains(',')) {
                    *cell = semicolons(cell);
                }
            }
            Block::Text(lines) => {
                for line in lines.iter_mut().filter(|l| l.contains("{=")) {
                    *line = map_inline(line, semicolons);
                }
            }
            Block::Functions(lines) => {
                for line in lines.iter_mut().filter(|l| l.contains(',')) {
                    // The description after ` #` is text.
                    let (def, rest) = match line.find(" #") {
                        Some(i) => line.split_at(i),
                        None => (line.as_str(), ""),
                    };
                    *line = format!("{}{rest}", semicolons(def));
                }
            }
            Block::Layout { lines, .. } => {
                for line in lines.iter_mut().filter(|l| l.contains(',')) {
                    let Ok(mut e) = crate::layout::Element::parse(line) else { continue };
                    let content = match e.kind {
                        crate::layout::ElementKind::Value => semicolons(&e.content),
                        crate::layout::ElementKind::Text | crate::layout::ElementKind::Note | crate::layout::ElementKind::Heading => map_inline(&e.content, semicolons),
                        _ => continue,
                    };
                    if content != e.content {
                        e.content = content;
                        *line = e.to_line();
                    }
                }
            }
            _ => {}
        }
    }
}

/// Writes `Table.X` (the old form) as `Table!X` in all formulas and `{=…}`.
pub fn normalize_table_dots(doc: &mut Document) {
    let tables: Vec<String> = doc.blocks.iter().filter_map(|b| if let Block::Table(t) = b { t.name.clone() } else { None }).collect();
    if tables.is_empty() {
        return;
    }
    let f = |src: &str| {
        if !src.contains('.') {
            return src.to_string();
        }
        let mut toks = tokens(src);
        for i in 0..toks.len() {
            let Some(name) = toks[i].name() else { continue };
            if !tables.iter().any(|t| t == name) || matches!(prev(&toks, i), Some(Tok::Op('.' | '!'))) {
                continue;
            }
            if let Some(k) = next_index(&toks, i).filter(|&k| toks[k] == Tok::Op('.'))
                && next(&toks, k).is_some_and(|t| t.name().is_some())
            {
                toks[k] = Tok::Op('!');
            }
        }
        join(&toks)
    };
    for block in &mut doc.blocks {
        match block {
            Block::Table(t) => {
                for cell in t.cells_mut().filter(|c| c.trim_start().starts_with('=')) {
                    *cell = f(cell);
                }
            }
            Block::Text(lines) => {
                for line in lines {
                    *line = map_inline(line, f);
                }
            }
            Block::Layout { lines, .. } => {
                for line in lines {
                    let Ok(mut e) = crate::layout::Element::parse(line) else { continue };
                    let content = match e.kind {
                        crate::layout::ElementKind::Value => f(&e.content),
                        crate::layout::ElementKind::Table => continue,
                        _ => map_inline(&e.content, f),
                    };
                    if content != e.content {
                        e.content = content;
                        *line = e.to_line();
                    }
                }
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------- tokens

/// Formula text split so that joining it gives the text back exactly.
#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Name(String),
    /// `"…"` – a quoted name or a string.
    Quoted(String),
    /// Numbers, units, spaces and anything else kept as it is.
    Raw(String),
    Op(char),
}

impl Tok {
    /// The name of a name or quoted name.
    fn name(&self) -> Option<&str> {
        match self {
            Tok::Name(n) | Tok::Quoted(n) => Some(n),
            _ => None,
        }
    }
}

fn tokens(src: &str) -> Vec<Tok> {
    let chars: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let start = i;
        if c == '"' {
            i += 1;
            while i < chars.len() && !(chars[i] == '"' && chars.get(i + 1) != Some(&'"')) {
                i += if chars[i] == '"' { 2 } else { 1 };
            }
            let inner: String = chars[start + 1..i.min(chars.len())].iter().collect();
            i = (i + 1).min(chars.len());
            out.push(Tok::Quoted(inner.replace("\"\"", "\"")));
            // Keep the exact text when joining: an unterminated quote stays raw.
            if !chars[start + 1..].contains(&'"') {
                out.pop();
                out.push(Tok::Raw(chars[start..].iter().collect()));
            }
        } else if c == '{' && prev(&out, out.len()).is_some_and(|t| matches!(t, Tok::Raw(n) if n.starts_with(|c: char| c.is_ascii_digit()))) {
            // A unit after a number: `5{m/s}`.
            i = chars[i..].iter().position(|&c| c == '}').map_or(chars.len(), |p| i + p + 1);
            out.push(Tok::Raw(chars[start..i].iter().collect()));
        } else if c.is_ascii_digit() {
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            out.push(Tok::Raw(chars[start..i].iter().collect()));
        } else if c.is_alphabetic() || c == '_' || c == '$' {
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '$') {
                i += 1;
            }
            out.push(Tok::Name(chars[start..i].iter().collect()));
        } else if c.is_whitespace() {
            while i < chars.len() && chars[i].is_whitespace() {
                i += 1;
            }
            out.push(Tok::Raw(chars[start..i].iter().collect()));
        } else {
            i += 1;
            out.push(Tok::Op(c));
        }
    }
    out
}

/// A name as a token: bare if it is an identifier, else quoted.
fn named(name: &str) -> Tok {
    if table_ref(name) == name { Tok::Name(name.to_string()) } else { Tok::Quoted(name.to_string()) }
}

fn join(toks: &[Tok]) -> String {
    toks.iter()
        .map(|t| match t {
            Tok::Name(s) | Tok::Raw(s) => s.clone(),
            Tok::Quoted(s) => format!("\"{}\"", s.replace('"', "\"\"")),
            Tok::Op(c) => c.to_string(),
        })
        .collect()
}

fn is_space(t: &Tok) -> bool {
    matches!(t, Tok::Raw(s) if s.trim().is_empty())
}

fn prev_index(toks: &[Tok], i: usize) -> Option<usize> {
    (0..i).rev().find(|&k| !is_space(&toks[k]))
}

fn next_index(toks: &[Tok], i: usize) -> Option<usize> {
    (i + 1..toks.len()).find(|&k| !is_space(&toks[k]))
}

fn prev(toks: &[Tok], i: usize) -> Option<&Tok> {
    prev_index(toks, i).map(|k| &toks[k])
}

fn next(toks: &[Tok], i: usize) -> Option<&Tok> {
    next_index(toks, i).map(|k| &toks[k])
}

/// `Table!` right before the reference at `i` (also for the end of a range `T!B2:B4`).
fn qualifier(toks: &[Tok], i: usize) -> Option<String> {
    let mut k = prev_index(toks, i)?;
    // The end of a range: look at its start.
    if toks[k] == Tok::Op(':') {
        let start = prev_index(toks, k)?;
        return qualifier(toks, start);
    }
    if toks[k] != Tok::Op('!') {
        return None;
    }
    k = prev_index(toks, k)?;
    toks[k].name().map(str::to_string)
}

/// The innermost `[` around `i`: `Some(Some(name))` for `name[…]`,
/// `Some(None)` for a bare `[…]` (`[@Col]`), `None` outside brackets.
fn bracket_owner(toks: &[Tok], i: usize) -> Option<Option<String>> {
    let mut depth = 0;
    for k in (0..i).rev() {
        match toks[k] {
            Tok::Op(']') => depth += 1,
            Tok::Op('[') if depth == 0 => {
                return Some(prev_index(toks, k).and_then(|p| toks[p].name().map(str::to_string)));
            }
            Tok::Op('[') => depth -= 1,
            _ => {}
        }
    }
    None
}

/// The first token of the reference before the `.` at `dot`: `B2`, `Amount`
/// of `Amount[Total]`, or a quoted name.
fn chain_start(toks: &[Tok], dot: usize) -> Option<usize> {
    let mut k = prev_index(toks, dot)?;
    loop {
        if toks[k] == Tok::Op(']') {
            let mut depth = 0;
            loop {
                match toks[k] {
                    Tok::Op(']') => depth += 1,
                    Tok::Op('[') => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
                k = k.checked_sub(1)?;
            }
            k = prev_index(toks, k)?;
            continue;
        }
        toks[k].name()?;
        // `Amount.Plan.February`: go further back through the dots.
        match prev_index(toks, k) {
            Some(d) if toks[d] == Tok::Op('.') => k = prev_index(toks, d)?,
            _ => return Some(k),
        }
    }
}

/// Renames table `sheet` to `new` and updates everything that refers to it
/// by name: formulas, `{=…}` in text, charts, layouts and `@name` layers.
pub fn rename_table(doc: &mut Document, sheet: usize, new: &str) {
    let mut tables = doc.blocks.iter_mut().filter_map(|b| if let Block::Table(t) = b { Some(t) } else { None });
    let Some(t) = tables.nth(sheet) else { return };
    let old = t.name.replace(new.to_string());
    if t.heading_level.is_none() {
        t.heading_level = Some(2);
    }
    let Some(old) = old else { return };
    let is_old = |name: &str| name.trim().trim_matches('"') == old;
    for block in &mut doc.blocks {
        match block {
            Block::Table(t) => {
                for cell in t.cells_mut().filter(|c| c.trim_start().starts_with('=')) {
                    *cell = rename_in_formula(cell, &old, new);
                }
            }
            Block::Text(lines) => {
                for line in lines {
                    *line = rename_inline(line, &old, new);
                }
            }
            Block::Layer(l) if l.target.as_deref() == Some(old.as_str()) => l.target = Some(new.to_string()),
            Block::Chart { lines, .. } => {
                // `key: Table!Column, …` or `data: Table` (a tensor).
                for line in lines {
                    let Some((key, items)) = line.split_once(':') else { continue };
                    if !matches!(key.trim(), "data" | "labels" | "x") {
                        continue;
                    }
                    let renamed: Vec<String> = items
                        .split(',')
                        .map(|item| match crate::chart::split_ref(item) {
                            Some((t, col, sep)) if is_old(t) => format!("{new}{sep}{}", col.trim()),
                            None if is_old(item) => new.to_string(),
                            _ => item.trim().to_string(),
                        })
                        .collect();
                    let renamed = format!("{key}: {}", renamed.join(", "));
                    if renamed.split_whitespace().ne(line.split_whitespace()) {
                        *line = renamed;
                    }
                }
            }
            Block::Layout { lines, .. } => {
                for line in lines {
                    let Ok(mut e) = crate::layout::Element::parse(line) else { continue };
                    let content = match e.kind {
                        crate::layout::ElementKind::Table if e.content == old => new.to_string(),
                        crate::layout::ElementKind::Value => rename_in_formula(&e.content, &old, new),
                        _ => rename_inline(&e.content, &old, new),
                    };
                    if content != e.content {
                        e.content = content;
                        *line = e.to_line();
                    }
                }
            }
            _ => {}
        }
    }
}

/// The formula copied `dr` rows and `dc` columns further (filling, as in
/// Excel): relative A1 references move along, `$`-anchored parts stay.
/// Column names, `@` references and other tables' columns are unchanged.
/// `names`: the column names of the formula's table.
pub fn shift_formula(src: &str, dr: isize, dc: isize, names: &[String]) -> String {
    let long = expand_short_ranges(src);
    let out = shift_long(&long, dr, dc, names);
    if out == long { src.to_string() } else { out }
}

fn shift_long(src: &str, dr: isize, dc: isize, names: &[String]) -> String {
    let chars: Vec<char> = src.chars().collect();
    let next_char = |mut i: usize| {
        while i < chars.len() && chars[i] == ' ' {
            i += 1;
        }
        chars.get(i).copied()
    };
    let shift = |v: usize, d: isize| usize::try_from(v as isize + d).ok();
    let mut out = String::new();
    let mut i = 0;
    let mut brackets = 0;
    let mut last = ' ';
    let mut qualified = false;
    while i < chars.len() {
        let c = chars[i];
        if c == '"' {
            let start = i;
            i += 1;
            while i < chars.len() && chars[i] != '"' {
                i += 1;
            }
            i = (i + 1).min(chars.len());
            out.extend(&chars[start..i]);
            qualified = next_char(i) == Some('!');
            last = '"';
            continue;
        }
        if c == '{' {
            let end = chars[i..].iter().position(|&c| c == '}').map_or(chars.len(), |p| i + p + 1);
            out.extend(&chars[i..end]);
            i = end;
            continue;
        }
        if c.is_ascii_digit() {
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '.') {
                out.push(chars[i]);
                i += 1;
            }
            last = '0';
            continue;
        }
        if c.is_alphabetic() || c == '_' || c == '$' {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '$') {
                i += 1;
            }
            let ident: String = chars[start..i].iter().collect();
            let next = next_char(i);
            let skip = matches!(next, Some('(' | '!')) || matches!(last, '.' | '@') || brackets > 0 || names.contains(&ident);
            let moved = (!skip).then(|| RefParts::parse(&ident)).flatten().and_then(|mut p| {
                // A bare column letter of another table is left alone.
                if p.row.is_none() && qualified {
                    return Some(ident.clone());
                }
                if !p.col_abs {
                    p.col = shift(p.col, dc)?;
                }
                if let Some((r, false)) = p.row {
                    p.row = Some((shift(r, dr)?, false));
                }
                Some(p.write())
            });
            let replaced = match (skip, RefParts::parse(&ident).is_some(), moved) {
                (false, true, None) => "#REF".to_string(),
                (_, _, Some(m)) => m,
                _ => ident.clone(),
            };
            out.push_str(&replaced);
            // `Table!` qualifies the next reference, and the end of a range.
            qualified = next == Some('!') || (qualified && next == Some(':'));
            last = 'a';
            continue;
        }
        match c {
            '[' => brackets += 1,
            ']' => brackets -= 1,
            ':' => {}
            _ if !c.is_whitespace() => qualified = false,
            _ => {}
        }
        out.push(c);
        if !c.is_whitespace() {
            last = c;
        }
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names() -> Vec<String> {
        ["Item", "Amount", "ID"].iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn formulas_follow_moved_rows_and_columns() {
        // Rows 0 and 1 swapped, columns A and B swapped.
        let moves = Moves { rows: &[1, 0, 2], cols: &[1, 0, 2] };
        let rw = |src: &str, here: bool| rewrite_formula(src, here, Some("Expenses"), &names(), &moves);
        assert_eq!(rw("=B1/sum(B)", true), "=A2/sum(A)");
        assert_eq!(rw("=sum(A1:A2) + Amount * 2", true), "=sum(B2:B1) + Amount * 2");
        assert_eq!(rw("=B1 & \"B1\" + 5{m} + 1e3", true), "=A2 & \"B1\" + 5{m} + 1e3");
        // Other tables: only qualified references, ranges included.
        assert_eq!(rw("=Expenses!B1 + Expenses!A1:A2 + B1", false), "=Expenses!A2 + Expenses!B2:B1 + B1");
        assert_eq!(rw("=Other!B1 + Expenses.Amount + Expenses[1, 2]", false), "=Other!B1 + Expenses.Amount + Expenses[1, 2]");
        // Column names and letters beyond the table are no references.
        assert_eq!(rw("=ID + Item + ZZ", true), "=ID + Item + ZZ");
    }

    #[test]
    fn layer_targets_follow() {
        let moves = Moves { rows: &[2, 0, 1], cols: &[0, 1] };
        let n = names();
        assert_eq!(rewrite_target("B1", &n, &moves).as_deref(), Some("B3"));
        assert_eq!(rewrite_target("1", &n, &moves).as_deref(), Some("3"));
        assert_eq!(rewrite_target("Amount", &n, &moves).as_deref(), Some("Amount"));
        assert_eq!(rewrite_target("row[Status=open]", &n, &moves).as_deref(), Some("row[Status=open]"));
        // Deleted lines: the rule goes, a range shrinks.
        let gone = Moves { rows: &[0, GONE, 1], cols: &[0, 1] };
        assert_eq!(rewrite_target("B2", &n, &gone), None);
        assert_eq!(rewrite_target("2", &n, &gone), None);
        assert_eq!(rewrite_target("B1:B3", &n, &gone).as_deref(), Some("B1:B2"));
        assert_eq!(rewrite_target("B2:C2", &n, &gone), None);
    }

    #[test]
    fn renaming_a_table_updates_its_uses() {
        let src = "## Costs :: matrix[*, 2]\n\n| A | B |\n|---|---|\n| 1 | =Costs!A1 + Costs.A + CostCenter.A |\n\n\
                   Total: {=sum(Costs.A)}. Costs. \"Costs.A\"\n\n\
                   ```layer format @Costs\nA: bold\n```\n\n\
                   ```chart K\ntype: bar\ndata: Costs.A, Costs\n```\n\n\
                   ```layout L\n1-6 / 1-5 table Costs\n1-6 / 6-6 value =sum(Costs.A)\n```\n";
        let mut doc = crate::parse(src).unwrap();
        rename_table(&mut doc, 0, "Expenses");
        let out = crate::write(&doc);
        assert!(out.contains("## Expenses :: matrix"), "{out}");
        // `Costs.A` is read as the old form of `Costs!A`.
        assert!(out.contains("=Expenses!A1 + Expenses!A + CostCenter.A"), "{out}");
        assert!(out.contains("Total: {=sum(Expenses!A)}. Costs. \"Costs.A\""), "{out}");
        assert!(out.contains("```layer format @Expenses"), "{out}");
        assert!(out.contains("data: Expenses.A, Expenses"), "{out}");
        assert!(out.contains("table    Expenses"), "{out}");
        assert!(out.contains("value    =sum(Expenses!A)"), "{out}");
    }

    #[test]
    fn short_ranges() {
        assert_eq!(expand_short_ranges("=sum(B2a:c) + B2:4c + B2:D + \"x:4\""), "=sum(B2a:B2c) + B2c:B4c + B2:D2 + \"x:4\"");
        let moves = Moves { rows: &[0, 2, 1], cols: &[0, 1] };
        assert_eq!(rewrite_formula("=sum(B1:3)", true, None, &[], &moves), "=sum(B1:B2)");
        assert_eq!(rewrite_formula("=sum(B1a:c)", true, None, &[], &moves), "=sum(B1a:c)");
    }

    #[test]
    fn renaming_footers_and_layers() {
        let f = |src: &str, here: bool| rename_footer_in_formula(src, here, Some("Costs"), "Total", "Total");
        assert_eq!(f("=Amount[Total] + Costs!Amount[Total] + Other!Amount[Total] + Costs[Total, B]", true), "=Amount[Total] + Costs!Amount[Total] + Other!Amount[Total] + Costs[Total, B]");
        assert_eq!(f("=Amount[Total]", false), "=Amount[Total]");
        let g = |src: &str, here: bool| rename_layer_in_formula(src, here, Some("Costs"), "Feb", "February");
        assert_eq!(g("=B2.Feb + Amount[Total].Feb + Costs!B2.Feb + Other!B2.Feb + Feb", true), "=B2.February + Amount[Total].February + Costs!B2.February + Other!B2.Feb + Feb");
        assert_eq!(g("=B2.Feb + Costs!B2.Feb", false), "=B2.Feb + Costs!B2.February");
    }

    #[test]
    fn table_names_with_spaces() {
        assert_eq!(table_ref("My sheet"), "\"My sheet\"");
        assert_eq!(rename_in_formula("=Costs!A1 + Costs!A + A1.Costs", "Costs", "Meine Costs"), "=\"Meine Costs\"!A1 + \"Meine Costs\"!A + A1.Costs");
        assert_eq!(rename_in_formula("=\"Meine Costs\"[1, 2] & \"Meine Costs\"", "Meine Costs", "Costs"), "=Costs[1, 2] & \"Meine Costs\"");
        let moves = Moves { rows: &[1, 0], cols: &[0] };
        assert_eq!(rewrite_formula("=\"My sheet\"!A1", false, Some("My sheet"), &[], &moves), "=\"My sheet\"!A2");
    }

    #[test]
    fn renaming_a_column_updates_its_uses() {
        let f = |src: &str, here: bool| rename_column_in_formula(src, here, Some("Costs"), "Amount", "Total");
        assert_eq!(f("=Amount * 2 + sum(Amount)", true), "=Total * 2 + sum(Total)");
        assert_eq!(f("=Amount", false), "=Amount", "another table's own column");
        assert_eq!(f("=Costs!Amount + Other!Amount + Costs!B1:Amount", false), "=Costs!Total + Other!Amount + Costs!B1:Total");
        assert_eq!(f("=Costs[Rent, Amount] + Other[Rent, Amount]", false), "=Costs[Rent, Total] + Other[Rent, Amount]");
        assert_eq!(f("=\"Costs\"!Amount & \"Amount\" + betrag(1)", false), "=\"Costs\"!Total & \"Amount\" + betrag(1)");
        // Layer names and row labels are no columns.
        assert_eq!(f("=Amount.February + B2.Amount + Amount[Amount] + Amount[-1]", true), "=Total.February + B2.Amount + Total[Amount] + Total[-1]");
        assert_eq!(f("=@Amount + [@Amount] + Costs[@Amount]", true), "=@Total + [@Total] + Costs[@Total]");

        let src = "## Costs :: matrix[*, 2]\n\n| Item | Amount |\n|---|---|\n| a | 1 |\n\n\
                   ```layer format\nAmount: bold\nrow[Amount>0]: fg red\n```\n\n\
                   ```chart K\ntype: bar\ndata: Costs.Amount\nlabels: Costs.Item\n```\n\n\
                   ```layout L\n1-6 / 1-5 table Costs   columns: Item, Amount\n```\n";
        let mut doc = crate::parse(src).unwrap();
        rename_column(&mut doc, 0, "Amount", "Total");
        let out = crate::write(&doc);
        assert!(out.contains("Total: bold") && out.contains("row[Total>0]: fg red"), "{out}");
        assert!(out.contains("data: Costs.Total") && out.contains("labels: Costs.Item"), "{out}");
        assert!(out.contains("columns: Item, Total"), "{out}");
    }

    #[test]
    fn filling_shifts_relative_references() {
        let names: Vec<String> = ["Amount", "ID"].iter().map(|s| s.to_string()).collect();
        let f = |src: &str, dr, dc| shift_formula(src, dr, dc, &names);
        assert_eq!(f("=B1/sum(B$1:B$3)", 1, 0), "=B2/sum(B$1:B$3)");
        assert_eq!(f("=$A1 + A$1 + $A$1 + B", 2, 1), "=$A3 + B$1 + $A$1 + C");
        assert_eq!(f("=Amount * 2 + @Amount + ID + Costs.Amount", 5, 3), "=Amount * 2 + @Amount + ID + Costs.Amount");
        assert_eq!(f("=Costs!B2 + \"My sheet\"!B2:B3 + Costs!B", 1, 0), "=Costs!B3 + \"My sheet\"!B3:B4 + Costs!B");
        assert_eq!(f("=A1 & \"A1\" + 5{m} + sum(A1)", 1, 0), "=A2 & \"A1\" + 5{m} + sum(A2)");
        assert_eq!(f("=A1", -1, 0), "=#REF");
        // References into data layers move too; short ranges keep their form unless moved.
        assert_eq!(f("=B2.February + B2[-1] + B2c", 1, 0), "=B3.February + B3[-1] + B3c");
        assert_eq!(f("=sum(B2:4c)", 1, 0), "=sum(B3c:B5c)");
        assert_eq!(f("=sum(B2a:c) + sum($B$2:4)", 0, 0), "=sum(B2a:c) + sum($B$2:4)");
        assert_eq!(f("=sum($B$2:$4)", 1, 0), "=sum($B$2:$4)");
    }
}
