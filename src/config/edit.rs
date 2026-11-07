//! Reading and changing the data at a path, with minimal changes to the
//! file (CONFIG-SPEC §7.2), and turning typed input into TOML values.

use super::path::{Path, Segment};
use super::schema::{FieldType, Schema};
use super::validate::{lookup, value_text, Node, Validator};
use super::view::collect_tables;
use crate::types::{parse_bool, Base, Type};
use crate::value::{civil_from_days, parse_date, parse_number};
use toml_edit::{Array, DocumentMut, Item, Table, TableLike, Value};

/// The node at `path`, if it exists.
pub fn get<'a>(root: &'a Table, path: &Path) -> Option<Node<'a>> {
    let mut cur = Node::Table(root);
    for seg in &path.0 {
        cur = match (seg, cur) {
            (Segment::Key(k), n) => Node::of(n.table_like()?.get(k)?)?,
            (Segment::Index(i), n) => *n.elements()?.get(*i)?,
        };
    }
    Some(cur)
}

/// The value at `path`, if it is a plain value.
pub fn value_at<'a>(root: &'a Table, path: &Path) -> Option<&'a Value> {
    get(root, path)?.value()
}

/// The table at the path, creating missing tables on the way. New tables use
/// dotted keys when their siblings do (`whitelist.both = …`), otherwise a
/// `[header]`.
fn table_mut<'a>(root: &'a mut Table, segs: &[Segment]) -> Result<&'a mut dyn TableLike, String> {
    let mut t: &mut dyn TableLike = root;
    let mut i = 0;
    while i < segs.len() {
        let Segment::Key(k) = &segs[i] else { return Err("index without a list in path".into()) };
        if !t.contains_key(k) {
            let dotted = t.iter().any(|(_, it)| it.as_table_like().is_some_and(|x| x.is_dotted()));
            let mut nt = Table::new();
            nt.set_implicit(true);
            nt.set_dotted(dotted);
            t.insert(k, Item::Table(nt));
        }
        let item = t.get_mut(k).expect("just ensured");
        if let Some(Segment::Index(n)) = segs.get(i + 1) {
            t = item.as_array_of_tables_mut().and_then(|a| a.get_mut(*n)).ok_or_else(|| format!("no element [{n}] in {k}"))?;
            i += 2;
        } else {
            t = item.as_table_like_mut().ok_or_else(|| format!("{k} is not a table"))?;
            i += 1;
        }
    }
    Ok(t)
}

/// Puts `new` in place of `slot`, keeping spacing and a trailing comment.
fn replace(slot: &mut Value, mut new: Value) {
    *new.decor_mut() = slot.decor().clone();
    *slot = new;
}

/// Sets the value at `path` (a key or a list element), creating the key if needed.
pub fn set(doc: &mut DocumentMut, path: &Path, value: Value) -> Result<(), String> {
    let segs = &path.0;
    match segs.as_slice() {
        [parent @ .., Segment::Key(list), Segment::Index(i)] => {
            let t = table_mut(doc.as_table_mut(), parent)?;
            let arr = t.get_mut(list).and_then(Item::as_array_mut).ok_or_else(|| format!("{list} is not a list"))?;
            let slot = arr.get_mut(*i).ok_or_else(|| format!("no element [{i}]"))?;
            replace(slot, value);
            Ok(())
        }
        [parent @ .., Segment::Key(k)] => {
            let t = table_mut(doc.as_table_mut(), parent)?;
            match t.get_mut(k) {
                Some(Item::Value(slot)) => replace(slot, value),
                Some(_) => return Err(format!("{k} is a table")),
                None => {
                    t.insert(k, Item::Value(value));
                }
            }
            Ok(())
        }
        _ => Err("cannot set the root".into()),
    }
}

/// Removes the key or list element at `path`. Returns whether something was removed.
pub fn remove(doc: &mut DocumentMut, path: &Path) -> bool {
    let segs = &path.0;
    let Ok(parent) = (match segs.as_slice() {
        [parent @ .., Segment::Key(_), Segment::Index(_)] | [parent @ .., Segment::Key(_)] => {
            // Only existing tables: removing must not create anything.
            if get(doc.as_table(), &Path(parent.to_vec())).is_none() {
                return false;
            }
            table_mut(doc.as_table_mut(), parent)
        }
        _ => return false,
    }) else {
        return false;
    };
    match segs.as_slice() {
        [.., Segment::Key(list), Segment::Index(i)] => match parent.get_mut(list).and_then(Item::as_array_mut) {
            Some(arr) if *i < arr.len() => {
                let removed = arr.remove(*i);
                // The new first element takes the spacing of the old one.
                if *i == 0 && let Some(first) = arr.get_mut(0) {
                    let prefix = removed.decor().prefix().and_then(|p| p.as_str()).unwrap_or("").to_string();
                    first.decor_mut().set_prefix(prefix);
                }
                true
            }
            _ => false,
        },
        [.., Segment::Key(k)] => parent.remove(k).is_some(),
        _ => false,
    }
}

