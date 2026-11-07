//! Checks the data part against the schema (CONFIG-SPEC §2.3, §2.4, §3, §4).

use super::path::Path;
use super::schema::{Condition, FieldType, Schema, Struct, Test};
use crate::types::{check_constraints, Base, Type};
use crate::value::Value as CellValue;
use toml_edit::{ArrayOfTables, Item, Table, TableLike, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    /// Not a problem, e.g. a value that has no effect.
    Note,
    Warning,
    Error,
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Severity::Note => "note",
            Severity::Warning => "warning",
            Severity::Error => "error",
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Diagnostic {
    pub severity: Severity,
    /// The root path stands for the whole file.
    pub path: Path,
    pub message: String,
}

impl std::fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.path.is_root() {
            write!(f, "{}: {}", self.severity, self.message)
        } else {
            write!(f, "{}: {}: {}", self.severity, self.path, self.message)
        }
    }
}

/// A node of the data: a value, a `[table]` or a `[[list]]` of tables.
#[derive(Clone, Copy, Debug)]
pub enum Node<'a> {
    Value(&'a Value),
    Table(&'a Table),
    Tables(&'a ArrayOfTables),
}

impl<'a> Node<'a> {
    pub fn of(item: &'a Item) -> Option<Node<'a>> {
        match item {
            Item::Value(v) => Some(Node::Value(v)),
            Item::Table(t) => Some(Node::Table(t)),
            Item::ArrayOfTables(a) => Some(Node::Tables(a)),
            Item::None => None,
        }
    }

    pub fn table_like(self) -> Option<&'a dyn TableLike> {
        match self {
            Node::Table(t) => Some(t),
            Node::Value(Value::InlineTable(t)) => Some(t),
            _ => None,
        }
    }

    pub fn kind(self) -> &'static str {
        match self {
            Node::Value(v) => kind(v),
            Node::Table(_) => "a table",
            Node::Tables(_) => "a list of tables",
        }
    }

    pub fn value(self) -> Option<&'a Value> {
        match self {
            Node::Value(v) => Some(v),
            _ => None,
        }
    }

    /// Elements of a list: values of an array or tables of a `[[…]]` list.
    pub fn elements(self) -> Option<Vec<Node<'a>>> {
        match self {
            Node::Value(Value::Array(a)) => Some(a.iter().map(Node::Value).collect()),
            Node::Tables(a) => Some(a.iter().map(Node::Table).collect()),
            _ => None,
        }
    }
}

fn kind(v: &Value) -> &'static str {
    match v {
        Value::String(_) => "text",
        Value::Integer(_) => "an integer",
        Value::Float(_) => "a float",
        Value::Boolean(_) => "a bool",
        Value::Datetime(_) => "a date/time",
        Value::Array(_) => "a list",
        Value::InlineTable(_) => "a table",
    }
}

/// A value as compared in conditions and `special`: strings without quotes, others as written.
pub fn value_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.value().clone(),
        Value::Integer(i) => i.value().to_string(),
        Value::Float(f) => f.value().to_string(),
        Value::Boolean(b) => b.value().to_string(),
        other => other.to_string().trim().to_string(),
    }
}

pub struct Validator<'a> {
    schema: &'a Schema,
    /// The document, for `ref(…)` lookups; `None` skips them.
    root: Option<&'a Table>,
    out: Vec<Diagnostic>,
}

impl<'a> Validator<'a> {
    pub fn new(schema: &'a Schema, root: Option<&'a Table>) -> Self {
        Validator { schema, root, out: Vec::new() }
    }

    /// All findings for the document.
    pub fn validate(schema: &'a Schema, doc: &'a toml_edit::DocumentMut) -> Vec<Diagnostic> {
        let mut v = Validator::new(schema, Some(doc.as_table()));
        v.check_struct(doc.as_table(), &schema.root, &Path::root());
        v.out
    }

    /// Findings for a default value from the schema.
    pub(crate) fn check_default(mut self, item: &'a Item, ty: &'a FieldType) -> Vec<Diagnostic> {
        if let Some(node) = Node::of(item) {
            self.check(node, ty, &Path::root());
        }
        self.out
    }

    fn push(&mut self, severity: Severity, path: &Path, message: String) {
        self.out.push(Diagnostic { severity, path: path.clone(), message });
    }

    fn check_struct(&mut self, t: &'a dyn TableLike, st: &'a Struct, path: &Path) {
        for f in &st.fields {
            let active = f.when.as_ref().is_none_or(|c| condition(t, st, c));
            let p = path.key(&f.name);
            match t.get(&f.name).and_then(Node::of) {
                None if f.required() && active => self.push(Severity::Error, &p, "missing required field".into()),
                None => {}
                Some(node) => {
                    if !active {
                        let c = f.when.as_ref().expect("inactive field has a condition");
                        self.push(Severity::Note, &p, format!("has no effect, only used when {c}"));
                    }
                    self.check(node, &f.ty, &p);
                }
            }
        }
        for (k, item) in t.iter() {
            if st.field(k).is_none() && !item.is_none() {
                self.push(Severity::Warning, &path.key(k), "not in schema".into());
            }
        }
    }

