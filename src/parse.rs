//! Line-based parser for `.hcmd` documents.

use crate::model::*;

#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    pub line: usize,
    pub message: String,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for ParseError {}

pub fn parse(src: &str) -> Result<Document, ParseError> {
    let lines: Vec<&str> = src.lines().collect();
    let mut doc = Document::default();
    let mut text: Vec<String> = Vec::new();
    let mut i = 0;

    let flush = |text: &mut Vec<String>, doc: &mut Document| {
        // Trailing blank lines belong to the separator between blocks, not the text.
        while text.last().is_some_and(|l| l.trim().is_empty()) {
            text.pop();
        }
        while text.first().is_some_and(|l| l.trim().is_empty()) {
            text.remove(0);
        }
        if !text.is_empty() {
            doc.blocks.push(Block::Text(std::mem::take(text)));
        }
    };

    while i < lines.len() {
        let line = lines[i];

        if let Some(fence) = fence_marker(line) {
            flush(&mut text, &mut doc);
            let info = line.trim_start()[fence.len()..].trim().to_string();
            let start = i;
            i += 1;
            let mut body = Vec::new();
            while i < lines.len() && !lines[i].trim_start().starts_with(fence) {
                body.push(lines[i].to_string());
                i += 1;
            }
            if i == lines.len() {
                return Err(ParseError { line: start + 1, message: "unclosed code fence".into() });
            }
            i += 1;
            // A slice or scenario belongs to the last table before it.
            if let Some((name, kind)) = data_layer_info(&info)
                && let Some(t) = doc.blocks.iter_mut().rev().find_map(|b| if let Block::Table(t) = b { Some(t) } else { None })
                && let Ok(rows) = parse_layer_rows(&body, &t.columns, t.rows.len())
                && !t.layers.iter().any(|l| l.name == name)
            {
                t.layers.push(DataLayer { name, kind, rows });
                continue;
            }
            // A footer block belongs to the table right before it.
            if is_footer_info(&info)
                && let Some(Block::Table(t)) = doc.blocks.last_mut()
                && t.footer.is_empty()
                && let Ok(rows) = parse_footer(&body, &t.columns)
            {
                t.footer = rows;
                continue;
            }
            doc.blocks.push(fenced_block(info, body, start + 1));
            continue;
        }

        if let Some((level, name, shape)) = table_heading(line) {
            let mut j = i + 1;
            while j < lines.len() && lines[j].trim().is_empty() {
                j += 1;
            }
            if j + 1 < lines.len() && is_table_row(lines[j]) && is_separator(lines[j + 1]) {
                flush(&mut text, &mut doc);
                let (shape, fill, layer_name, compact) = parse_shape_and_layer(&shape).map_err(|m| ParseError { line: i + 1, message: m })?;
                let (mut table, next) = parse_table(&lines, j);
                table.heading_level = Some(level);
                table.name = Some(name);
                table.shape = shape;
                table.layer_name = layer_name;
                table.fill = fill;
                table.compact = compact;
                doc.blocks.push(Block::Table(table));
                i = next;
                continue;
            }
        }

        if is_table_row(line) && i + 1 < lines.len() && is_separator(lines[i + 1]) {
            flush(&mut text, &mut doc);
            let (table, next) = parse_table(&lines, i);
            doc.blocks.push(Block::Table(table));
            i = next;
            continue;
        }

        text.push(line.to_string());
        i += 1;
    }
    flush(&mut text, &mut doc);
    crate::refs::normalize_table_dots(&mut doc);
    Ok(doc)
}

fn fence_marker(line: &str) -> Option<&'static str> {
    let t = line.trim_start();
    if t.starts_with("````") {
        Some("````")
    } else if t.starts_with("```") {
        Some("```")
    } else {
        None
    }
}