/// Appends `value` to the list at `path`, creating the list if needed.
pub fn push(doc: &mut DocumentMut, path: &Path, value: Value) -> Result<(), String> {
    let [parent @ .., Segment::Key(k)] = path.0.as_slice() else { return Err("not a list".into()) };
    let t = table_mut(doc.as_table_mut(), parent)?;
    if !t.contains_key(k) {
        t.insert(k, Item::Value(Value::Array(Array::new())));
    }
    let arr = t.get_mut(k).and_then(Item::as_array_mut).ok_or_else(|| format!("{k} is not a list"))?;
    // Keep the style of existing elements (`["a", "b"]`); the first one gets no leading space.
    let spaced = !arr.is_empty();
    arr.push(value);
    if let Some(last) = arr.get_mut(arr.len() - 1) {
        last.decor_mut().set_prefix(if spaced { " " } else { "" });
    }
    Ok(())
}

// ── Map entries (§7.2) ──────────────────────────────────────────────────────

/// A new map entry: for a struct, only its required fields, empty; defaults
/// stay with the program that reads the file.
pub fn new_entry(schema: &Schema, value: &FieldType) -> Item {
    let empty = |ty: &FieldType| -> Option<Value> {
        match ty {
            FieldType::Scalar { ty, .. } => match ty.base {
                Base::Text | Base::Duration | Base::Ref | Base::Enum(_) | Base::Auto => Some(Value::from("")),
                _ => None,
            },
            FieldType::Ref(_) => Some(Value::from("")),
            FieldType::List { .. } => Some(Value::Array(Array::new())),
            _ => None,
        }
    };
    match value {
        FieldType::Struct(name) => {
            let mut t = Table::new();
            if let Some(st) = schema.struct_def(name) {
                for f in st.fields.iter().filter(|f| f.required() && !f.readonly) {
                    if let Some(v) = empty(&f.ty) {
                        t.insert(&f.name, Item::Value(v));
                    }
                }
            }
            Item::Table(t)
        }
        FieldType::Map { .. } => Item::Table(Table::new()),
        FieldType::Scalar { ty, .. } => Item::Value(match &ty.base {
            Base::Bool => Value::from(false),
            Base::Int => Value::from(0),
            Base::Float | Base::Decimal(_) => Value::from(0.0),
            _ => Value::from(""),
        }),
        other => Item::Value(empty(other).unwrap_or_else(|| Value::from(""))),
    }
}

/// Why `key` cannot be the key of a new entry in the map at `map`, if anything.
pub fn key_problem(schema: &Schema, root: &Table, map: &Path, key_ty: &FieldType, key: &str) -> Option<String> {
    if key.trim().is_empty() {
        return Some("the name must not be empty".into());
    }
    if get(root, map).and_then(Node::table_like).is_some_and(|t| t.contains_key(key)) {
        return Some(format!("'{key}' exists already"));
    }
    match key_ty {
        FieldType::Scalar { ty, .. } => Validator::new(schema, None).check_scalar(&Value::from(key), ty).err(),
        FieldType::Ref(target) => (!lookup(root, target).is_some_and(|t| t.contains_key(key))).then(|| format!("'{key}' is not an entry of {target}")),
        _ => None,
    }
}

