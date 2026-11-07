//! What hardconf shows: the tree of tables, the form rows of a node and the
//! comments of the file (CONFIG-SPEC §4, §7.1). Independent of the terminal UI.

use super::edit::get;
use super::path::{Path, Segment};
use super::schema::{Field, FieldType, Struct};
use super::validate::Node;
use super::ConfigFile;
use std::collections::{HashMap, HashSet};
use toml_edit::{Item, Table, TableLike, Value};

/// What a tree node is, as far as the schema knows.
#[derive(Debug, Clone, PartialEq)]
pub enum Shape {
    /// A struct; the empty name is the root.
    Struct(String),
    Map { value: Box<FieldType>, key: Box<FieldType> },
    /// `list(Struct)`: the item type.
    List(FieldType),
    /// No schema for this table.
    Generic,
}

#[derive(Debug, Clone)]
pub struct TreeNode {
    pub path: Path,
    /// Key in the file (`nas1`, `[0]`).
    pub label: String,
    /// Short text after the label, e.g. the hint of the field.
    pub hint: Option<String>,
    /// Heading of the form, e.g. `Storage target: nas1`.
    pub title: String,
    pub depth: usize,
    pub shape: Shape,
    /// Exists in the file (a missing struct shows its defaults).
    pub present: bool,
    pub has_children: bool,
    /// The schema field this node is the value of, if any.
    pub field: Option<Field>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RowKind {
    /// A single value.
    Value,
    /// Heading of a list of simple values; its elements follow.
    List,
    Element(usize),
    /// "+ add" below the elements of a list.
    Add,
    /// A struct, map or list of structs: a node of the tree.
    Child,
}

#[derive(Debug, Clone)]
pub struct Row {
    pub path: Path,
    pub label: String,
    pub kind: RowKind,
    /// Schema field of the row (for elements and "add": the list's field).
    pub field: Option<Field>,
    /// Type of the value; for list rows the type of the elements.
    pub ty: Option<FieldType>,
    /// Field order inside the struct, for `when` lookups: the struct.
    pub owner: Option<String>,
}

impl ConfigFile {
    fn root_table(&self) -> &Table {
        self.doc.as_table()
    }

    fn node(&self, path: &Path) -> Option<Node<'_>> {
        get(self.root_table(), path)
    }

    /// The struct definition of a shape (`""`: root).
    pub fn struct_of(&self, name: &str) -> Option<&Struct> {
        let s = self.schema.as_ref()?;
        if name.is_empty() { Some(&s.root) } else { s.struct_def(name) }
    }

    /// The visible tree: the root and, below expanded nodes, their children.
    pub fn tree(&self, title: &str, expanded: &HashSet<Path>) -> Vec<TreeNode> {
        let root_shape = if self.schema.is_some() { Shape::Struct(String::new()) } else { Shape::Generic };
        let title = self.schema.as_ref().and_then(|s| s.root.title.clone()).unwrap_or_else(|| title.to_string());
        let mut root = TreeNode {
            path: Path::root(),
            label: title.clone(),
            hint: None,
            title,
            depth: 0,
            shape: root_shape,
            present: true,
            has_children: false,
            field: None,
        };
        let mut out = Vec::new();
        self.walk(&mut root, expanded, &mut out);
        out
    }

    fn walk(&self, node: &mut TreeNode, expanded: &HashSet<Path>, out: &mut Vec<TreeNode>) {
        let mut kids = self.children(node);
        node.has_children = !kids.is_empty();
        out.push(node.clone());
        if node.path.is_root() || expanded.contains(&node.path) {
            for k in &mut kids {
                self.walk(k, expanded, out);
            }
        }
    }