fn fenced_block(info: String, body: Vec<String>, line: usize) -> Block {
    // A slice or scenario that did not fit its table stays as it is (check reports it).
    if data_layer_info(&info).is_some() || is_data_layer_like(&info) {
        return Block::Code { info, body };
    }
    if info == "types" {
        return Block::Types(body);
    }
    if info == "functions" {
        return Block::Functions(body);
    }
    if let Some(rest) = info.strip_prefix("layer ") {
        // `layer name @Table`; the table name may contain spaces.
        let (head, target) = match rest.split_once('@') {
            Some((h, t)) => (h, Some(t.trim().to_string()).filter(|t| !t.is_empty())),
            None => (rest, None),
        };
        let name = head.split_whitespace().next().unwrap_or_default().to_string();
        let rules = body.into_iter().map(parse_rule).collect();
        return Block::Layer(Layer { line: SrcLine(line), name, target, rules });
    }
    if let Some(name) = info.strip_prefix("layout ") {
        return Block::Layout { name: name.trim().to_string(), lines: body };
    }
    if let Some(name) = info.strip_prefix("chart ") {
        return Block::Chart { name: name.trim().to_string(), lines: body };
    }
    Block::Code { info, body }
}

/// `footer` or `footer <Table>` (the name is for readers; the block belongs
/// to the table right before it).
pub fn is_footer_info(info: &str) -> bool {
    info == "footer" || info.starts_with("footer ")
}

/// The rows of a footer block, a Markdown table: the first column holds the
/// labels, the others are columns of the table (by name or letter, in any
/// order); cells are written as in the data.
pub fn parse_footer(lines: &[String], columns: &[Column]) -> Result<Vec<FooterRow>, String> {
    let lines: Vec<&str> = lines.iter().map(|l| l.as_str()).filter(|l| !l.trim().is_empty()).collect();
    let [header, sep, body @ ..] = lines.as_slice() else { return Err("expected a table: | | Column | … |".into()) };
    if !is_table_row(header) || !is_separator(sep) {
        return Err("expected a table: | | Column | … |".into());
    }
    // A type after the name (`Amount:Euro`) is allowed but not needed.
    let targets = column_targets(&split_row(header)[1..], columns)?;
    let mut rows = Vec::new();
    for line in body {
        if !is_table_row(line) {
            return Err(format!("expected a table row: '{}'", line.trim()));
        }
        let cells = split_row(line);
        let label = cells.first().map(|l| l.trim().to_string()).unwrap_or_default();
        if label.is_empty() {
            return Err(format!("missing label in '{}'", line.trim()));
        }
        let mut row = vec![String::new(); columns.len()];
        for (raw, &c) in cells.iter().skip(1).zip(&targets) {
            row[c] = raw.trim().to_string();
        }
        rows.push(FooterRow { label, cells: row });
    }
    Ok(rows)
}

/// Whether `before` is a cell like `A1` and `after` starts with a second cell
/// and a colon (`B2:`), so the colon between them belongs to a range target.
fn is_range_start(before: &str, after: &str) -> bool {
    let cell = |s: &str| {
        let letters = s.chars().take_while(|c| c.is_ascii_alphabetic()).count();
        let digits = s[letters..].chars().take_while(|c| c.is_ascii_digit()).count();
        letters > 0 && digits > 0 && letters + digits == s.len()
    };
    cell(before.trim()) && after.split_once(':').is_some_and(|(end, _)| cell(end))
}

fn parse_rule(line: String) -> Rule {
    let t = line.trim();
    if t.is_empty() || t.starts_with('#') {
        return Rule::Raw(line);
    }
    // The target may contain brackets with ':' inside (e.g. `row[t=12:00]`), so
    // split at the first ':' outside brackets.
    let mut depth = 0i32;
    for (idx, ch) in t.char_indices() {
        match ch {
            '[' => depth += 1,
            ']' => depth -= 1,
            ':' if depth == 0 => {
                // `A1:B2: bg green`: this colon joins the two ends of a range.
                if is_range_start(&t[..idx], &t[idx + 1..]) {
                    continue;
                }
                return Rule::Rule {
                    target: t[..idx].trim().to_string(),
                    body: t[idx + 1..].trim().to_string(),
                };
            }
            _ => {}
        }
    }
    Rule::Raw(line)
}

/// `## Name :: shape` → (level, name, shape)
fn table_heading(line: &str) -> Option<(usize, String, String)> {
    let level = line.chars().take_while(|&c| c == '#').count();
    if level == 0 || level > 6 {
        return None;
    }
    let rest = line[level..].strip_prefix(' ')?;
    let (name, shape) = rest.split_once("::")?;
    Some((level, name.trim().to_string(), shape.trim().to_string()))
}