/// Adds `entry` under `key` to the table at `map`: a new `[table]` (or
/// `[[list]]`) goes after the last table inside `map`; inside an inline or
/// dotted table it takes their style.
pub fn add_entry(doc: &mut DocumentMut, map: &Path, key: &str, entry: Item) -> Result<(), String> {
    let existing = get(doc.as_table(), map);
    if existing.and_then(Node::table_like).is_some_and(|t| t.contains_key(key)) {
        return Err(format!("'{key}' exists already"));
    }
    let inline = matches!(existing, Some(Node::Value(Value::InlineTable(_))));
    let dotted = existing.and_then(Node::table_like).is_some_and(|t| t.is_dotted());
    let item = match entry {
        Item::Table(t) if inline => Item::Value(Value::InlineTable(t.into_inline_table())),
        Item::ArrayOfTables(a) if inline => Item::Value(Value::Array(a.into_array())),
        Item::Table(mut t) if dotted => {
            t.set_dotted(true);
            Item::Table(t)
        }
        Item::Table(mut t) => {
            place_table(doc, &mut t, map);
            Item::Table(t)
        }
        Item::ArrayOfTables(mut a) => {
            if let Some(t) = a.get_mut(0) {
                place_table(doc, t, map);
            }
            Item::ArrayOfTables(a)
        }
        other => other,
    };
    table_mut(doc.as_table_mut(), &map.0)?.insert(key, item);
    Ok(())
}

/// Appends an element to the `[[list]]` at `path`; it goes after the last
/// table of the list.
pub fn push_table(doc: &mut DocumentMut, path: &Path) -> Result<(), String> {
    let mut t = Table::new();
    place_table(doc, &mut t, path);
    let [parent @ .., Segment::Key(k)] = path.0.as_slice() else { return Err("not a list of tables".into()) };
    let list = table_mut(doc.as_table_mut(), parent)?
        .get_mut(k)
        .and_then(Item::as_array_of_tables_mut)
        .ok_or_else(|| format!("{path} is not a list of tables"))?;
    list.push(t);
    Ok(())
}

/// Gives a new `[header]` table the position after the last table inside
/// `after` (or at the end of the data). Comments at the end of that table
/// belong to it: the new table goes below them.
fn place_table(doc: &mut DocumentMut, t: &mut Table, after: &Path) {
    let mut all = Vec::new();
    collect_tables(doc.as_table(), &Path::root(), &mut all);
    let pos = all
        .iter()
        .filter(|(_, p, _)| p.starts_with(after))
        .map(|(pos, _, _)| *pos)
        .max()
        .unwrap_or_else(|| all.iter().map(|(pos, _, _)| *pos).max().unwrap_or(0));
    let next = all.iter().filter(|(p, _, _)| *p > pos).min_by_key(|(p, _, _)| *p).map(|(_, p, pre)| (p.clone(), pre.clone()));
    let moved = match &next {
        Some((p, pre)) => {
            let (before, after) = split_blank(pre);
            if !before.is_empty()
                && let Some(h) = header_mut(doc.as_table_mut(), &p.0)
            {
                h.decor_mut().set_prefix(after);
            }
            before
        }
        None => {
            let trailing = doc.trailing().as_str().unwrap_or("").to_string();
            let at = trailing.split_inclusive('\n').take_while(|l| !l.starts_with("#%")).map(str::len).sum::<usize>();
            let (pre, schema) = trailing.split_at(at);
            let (before, after) = split_blank(pre);
            if !before.is_empty() {
                doc.set_trailing(format!("{after}{schema}"));
            }
            before
        }
    };
    if !moved.is_empty() {
        t.decor_mut().set_prefix(format!("{moved}\n"));
    }
    shift_positions(doc.as_table_mut(), pos);
    t.set_position(Some(pos + 1));
}