    /// Child nodes of a tree node, in schema order, then in file order.
    pub fn children(&self, node: &TreeNode) -> Vec<TreeNode> {
        let table = self.node(&node.path).and_then(Node::table_like);
        // Entries of a map or list repeat no hint: it is the one of their parent.
        let entries = matches!(node.shape, Shape::Map { .. } | Shape::List(_));
        let child = |label: String, path: Path, shape: Shape, field: Option<&Field>, present: bool| {
            let title = self.title_of(&shape, &label, field, node);
            TreeNode {
                path,
                label,
                hint: field.filter(|_| !entries).and_then(|f| self.schema.as_ref().and_then(|s| s.hint(f)).map(str::to_string)),
                title,
                depth: node.depth + 1,
                shape,
                present,
                has_children: false,
                field: field.cloned(),
            }
        };
        let mut out = Vec::new();
        match &node.shape {
            Shape::Struct(name) => {
                let Some(st) = self.struct_of(name) else { return out };
                for f in &st.fields {
                    if let Some(shape) = shape_of(&f.ty) {
                        let present = table.is_some_and(|t| t.contains_key(&f.name));
                        out.push(child(f.name.clone(), node.path.key(&f.name), shape, Some(f), present));
                    }
                }
                if let Some(t) = table {
                    for (k, item) in t.iter() {
                        if st.field(k).is_none() && is_tree_item(item) {
                            out.push(child(k.to_string(), node.path.key(k), Shape::Generic, None, true));
                        }
                    }
                }
            }
            Shape::Map { value, .. } => {
                let Some(shape) = shape_of(value) else { return out };
                if let Some(t) = table {
                    for (k, _) in t.iter() {
                        out.push(child(k.to_string(), node.path.key(k), shape.clone(), node.field.as_ref(), true));
                    }
                }
            }
            Shape::List(item) => {
                let shape = shape_of(item).unwrap_or(Shape::Generic);
                let n = self.node(&node.path).and_then(Node::elements).map_or(0, |e| e.len());
                for i in 0..n {
                    out.push(child(format!("[{i}]"), node.path.index(i), shape.clone(), node.field.as_ref(), true));
                }
            }
            Shape::Generic => match self.node(&node.path) {
                Some(Node::Tables(a)) => {
                    for i in 0..a.len() {
                        out.push(child(format!("[{i}]"), node.path.index(i), Shape::Generic, None, true));
                    }
                }
                _ => {
                    if let Some(t) = table {
                        for (k, item) in t.iter() {
                            if is_tree_item(item) {
                                out.push(child(k.to_string(), node.path.key(k), Shape::Generic, None, true));
                            }
                        }
                    }
                }
            },
        }
        out
    }

    fn title_of(&self, shape: &Shape, label: &str, field: Option<&Field>, parent: &TreeNode) -> String {
        match (shape, &parent.shape) {
            // A map entry or list element: `Storage target: nas1`.
            (Shape::Struct(s), Shape::Map { .. } | Shape::List(_)) => match self.struct_of(s) {
                Some(st) => format!("{}: {label}", st.label()),
                None => label.to_string(),
            },
            _ => match (field.and_then(|f| self.schema.as_ref()?.hint(f)), shape) {
                (Some(h), _) => format!("{h} ({label})"),
                (None, Shape::Struct(s)) => self.struct_of(s).map_or(label.to_string(), |st| format!("{}: {label}", st.label())),
                _ => label.to_string(),
            },
        }
    }

    /// Form rows of a tree node.
    pub fn rows(&self, node: &TreeNode) -> Vec<Row> {
        let mut out = Vec::new();
        let table = self.node(&node.path).and_then(Node::table_like);
        match &node.shape {
            Shape::Struct(name) => {
                let Some(st) = self.struct_of(name) else { return out };
                for f in &st.fields {
                    self.push_rows(&mut out, node.path.key(&f.name), f.name.clone(), Some(f), Some(&f.ty), Some(name));
                }
                if let Some(t) = table {
                    for (k, item) in t.iter() {
                        if st.field(k).is_none() && !item.is_none() {
                            self.push_generic(&mut out, node.path.key(k), k, item);
                        }
                    }
                }
            }
            Shape::Map { value, .. } => {
                if let Some(t) = table {
                    for (k, _) in t.iter() {
                        self.push_rows(&mut out, node.path.key(k), k.to_string(), node.field.as_ref(), Some(value), None);
                    }
                }
            }
            Shape::List(_) => {
                let n = self.node(&node.path).and_then(Node::elements).map_or(0, |e| e.len());
                for i in 0..n {
                    out.push(Row {
                        path: node.path.index(i),
                        label: format!("[{i}]"),
                        kind: RowKind::Child,
                        field: node.field.clone(),
                        ty: None,
                        owner: None,
                    });
                }
            }
            Shape::Generic => {
                if let Some(t) = table {
                    for (k, item) in t.iter() {
                        if !item.is_none() {
                            self.push_generic(&mut out, node.path.key(k), k, item);
                        }
                    }
                }
            }
        }
        out
    }