/// `matrix[*, 3] fill [January]`: the shape, `fill` / `nofill` and the name
/// of data layer a.
fn parse_shape_and_layer(s: &str) -> Result<(Shape, Option<bool>, Option<String>, bool), String> {
    // `aligned` (padded columns) sits between the fill flag and the layer name;
    // `compact`, the default, is accepted too.
    let split_compact = |s: &str| -> (String, bool) {
        let s = s.trim();
        for (word, compact) in [("aligned", false), ("compact", true)] {
            if let Some(rest) = s.strip_suffix(word)
                && rest.ends_with(char::is_whitespace)
            {
                return (rest.trim().to_string(), compact);
            }
        }
        (s.to_string(), true)
    };
    if let Some((shape, rest)) = s.trim_end().rsplit_once(" [")
        && let Some(name) = rest.strip_suffix(']').map(str::trim)
        && valid_layer_name(name)
    {
        let (shape, compact) = split_compact(shape);
        if let Ok((shape, fill)) = parse_shape_and_fill(&shape) {
            return Ok((shape, fill, Some(name.to_string()), compact));
        }
    }
    let (rest, compact) = split_compact(s);
    let (shape, fill) = parse_shape_and_fill(&rest)?;
    Ok((shape, fill, None, compact))
}

/// `playground nofill`: the shape and the optional fill flag.
fn parse_shape_and_fill(s: &str) -> Result<(Shape, Option<bool>), String> {
    let s = s.trim();
    for (word, on) in [("nofill", false), ("fill", true)] {
        if let Some(shape) = s.strip_suffix(word)
            && shape.ends_with(char::is_whitespace)
        {
            return Ok((parse_shape(shape.trim())?, Some(on)));
        }
    }
    Ok((parse_shape(s)?, None))
}

/// Layer names: not empty, no `,` `:` `.` `"` `[` `]`, and nothing that
/// reads like a reference or a layer letter (`B2`, `b`).
pub fn valid_layer_name(name: &str) -> bool {
    !name.is_empty()
        && !name.contains([',', ':', '.', '"', '[', ']', '`', '@', '!'])
        && name.trim() == name
        && name != "*"
        && !name.chars().all(|c| c.is_ascii_lowercase())
        && !name.chars().all(|c| c.is_ascii_digit())
        && crate::engine::RefParts::parse(name).is_none()
}

/// `slice February`, `scenario Optimistisch` → (name, kind).
pub fn data_layer_info(info: &str) -> Option<(String, LayerKind)> {
    let (name, kind) = if let Some(name) = info.strip_prefix("slice ") {
        (name, LayerKind::Slice)
    } else {
        (info.strip_prefix("scenario ")?, LayerKind::Scenario)
    };
    let name = name.trim();
    valid_layer_name(name).then(|| (name.to_string(), kind))
}

/// A block that is meant as a slice or scenario (reported if it does not fit).
pub fn is_data_layer_like(info: &str) -> bool {
    info.starts_with("slice ") || info.starts_with("scenario ")
}

/// The cells of a data layer: a table with (some of) the columns, rows in
/// the order of the table's rows; empty cells take the table's.
pub fn parse_layer_rows(lines: &[String], columns: &[Column], rows: usize) -> Result<Vec<Vec<String>>, String> {
    let lines: Vec<&str> = lines.iter().map(|l| l.as_str()).filter(|l| !l.trim().is_empty()).collect();
    let [header, sep, body @ ..] = lines.as_slice() else { return Err("expected a table: | Column | … |".into()) };
    if !is_table_row(header) || !is_separator(sep) {
        return Err("expected a table: | Column | … |".into());
    }
    let targets = column_targets(&split_row(header), columns)?;
    if body.len() > rows {
        return Err(format!("{} rows, the table has {rows}", body.len()));
    }
    body.iter()
        .map(|line| {
            if !is_table_row(line) {
                return Err(format!("expected a table row: '{}'", line.trim()));
            }
            let mut row = vec![String::new(); columns.len()];
            for (raw, &c) in split_row(line).iter().zip(&targets) {
                row[c] = raw.trim().to_string();
            }
            Ok(row)
        })
        .collect()
}