/// Removes the map entry at `entry` with all its tables and the comment
/// directly above it. Comments above it that belong to the table before stay.
pub fn remove_entry(doc: &mut DocumentMut, entry: &Path) -> Result<(), String> {
    if entry.is_root() {
        return Err("the root cannot be removed".into());
    }
    if get(doc.as_table(), entry).is_none() {
        return Err(format!("{entry} does not exist"));
    }
    let mut all = Vec::new();
    collect_tables(doc.as_table(), &Path::root(), &mut all);
    let own_prefix = all.iter().find(|(_, p, _)| p == entry).map(|(_, _, pre)| pre.clone()).unwrap_or_default();
    let subtree_end = all.iter().filter(|(_, p, _)| p.starts_with(entry)).map(|(pos, _, _)| *pos).max();
    let subtree_start = all.iter().filter(|(_, p, _)| p.starts_with(entry)).map(|(pos, _, _)| *pos).min();
    // Nothing above it: what follows becomes the start of the file.
    let at_top = subtree_start.is_some_and(|start| all.iter().all(|(pos, _, _)| *pos >= start))
        && !doc.as_table().iter().any(|(_, i)| i.is_value());
    let next = subtree_end.and_then(|end| {
        all.iter().filter(|(pos, p, _)| *pos > end && !p.starts_with(entry)).min_by_key(|(pos, _, _)| *pos).map(|(_, p, pre)| (p.clone(), pre.clone()))
    });
    match entry.0.as_slice() {
        // An element of a `[[list]]`; the list goes when it gets empty.
        [parent @ .., Segment::Key(k), Segment::Index(i)] => {
            let t = table_mut(doc.as_table_mut(), parent)?;
            let item = t.get_mut(k).ok_or_else(|| format!("no {k}"))?;
            let empty = if let Some(list) = item.as_array_of_tables_mut() {
                list.remove(*i);
                list.is_empty()
            } else if let Some(arr) = item.as_array_mut() {
                // A list of inline tables.
                arr.remove(*i);
                arr.is_empty()
            } else {
                return Err(format!("{k} is not a list"));
            };
            if empty {
                t.remove(k);
            }
        }
        [parent @ .., Segment::Key(k)] => {
            table_mut(doc.as_table_mut(), parent)?.remove(k);
        }
        _ => return Err(format!("{entry} cannot be removed")),
    }
    if subtree_end.is_none() {
        return Ok(());
    }
    // The text between the neighbours: what belonged to the table before stays,
    // what belonged to the removed one (at its end) goes.
    let keep = split_blank(&own_prefix).0;
    match next {
        Some((mut p, pre)) => {
            // A later element of the same `[[list]]` moved up by one.
            if let [list @ .., Segment::Index(i)] = entry.0.as_slice()
                && p.0.len() > list.len()
                && p.0.starts_with(list)
                && let Segment::Index(j) = &mut p.0[list.len()]
                && *j > *i
            {
                *j -= 1;
            }
            let mut text = format!("{keep}{}", split_blank(&pre).1);
            if at_top {
                text = text.trim_start_matches(['\n', '\r', ' ', '\t']).to_string();
            }
            if let Some(t) = header_mut(doc.as_table_mut(), &p.0) {
                t.decor_mut().set_prefix(text);
            }
        }
        None => {
            let trailing = doc.trailing().as_str().unwrap_or("").to_string();
            let at = trailing.split_inclusive('\n').take_while(|l| !l.starts_with("#%")).map(str::len).sum::<usize>();
            let (before, schema) = trailing.split_at(at);
            doc.set_trailing(format!("{keep}{}{schema}", split_blank(before).1));
        }
    }
    Ok(())
}

/// Splits text above a header at its last blank line: the lines before it and the rest.
fn split_blank(text: &str) -> (String, String) {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    match lines.iter().rposition(|l| l.trim().is_empty()) {
        Some(i) => (lines[..i].concat(), lines[i..].concat()),
        None => (String::new(), text.to_string()),
    }
}

/// The `[header]` table at the path.
pub(crate) fn header_mut<'a>(root: &'a mut Table, segs: &[Segment]) -> Option<&'a mut Table> {
    let mut t = root;
    let mut i = 0;
    while i < segs.len() {
        let Segment::Key(k) = &segs[i] else { return None };
        let item = t.get_mut(k)?;
        if let Some(Segment::Index(n)) = segs.get(i + 1) {
            t = item.as_array_of_tables_mut()?.get_mut(*n)?;
            i += 2;
        } else {
            t = item.as_table_mut()?;
            i += 1;
        }
    }
    Some(t)
}

/// Moves every table after `after` one position down, making room for a new one.
fn shift_positions(t: &mut Table, after: isize) {
    for (_, item) in t.iter_mut() {
        match item {
            Item::Table(sub) => {
                if let Some(p) = sub.position().filter(|p| *p > after) {
                    sub.set_position(Some(p + 1));
                }
                shift_positions(sub, after);
            }
            Item::ArrayOfTables(a) => {
                for sub in a.iter_mut() {
                    if let Some(p) = sub.position().filter(|p| *p > after) {
                        sub.set_position(Some(p + 1));
                    }
                    shift_positions(sub, after);
                }
            }
            _ => {}
        }
    }
}

// ── Values without schema ───────────────────────────────────────────────────

/// Kinds of values that can be created or switched to without a schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Text,
    Multiline,
    Integer,
    Float,
    Bool,
    Date,
    List,
    Table,
    Tables,
}

impl Kind {
    /// All kinds for a new key; values switch between the first seven.
    pub const ALL: [Kind; 9] = [Kind::Text, Kind::Multiline, Kind::Integer, Kind::Float, Kind::Bool, Kind::Date, Kind::List, Kind::Table, Kind::Tables];