    fn push_rows(&self, out: &mut Vec<Row>, path: Path, label: String, field: Option<&Field>, ty: Option<&FieldType>, owner: Option<&String>) {
        let row = |path: Path, label: String, kind: RowKind, ty: Option<&FieldType>| Row {
            path,
            label,
            kind,
            field: field.cloned(),
            ty: ty.cloned(),
            owner: owner.cloned(),
        };
        match ty {
            Some(t) if shape_of(t).is_some() => out.push(row(path, label, RowKind::Child, ty)),
            Some(FieldType::List { item, .. }) => {
                let n = self.node(&path).and_then(Node::elements).map_or(0, |e| e.len());
                out.push(row(path.clone(), label, RowKind::List, Some(item)));
                for i in 0..n {
                    out.push(row(path.index(i), format!("[{i}]"), RowKind::Element(i), Some(item)));
                }
                if !field.is_some_and(|f| f.readonly) {
                    out.push(row(path, "+ add".into(), RowKind::Add, Some(item)));
                }
            }
            _ => out.push(row(path, label, RowKind::Value, ty)),
        }
    }

    fn push_generic(&self, out: &mut Vec<Row>, path: Path, label: &str, item: &Item) {
        let row = |path: Path, label: String, kind: RowKind| Row { path, label, kind, field: None, ty: None, owner: None };
        match item {
            Item::Value(Value::Array(a)) if !a.iter().any(|v| v.is_inline_table()) => {
                out.push(row(path.clone(), label.to_string(), RowKind::List));
                for i in 0..a.len() {
                    out.push(row(path.index(i), format!("[{i}]"), RowKind::Element(i)));
                }
                out.push(row(path, "+ add".into(), RowKind::Add));
            }
            it if is_tree_item(it) => out.push(row(path, label.to_string(), RowKind::Child)),
            _ => out.push(row(path, label.to_string(), RowKind::Value)),
        }
    }

    /// Places that refer to the entry `key` of the map at `target` with `ref(…)`:
    /// values and map keys.
    pub fn references(&self, target: &Path, key: &str) -> Vec<Path> {
        let mut out = Vec::new();
        if let Some(schema) = &self.schema {
            refs_in_struct(schema, self.root_table(), &schema.root, &Path::root(), target, key, &mut out);
        }
        out
    }

    /// Comments in the file, by the table or key they belong to (§4).
    pub fn comments(&self) -> HashMap<Path, String> {
        super::comments::comments(&self.doc)
    }
}

/// Joins `///` lines into paragraphs: a line continues the previous one unless
/// that ends a sentence (`.` `:` `!` `?`), either is empty, or it is indented
/// (CONFIG-SPEC §2.3).
pub fn reflow(doc: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in doc {
        let joins = match out.last() {
            Some(prev) => {
                let p = prev.trim_end();
                !p.is_empty() && !p.ends_with(['.', ':', '!', '?']) && !line.trim().is_empty() && !line.starts_with(char::is_whitespace)
            }
            None => false,
        };
        if joins {
            let prev = out.last_mut().expect("checked");
            prev.push(' ');
            prev.push_str(line.trim());
        } else {
            out.push(line.clone());
        }
    }
    out
}

fn refs_in_struct(schema: &super::Schema, t: &dyn TableLike, st: &Struct, path: &Path, target: &Path, key: &str, out: &mut Vec<Path>) {
    for f in &st.fields {
        if let Some(n) = t.get(&f.name).and_then(Node::of) {
            refs_in(schema, n, &f.ty, &path.key(&f.name), target, key, out);
        }
    }
}