/// Header cells → column indexes (by name, `Name:Type` or letter).
fn column_targets(header: &[String], columns: &[Column]) -> Result<Vec<usize>, String> {
    header
        .iter()
        .map(|h| {
            let name = h.rsplit_once(':').map_or(h.as_str(), |(n, _)| n).trim();
            columns
                .iter()
                .position(|c| c.name == name || c.name == h.trim())
                .or_else(|| crate::engine::letters_to_col(name).filter(|&c| c < columns.len()))
                .ok_or_else(|| format!("unknown column '{name}'"))
        })
        .collect()
}

pub fn parse_shape(s: &str) -> Result<Shape, String> {
    let (kind, args) = match s.split_once('[') {
        Some((k, rest)) => {
            let inner = rest.strip_suffix(']').ok_or_else(|| format!("missing ']' in shape '{s}'"))?;
            let args: Vec<String> = inner.split(',').map(|a| a.trim().to_string()).filter(|a| !a.is_empty()).collect();
            (k.trim(), args)
        }
        None => (s.trim(), Vec::new()),
    };
    match kind {
        "vector" => Ok(Shape::Vector),
        "playground" => Ok(Shape::Playground),
        "matrix" => {
            if !(args.is_empty() || args.len() == 2) {
                return Err(format!("matrix needs 0 or 2 dimensions, got {}", args.len()));
            }
            Ok(Shape::Matrix(args.iter().map(|a| parse_dim(a)).collect()))
        }
        "tensor" => {
            if args.is_empty() {
                return Err("tensor needs at least one named axis".into());
            }
            Ok(Shape::Tensor(args))
        }
        other => Err(format!("unknown shape '{other}'")),
    }
}

fn parse_dim(s: &str) -> Dim {
    if s == "*" {
        Dim::Any
    } else if let Ok(n) = s.parse() {
        Dim::Fixed(n)
    } else {
        Dim::Named(s.to_string())
    }
}

fn is_table_row(line: &str) -> bool {
    line.trim_start().starts_with('|')
}

fn is_separator(line: &str) -> bool {
    let cells = split_row(line);
    !cells.is_empty()
        && cells.iter().all(|c| {
            let c = c.trim();
            let inner = c.trim_start_matches(':').trim_end_matches(':');
            !inner.is_empty() && inner.chars().all(|ch| ch == '-')
        })
}

/// Splits `| a | b \| c |` into `["a", "b | c"]`.
pub fn split_row(line: &str) -> Vec<String> {
    let t = line.trim();
    let t = t.strip_prefix('|').unwrap_or(t);
    let mut cells = Vec::new();
    let mut cur = String::new();
    let mut chars = t.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&'|') => {
                cur.push('|');
                chars.next();
            }
            '|' => cells.push(std::mem::take(&mut cur).trim().to_string()),
            _ => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        cells.push(cur.trim().to_string());
    }
    cells
}

fn parse_align(sep: &str) -> Align {
    let s = sep.trim();
    match (s.starts_with(':'), s.ends_with(':')) {
        (true, true) => Align::Center,
        (true, false) => Align::Left,
        (false, true) => Align::Right,
        (false, false) => Align::None,
    }
}

fn parse_table(lines: &[&str], start: usize) -> (Table, usize) {
    let header = split_row(lines[start]);
    let seps = split_row(lines[start + 1]);
    let columns: Vec<Column> = header
        .iter()
        .enumerate()
        .map(|(idx, h)| {
            let (name, ty) = match h.rsplit_once(':') {
                Some((n, t)) if !t.trim().is_empty() && !n.trim().is_empty() => {
                    (n.trim().to_string(), Some(t.trim().to_string()))
                }
                _ => (h.clone(), None),
            };
            let align = seps.get(idx).map(|s| parse_align(s)).unwrap_or_default();
            Column { name, ty, align }
        })
        .collect();

    let mut rows = Vec::new();
    let mut i = start + 2;
    while i < lines.len() && is_table_row(lines[i]) {
        let mut cells = split_row(lines[i]);
        cells.resize(columns.len(), String::new());
        rows.push(cells);
        i += 1;
    }
    let table = Table { compact: true, line: SrcLine(start + 1), heading_level: None, name: None, shape: Shape::Matrix(Vec::new()), columns, rows, footer: Vec::new(), layer_name: None, layers: Vec::new(), fill: None };
    (table, i)
}