    pub fn label(self) -> &'static str {
        match self {
            Kind::Text => "text",
            Kind::Multiline => "multi-line text",
            Kind::Integer => "integer",
            Kind::Float => "decimal number",
            Kind::Bool => "true / false",
            Kind::Date => "date",
            Kind::List => "list",
            Kind::Table => "table",
            Kind::Tables => "list of tables",
        }
    }

    pub fn from_label(s: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.label() == s)
    }

    /// Whether this kind is a plain value (not a table).
    pub fn is_value(self) -> bool {
        !matches!(self, Kind::Table | Kind::Tables)
    }
}

/// The kind of a value; `None` for kinds without an entry (time, inline table …).
pub fn kind_of(v: &Value) -> Option<Kind> {
    Some(match v {
        Value::String(_) if is_multiline(v) => Kind::Multiline,
        Value::String(_) => Kind::Text,
        Value::Integer(_) => Kind::Integer,
        Value::Float(_) => Kind::Float,
        Value::Boolean(_) => Kind::Bool,
        Value::Datetime(d) if d.value().time.is_none() => Kind::Date,
        Value::Array(_) => Kind::List,
        _ => return None,
    })
}

/// A string written as `"""…"""` / `\'\'\'…\'\'\'` or holding a line break.
pub fn is_multiline(v: &Value) -> bool {
    match v {
        Value::String(s) => {
            let repr = v.to_string();
            let repr = repr.trim_start();
            s.value().contains('\n') || repr.starts_with("\"\"\"") || repr.starts_with("\'\'\'")
        }
        _ => false,
    }
}

/// A multi-line string, written as `"""…"""` even if it has one line.
pub fn multiline_string(text: &str) -> Value {
    let escaped = text.replace('\\', "\\\\").replace("\"\"\"", "\"\"\\\"");
    format!("\"\"\"\n{escaped}\"\"\"").parse::<Value>().unwrap_or_else(|_| Value::from(text))
}

fn today() -> Value {
    let days = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() / 86_400) as i64;
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}").parse().expect("formatted date is TOML")
}

/// An empty value of a kind (tables are made by the caller).
pub fn empty_value(kind: Kind) -> Value {
    match kind {
        Kind::Text | Kind::Table | Kind::Tables => Value::from(""),
        Kind::Multiline => multiline_string(""),
        Kind::Integer => Value::from(0),
        Kind::Float => Value::from(0.0),
        Kind::Bool => Value::from(false),
        Kind::Date => today(),
        Kind::List => Value::Array(Array::new()),
    }
}

/// `v` as another kind. When it cannot be converted, an empty value of the
/// kind and a note saying so.
pub fn convert(v: &Value, kind: Kind) -> (Value, Option<String>) {
    // A list with one element converts like that element.
    if let (Value::Array(a), false) = (v, kind == Kind::List) {
        return match a.len() {
            1 => convert(a.get(0).expect("one element"), kind),
            n => (empty_value(kind), Some(format!("a list of {n} cannot become {}: now empty", kind.label()))),
        };
    }
    let text = match v {
        Value::String(s) => s.value().clone(),
        other => value_text(other),
    };
    let failed = || (empty_value(kind), Some(format!("'{text}' is no {}: now empty", kind.label())));
    let converted = match kind {
        Kind::Text => Some(Value::from(text.as_str())),
        Kind::Multiline => Some(multiline_string(&text)),
        Kind::Integer => match v {
            Value::Float(f) if f.value().fract() == 0.0 => Some(Value::from(*f.value() as i64)),
            Value::Boolean(b) => Some(Value::from(i64::from(*b.value()))),
            _ => text.trim().replace('_', "").parse::<i64>().ok().map(Value::from),
        },
        Kind::Float => parse_number(text.trim()).map(|(n, _)| Value::from(n)),
        Kind::Bool => match text.trim() {
            "1" => Some(Value::from(true)),
            "0" => Some(Value::from(false)),
            t => parse_bool(t).map(Value::from),
        },
        Kind::Date => match v {
            Value::Datetime(d) => d.value().date.map(|d| d.to_string().parse().expect("a date is TOML")),
            _ => parse_date(text.trim()).map(|days| {
                let (y, m, d) = civil_from_days(days);
                format!("{y:04}-{m:02}-{d:02}").parse().expect("formatted date is TOML")
            }),
        },
        Kind::List => Some(match v {
            Value::Array(_) => v.clone(),
            other => {
                let mut a = Array::new();
                a.push(other.clone().decorated("", ""));
                Value::Array(a)
            }
        }),
        Kind::Table | Kind::Tables => None,
    };
    match converted {
        Some(c) => (c, None),
        None => failed(),
    }
}