fn refs_in(schema: &super::Schema, node: Node, ty: &FieldType, path: &Path, target: &Path, key: &str, out: &mut Vec<Path>) {
    match ty {
        FieldType::Ref(t) if t == target => {
            if node.value().and_then(Value::as_str) == Some(key) {
                out.push(path.clone());
            }
        }
        FieldType::Struct(s) => {
            if let (Some(t), Some(st)) = (node.table_like(), schema.struct_def(s)) {
                refs_in_struct(schema, t, st, path, target, key, out);
            }
        }
        FieldType::Map { value, key: kt, .. } => {
            if let Some(t) = node.table_like() {
                for (k, item) in t.iter() {
                    if matches!(kt.as_ref(), FieldType::Ref(p) if p == target) && k == key {
                        out.push(path.key(k));
                    }
                    if let Some(n) = Node::of(item) {
                        refs_in(schema, n, value, &path.key(k), target, key, out);
                    }
                }
            }
        }
        FieldType::List { item, .. } => {
            for (i, e) in node.elements().unwrap_or_default().into_iter().enumerate() {
                refs_in(schema, e, item, &path.index(i), target, key, out);
            }
        }
        _ => {}
    }
}

/// The tree shape of a field type; `None` for values shown in the form.
pub fn shape_of(ty: &FieldType) -> Option<Shape> {
    match ty {
        FieldType::Struct(s) => Some(Shape::Struct(s.clone())),
        FieldType::Map { value, key, .. } => Some(Shape::Map { value: value.clone(), key: key.clone() }),
        FieldType::List { item, .. } if matches!(**item, FieldType::Struct(_)) => Some(Shape::List((**item).clone())),
        _ => None,
    }
}

/// Whether an item without schema is a node of the tree (a table or a list of tables).
fn is_tree_item(item: &Item) -> bool {
    match item {
        Item::Table(_) | Item::ArrayOfTables(_) => true,
        Item::Value(Value::InlineTable(_)) => true,
        Item::Value(Value::Array(a)) => !a.is_empty() && a.iter().all(|v| v.is_inline_table()),
        _ => false,
    }
}

fn prefix_of(decor: &toml_edit::Decor) -> String {
    decor.prefix().and_then(|p| p.as_str()).unwrap_or("").to_string()
}

/// `[header]` tables with their position and the text above the header.
pub(crate) fn collect_tables(t: &dyn TableLike, path: &Path, out: &mut Vec<(isize, Path, String)>) {
    for (k, item) in t.iter() {
        let p = path.key(k);
        match item {
            Item::Table(sub) => {
                if let (Some(pos), false) = (sub.position(), sub.is_implicit() || sub.is_dotted()) {
                    out.push((pos, p.clone(), prefix_of(sub.decor())));
                }
                collect_tables(sub, &p, out);
            }
            Item::ArrayOfTables(a) => {
                for (i, sub) in a.iter().enumerate() {
                    let pi = p.index(i);
                    if let Some(pos) = sub.position() {
                        out.push((pos, pi.clone(), prefix_of(sub.decor())));
                    }
                    collect_tables(sub, &pi, out);
                }
            }
            _ => {}
        }
    }
}

/// Keys of plain values: (table path, key path, text above the key).
pub(crate) fn collect_keys(t: &dyn TableLike, path: &Path, out: &mut Vec<(Path, Path, String)>) {
    for (k, item) in t.iter() {
        let p = path.key(k);
        match item {
            Item::Table(sub) if !sub.is_dotted() => collect_keys(sub, &p, out),
            Item::ArrayOfTables(a) => {
                for (i, sub) in a.iter().enumerate() {
                    collect_keys(sub, &p.index(i), out);
                }
            }
            Item::Value(_) => {
                if let Some(key) = t.key(k) {
                    out.push((path.clone(), p, prefix_of(key.leaf_decor())));
                }
            }
            _ => {}
        }
    }
}

/// The struct path segment of a row path's parent, for messages.
pub fn parent(path: &Path) -> Path {
    let mut p = path.clone();
    if let Some(Segment::Index(_)) = p.0.last() {
        p.0.pop();
    }
    p.0.pop();
    p
}