    fn check(&mut self, node: Node<'a>, ty: &'a FieldType, path: &Path) {
        match ty {
            FieldType::Struct(name) => {
                let st = self.schema.struct_def(name).expect("resolved struct exists");
                match node.table_like() {
                    Some(t) => self.check_struct(t, st, path),
                    None => self.push(Severity::Error, path, format!("expected a table ({}), found {}", st.label(), node.kind())),
                }
            }
            FieldType::Map { value, key, length } => match node.table_like() {
                Some(t) => {
                    let n = t.iter().filter(|(_, i)| !i.is_none()).count();
                    if let Some(b) = length
                        && !b.contains(n as f64)
                    {
                        self.push(Severity::Error, path, format!("{n} entries, allowed {b}"));
                    }
                    for (k, item) in t.iter() {
                        let p = path.key(k);
                        self.check_key(k, key, &p);
                        if let Some(n) = Node::of(item) {
                            self.check(n, value, &p);
                        }
                    }
                }
                None => self.push(Severity::Error, path, format!("expected a table, found {}", node.kind())),
            },
            FieldType::List { item, length } => {
                let Some(elems) = node.elements() else {
                    return self.push(Severity::Error, path, format!("expected a list, found {}", node.kind()));
                };
                if let Some(b) = length
                    && !b.contains(elems.len() as f64) {
                        self.push(Severity::Error, path, format!("{} elements, allowed {b}", elems.len()));
                    }
                for (i, e) in elems.into_iter().enumerate() {
                    self.check(e, item, &path.index(i));
                }
            }
            FieldType::Ref(target) => match node {
                Node::Value(Value::String(s)) => self.check_ref(s.value(), target, path),
                other => self.push(Severity::Error, path, format!("expected text (an entry of {target}), found {}", other.kind())),
            },
            FieldType::Scalar { ty, .. } => match node {
                Node::Value(v) => {
                    if let Err(e) = self.check_scalar(v, ty) {
                        self.push(Severity::Error, path, e);
                    }
                }
                other => self.push(Severity::Error, path, format!("expected {}, found {}", ty.base, other.kind())),
            },
        }
    }

    fn check_key(&mut self, key: &str, ty: &FieldType, path: &Path) {
        match ty {
            FieldType::Ref(target) => self.check_ref(key, target, path),
            FieldType::Scalar { ty, .. } => {
                let v = Value::from(key);
                if let Err(e) = self.check_scalar(&v, ty) {
                    self.push(Severity::Error, path, format!("bad key: {e}"));
                }
            }
            _ => {}
        }
    }

    fn check_ref(&mut self, key: &str, target: &Path, path: &Path) {
        let Some(root) = self.root else { return };
        if !lookup(root, target).is_some_and(|t| t.contains_key(key)) {
            self.push(Severity::Error, path, format!("'{key}' is not an entry of {target}"));
        }
    }

    /// Checks a plain value against a type (base type, modifiers, checks).
    pub fn check_scalar(&self, v: &Value, ty: &Type) -> Result<(), String> {
        let raw = value_text(v);
        if ty.special_meaning(&raw).is_some() {
            return Ok(());
        }
        let mismatch = || Err(format!("expected {}, found {}", ty.base, kind(v)));
        let cell = match (&ty.base, v) {
            (Base::Auto, _) => return Ok(()),
            (Base::Int, Value::Integer(i)) => CellValue::num(*i.value() as f64),
            (Base::Float | Base::Decimal(_), Value::Integer(i)) => CellValue::num(*i.value() as f64),
            (Base::Float | Base::Decimal(_), Value::Float(f)) => CellValue::num(*f.value()),
            (Base::Text | Base::Duration | Base::Ref, Value::String(s)) => CellValue::Text(s.value().clone()),
            (Base::Enum(variants), Value::String(s)) => {
                if !variants.iter().any(|x| x == s.value()) {
                    return Err(format!("'{}' is not one of {}", s.value(), variants.join(", ")));
                }
                CellValue::Text(s.value().clone())
            }
            (Base::Bool, Value::Boolean(b)) => CellValue::Bool(*b.value()),
            (Base::Date | Base::Time | Base::DateTime, Value::Datetime(d)) => {
                let d = d.value();
                let ok = match ty.base {
                    Base::Date => d.date.is_some() && d.time.is_none(),
                    Base::Time => d.date.is_none() && d.time.is_some(),
                    _ => d.date.is_some() && d.time.is_some(),
                };
                if !ok {
                    return mismatch();
                }
                CellValue::Null
            }
            (Base::Record(fields), Value::InlineTable(t)) => {
                for (name, src) in fields {
                    let fty = self.schema.types.resolve(src)?;
                    match t.get(name) {
                        Some(fv) => self.check_scalar(fv, &fty).map_err(|e| format!("{name}: {e}"))?,
                        None => return Err(format!("missing record field {name}")),
                    }
                }
                if let Some((k, _)) = t.iter().find(|(k, _)| !fields.iter().any(|(n, _)| n == k)) {
                    return Err(format!("unknown record field {k}"));
                }
                return Ok(());
            }
            _ => return mismatch(),
        };
        check_constraints(ty, &raw, &cell)
    }
}

/// Whether the condition of a field holds, with defaults for missing fields (§2.4).
fn condition(t: &dyn TableLike, st: &Struct, c: &Condition) -> bool {
    let value = t
        .get(&c.field)
        .and_then(Item::as_value)
        .cloned()
        .or_else(|| st.field(&c.field).and_then(|f| f.default.clone()));
    let is = |vs: &Vec<String>| value.as_ref().is_some_and(|v| vs.contains(&value_text(v)));
    match &c.test {
        Test::Set => match &value {
            Some(Value::Boolean(b)) => *b.value(),
            Some(_) => true,
            None => false,
        },
        Test::In(vs) => is(vs),
        Test::NotIn(vs) => !is(vs),
    }
}

/// The table at `path` in the document.
pub fn lookup<'a>(root: &'a Table, path: &Path) -> Option<&'a dyn TableLike> {
    let mut t: &dyn TableLike = root;
    for seg in &path.0 {
        let super::path::Segment::Key(k) = seg else { return None };
        t = t.get(k)?.as_table_like()?;
    }
    Some(t)
}