/// Turns what was typed into a value of the field's type. `old` is the
/// current value, used to keep its kind when there is no schema.
pub fn parse_input(input: &str, ty: Option<&FieldType>, old: Option<&Value>) -> Result<Value, String> {
    match ty {
        Some(FieldType::Scalar { ty, .. }) => parse_typed(input, ty),
        Some(FieldType::Ref(_)) => Ok(Value::from(input.trim())),
        Some(other) => Err(format!("{} is edited in the tree", describe(other))),
        None => parse_generic(input, old),
    }
}

fn parse_typed(input: &str, ty: &Type) -> Result<Value, String> {
    let raw = if ty.multiline { input } else { input.trim() };
    let typed = match &ty.base {
        Base::Auto => parse_generic(raw, None),
        Base::Int => raw
            .replace('_', "")
            .parse::<i64>()
            .map(Value::from)
            .map_err(|_| format!("not an integer: {raw}")),
        Base::Float => parse_number(raw).map(|(n, _)| Value::from(n)).ok_or_else(|| format!("not a number: {raw}")),
        Base::Decimal(d) => match parse_number(raw) {
            Some((n, _)) => Ok(format!("{n:.*}", *d as usize).parse::<Value>().expect("formatted number is TOML")),
            None => Err(format!("not a number: {raw}")),
        },
        Base::Bool => parse_bool(raw).map(Value::from).ok_or_else(|| format!("not true/false: {raw}")),
        Base::Date => match parse_date(raw) {
            Some(days) => {
                let (y, m, d) = civil_from_days(days);
                Ok(format!("{y:04}-{m:02}-{d:02}").parse::<Value>().expect("formatted date is TOML"))
            }
            None => Err(format!("not a date: {raw}")),
        },
        Base::Time | Base::DateTime => match raw.parse::<Value>() {
            Ok(v @ Value::Datetime(_)) => Ok(v),
            _ => Err(format!("not a {}: {raw}", ty.base)),
        },
        Base::Record(_) => match raw.parse::<Value>() {
            Ok(v @ Value::InlineTable(_)) => Ok(v),
            _ => Err(format!("expected {{ field = value, … }}: {raw}")),
        },
        Base::Text | Base::Duration | Base::Ref | Base::Enum(_) => Ok(Value::from(raw)),
    };
    match typed {
        // A special value is always allowed, as text if it is not of the type.
        Err(_) if ty.special_meaning(raw).is_some() => Ok(Value::from(raw)),
        other => other,
    }
}

/// Without a type, the input keeps the kind of the current value: text
/// stays text, a number a number, a date a date (also `24.12.2026`). Input
/// that does not fit is refused; F9 changes the kind. A new value is read
/// as a TOML literal if possible, otherwise as text.
fn parse_generic(raw: &str, old: Option<&Value>) -> Result<Value, String> {
    if let Some(o @ Value::String(_)) = old {
        // A multi-line text stays one, also when it gets a single line.
        return Ok(if is_multiline(o) { multiline_string(raw) } else { Value::from(raw) });
    }
    let literal = raw.trim().parse::<Value>().ok().map(|v| v.decorated("", ""));
    match old.and_then(kind_of) {
        Some(kind) => {
            if let Some(v) = literal.filter(|v| kind_of(v) == Some(kind)) {
                return Ok(v);
            }
            match convert(&Value::from(raw.trim()), kind) {
                (v, None) => Ok(v),
                (_, Some(_)) => Err(format!("'{}' is no {} – F9 changes the type", raw.trim(), kind.label())),
            }
        }
        None => Ok(literal.unwrap_or_else(|| Value::from(raw))),
    }
}

/// Values offered as a choice list: `(value, meaning)`.
pub fn choices(ty: &FieldType, root: &Table) -> Option<Vec<(String, String)>> {
    match ty {
        FieldType::Scalar { ty, .. } => {
            let mut out: Vec<(String, String)> = match &ty.base {
                Base::Enum(v) => v.iter().map(|x| (x.clone(), String::new())).collect(),
                Base::Bool => vec![("true".into(), String::new()), ("false".into(), String::new())],
                _ => return None,
            };
            out.extend(ty.special.iter().cloned());
            Some(out)
        }
        FieldType::Ref(target) => {
            Some(lookup(root, target).map(|t| t.iter().map(|(k, _)| (k.to_string(), String::new())).collect()).unwrap_or_default())
        }
        _ => None,
    }
}