/// Whether a field is used, given the values of its struct (§2.4).
pub fn active(cfg: &ConfigFile, row: &Row) -> bool {
    let Some(f) = &row.field else { return true };
    let Some(c) = &f.when else { return true };
    let Some(st) = row.owner.as_deref().and_then(|o| cfg.struct_of(o)) else { return true };
    let table = get(cfg.doc.as_table(), &parent(&row.path)).and_then(Node::table_like);
    let value = table
        .and_then(|t| t.get(&c.field))
        .and_then(Item::as_value)
        .cloned()
        .or_else(|| st.field(&c.field).and_then(|f| f.default.clone()));
    let is = |vs: &Vec<String>| value.as_ref().is_some_and(|v| vs.contains(&super::validate::value_text(v)));
    match &c.test {
        super::schema::Test::Set => match &value {
            Some(Value::Boolean(b)) => *b.value(),
            Some(_) => true,
            None => false,
        },
        super::schema::Test::In(vs) => is(vs),
        super::schema::Test::NotIn(vs) => !is(vs),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = include_str!("../../examples/backup.toml");

    #[test]
    fn example_tree() {
        let cfg = ConfigFile::parse(EXAMPLE).unwrap();
        let expanded: HashSet<Path> = ["targets", "jobs", "jobs.photos"].iter().map(|p| Path::parse(p).unwrap()).collect();
        let tree = cfg.tree("backup.toml", &expanded);
        let lines: Vec<String> = tree.iter().map(|n| format!("{}{}{}", "  ".repeat(n.depth), n.label, if n.present { "" } else { " -" })).collect();
        assert_eq!(
            lines,
            [
                "Backup configuration",
                "  global",
                "  targets",
                "    nas1",
                "  accounts",
                "  jobs",
                "    photos",
                "      include",
                "      exclude",
                "      accounts",
                "      targets",
                "    documents",
            ]
        );
        let nas1 = &tree[3];
        assert_eq!(nas1.title, "Storage target: nas1");
        let rows: Vec<String> = cfg.rows(nas1).iter().map(|r| r.label.clone()).collect();
        assert_eq!(rows, ["id", "notes", "host", "ssh_port", "api_port", "tls", "tls_name", "tls_ca", "tls_fingerprint"]);
        // documents has no include table: the node is there but marked missing.
        let buch = cfg.children(&tree[11]);
        assert!(buch.iter().any(|n| n.label == "include" && !n.present));
        let wl = cfg.children(&tree[6]).into_iter().find(|n| n.label == "include").unwrap();
        let rows: Vec<(String, RowKind)> = cfg.rows(&wl).iter().map(|r| (r.label.clone(), r.kind)).collect();
        assert_eq!(rows[0], ("both".to_string(), RowKind::List));
        assert_eq!(rows[1], ("+ add".to_string(), RowKind::Add));
        assert_eq!(rows[2], ("upload".to_string(), RowKind::List));
        assert_eq!(rows[3], ("[0]".to_string(), RowKind::Element(0)));
    }

    #[test]
    fn comments_belong_to_the_right_table() {
        let cfg = ConfigFile::parse(EXAMPLE).unwrap();
        let c = cfg.comments();
        let nas1 = &c[&Path::parse("targets.nas1").unwrap()];
        assert!(nas1.starts_with("No certificate accepted yet"), "{nas1}");
        assert!(nas1.ends_with("Used by: photos"));
        assert!(!c.contains_key(&Path::parse("accounts.archive").unwrap()));
        assert!(c[&Path::root()].starts_with("Backup configuration – example"));
        assert!(!c.values().any(|v| v.contains("int range")), "schema lines are not comments");
    }

    #[test]
    fn doc_paragraphs() {
        let d = |s: &[&str]| s.iter().map(|l| l.to_string()).collect::<Vec<_>>();
        let got = reflow(&d(&["A server for backups and", "snapshots. Which", "job uses it.", "\"pin\": exactly that, for", "      self-signed.", "", "New:", "more"]));
        assert_eq!(got, d(&["A server for backups and snapshots. Which job uses it.", "\"pin\": exactly that, for", "      self-signed.", "", "New:", "more"]));
    }

    #[test]
    fn generic_without_schema() {
        let cfg = ConfigFile::parse("a = 1\nl = [1, 2]\n\n[t]\nx = \"y\"\n\n[[p]]\nn = 1\n").unwrap();
        let tree = cfg.tree("f.toml", &HashSet::from([Path::parse("p").unwrap()]));
        let labels: Vec<&str> = tree.iter().map(|n| n.label.as_str()).collect();
        assert_eq!(labels, ["f.toml", "t", "p", "[0]"]);
        let rows: Vec<String> = cfg.rows(&tree[0]).iter().map(|r| format!("{} {:?}", r.label, r.kind)).collect();
        assert_eq!(rows, ["a Value", "l List", "[0] Element(0)", "[1] Element(1)", "+ add Add", "t Child", "p Child"]);
    }
}