/// Short description of a type for the form and the doc panel.
pub fn describe(ty: &FieldType) -> String {
    match ty {
        FieldType::Scalar { name: Some(n), .. } => n.clone(),
        FieldType::Scalar { ty, .. } => ty.base.to_string(),
        FieldType::List { item, .. } => format!("list({})", describe(item)),
        FieldType::Map { value, .. } => format!("map({})", describe(value)),
        FieldType::Ref(p) => format!("ref({p})"),
        FieldType::Struct(s) => s.clone(),
    }
}

/// A value on one line, as shown in the form.
pub fn display(v: &Value) -> String {
    match v {
        Value::String(s) => s.value().replace('\n', "⏎"),
        Value::Array(a) => {
            let parts: Vec<String> = a.iter().map(|x| match x {
                Value::String(s) => format!("\"{}\"", s.value()),
                other => display(other),
            }).collect();
            format!("[{}]", parts.join(", "))
        }
        Value::InlineTable(_) => v.to_string().trim().to_string(),
        other => value_text(other),
    }
}

/// The text put into the edit field for a value.
pub fn edit_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.value().clone(),
        other => display(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(s: &str) -> DocumentMut {
        s.parse().unwrap()
    }

    #[test]
    fn set_keeps_comment_and_spacing() {
        let mut d = doc("[a]\nport  = 1 # note\n");
        set(&mut d, &Path::parse("a.port").unwrap(), Value::from(2)).unwrap();
        assert_eq!(d.to_string(), "[a]\nport  = 2 # note\n");
    }

    #[test]
    fn set_creates_tables_in_the_style_of_siblings() {
        let mut d = doc("[g]\nblack.both = []\n\n[h]\nx = 1\n");
        set(&mut d, &Path::parse("g.white.both").unwrap(), Value::from("x")).unwrap();
        set(&mut d, &Path::parse("h.sub.y").unwrap(), Value::from(2)).unwrap();
        assert_eq!(d.to_string(), "[g]\nblack.both = []\nwhite.both = \"x\"\n\n[h]\nx = 1\n\n[h.sub]\ny = 2\n");
    }

    #[test]
    fn list_elements() {
        let mut d = doc("a = [\"x\", \"y\"]\nb = []\n");
        let a = Path::parse("a").unwrap();
        set(&mut d, &a.index(1), Value::from("z")).unwrap();
        push(&mut d, &a, Value::from("w")).unwrap();
        push(&mut d, &Path::parse("b").unwrap(), Value::from(1)).unwrap();
        push(&mut d, &Path::parse("c").unwrap(), Value::from(true)).unwrap();
        assert!(remove(&mut d, &a.index(0)));
        assert_eq!(d.to_string(), "a = [\"z\", \"w\"]\nb = [1]\nc = [true]\n");
        assert_eq!(edit_text(value_at(d.as_table(), &a.index(1)).unwrap()), "w");
    }

    #[test]
    fn remove_does_not_create() {
        let mut d = doc("x = 1\n");
        assert!(!remove(&mut d, &Path::parse("a.b").unwrap()));
        assert!(remove(&mut d, &Path::parse("x").unwrap()));
        assert_eq!(d.to_string(), "");
    }

    #[test]
    fn converting_kinds() {
        let c = |v: Value, k: Kind| {
            let (v, note) = convert(&v, k);
            (v.to_string().trim().to_string(), note.is_some())
        };
        assert_eq!(c(Value::from("42"), Kind::Integer), ("42".into(), false));
        assert_eq!(c(Value::from("abc"), Kind::Integer), ("0".into(), true));
        assert_eq!(c(Value::from(3), Kind::Text), ("\"3\"".into(), false));
        assert_eq!(c(Value::from("2,5"), Kind::Float), ("2.5".into(), false));
        assert_eq!(c(Value::from(2.0), Kind::Integer), ("2".into(), false));
        assert_eq!(c(Value::from("ja"), Kind::Bool), ("true".into(), false));
        assert_eq!(c(Value::from("17.10.2026"), Kind::Date), ("2026-10-17".into(), false));
        assert_eq!(c(Value::from("x"), Kind::List), ("[\"x\"]".into(), false));
        assert_eq!(c("[7]".parse().unwrap(), Kind::Integer), ("7".into(), false));
        assert_eq!(c("[1, 2]".parse().unwrap(), Kind::Text), ("\"\"".into(), true));
        let (m, _) = convert(&Value::from("one line"), Kind::Multiline);
        assert!(is_multiline(&m) && m.as_str() == Some("one line"));
        assert_eq!(kind_of(&m), Some(Kind::Multiline));
        assert_eq!(kind_of(&Value::from("x")), Some(Kind::Text));
        // Typing into a multi-line text keeps it one.
        assert!(is_multiline(&parse_input("kurz", None, Some(&m)).unwrap()));
        let tricky = multiline_string("a \"\"\" b \\ c");
        assert_eq!(tricky.as_str(), Some("a \"\"\" b \\ c"));
    }

    #[test]
    fn lists_of_tables() {
        let mut d = doc("[[m]]\nn = 1\n\n[[m]]\nn = 2\n\n[z]\nx = 1\n");
        push_table(&mut d, &Path::parse("m").unwrap()).unwrap();
        set(&mut d, &Path::parse("m[2].n").unwrap(), Value::from(3)).unwrap();
        assert_eq!(d.to_string(), "[[m]]\nn = 1\n\n[[m]]\nn = 2\n\n[[m]]\nn = 3\n\n[z]\nx = 1\n");
        remove_entry(&mut d, &Path::parse("m[0]").unwrap()).unwrap();
        assert_eq!(d.to_string(), "[[m]]\nn = 2\n\n[[m]]\nn = 3\n\n[z]\nx = 1\n");
        let mut aot = toml_edit::ArrayOfTables::new();
        aot.push(Table::new());
        add_entry(&mut d, &Path::root(), "new", Item::ArrayOfTables(aot)).unwrap();
        add_entry(&mut d, &Path::parse("z").unwrap(), "sub", Item::Table(Table::new())).unwrap();
        assert_eq!(d.to_string(), "[[m]]\nn = 2\n\n[[m]]\nn = 3\n\n[z]\nx = 1\n\n[z.sub]\n\n[[new]]\n");
        remove_entry(&mut d, &Path::parse("m[1]").unwrap()).unwrap();
        remove_entry(&mut d, &Path::parse("m[0]").unwrap()).unwrap();
        assert!(!d.to_string().contains("[[m]]") && d.get("m").is_none(), "empty list removed");
    }

    #[test]
    fn typed_input() {
        let t = |src: &str| FieldType::Scalar { ty: crate::types::TypeRegistry::default().resolve(src).unwrap(), name: None };
        let s = |v: Result<Value, String>| v.map(|v| v.to_string());
        assert_eq!(s(parse_input(" 1_000 ", Some(&t("int")), None)), Ok("1000".into()));
        assert!(parse_input("x", Some(&t("int")), None).is_err());
        assert_eq!(s(parse_input("x", Some(&t(r#"int special("x": "")"#)), None)), Ok("\"x\"".into()));
        assert_eq!(s(parse_input("2,5", Some(&t("decimal(2)")), None)), Ok("2.50".into()));
        assert_eq!(s(parse_input("ja", Some(&t("bool")), None)), Ok("true".into()));
        assert_eq!(s(parse_input("08.10.2026", Some(&t("date")), None)), Ok("2026-10-08".into()));
        assert_eq!(s(parse_input("a\nb", Some(&t("text multiline")), None)), Ok("\"\"\"\na\nb\"\"\"".into()));
        assert_eq!(s(parse_input("42", None, Some(&Value::from("old")))), Ok("\"42\"".into()));
        assert_eq!(s(parse_input("42", None, Some(&Value::from(1)))), Ok("42".into()));
        assert_eq!(s(parse_input("24.12.2026", None, Some(&"2026-01-01".parse().unwrap()))), Ok("2026-12-24".into()));
        assert_eq!(s(parse_input("2,5", None, Some(&Value::from(1.0)))), Ok("2.5".into()));
        assert_eq!(s(parse_input("[1, 2]", None, Some(&"[]".parse().unwrap()))), Ok("[1, 2]".into()));
        assert!(parse_input("abc", None, Some(&Value::from(1))).unwrap_err().contains("F9"));
        assert_eq!(s(parse_input("hallo welt", None, None)), Ok("\"hallo welt\"".into()));
    }
}
