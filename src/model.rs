//! In-memory representation of a `.hcmd` document.

/// A parsed hardcalc document: an ordered sequence of blocks.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Document {
    pub blocks: Vec<Block>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    /// Plain Markdown lines, preserved verbatim.
    Text(Vec<String>),
    Table(Table),
    /// ```types block: raw definition lines.
    Types(Vec<String>),
    /// ```functions block: user function definitions, kept verbatim.
    Functions(Vec<String>),
    Layer(Layer),
    /// ```chart Name block: `key: value` lines, kept verbatim.
    Chart { name: String, lines: Vec<String> },
    /// ```layout Name block: page settings and placed elements, kept verbatim.
    Layout { name: String, lines: Vec<String> },
    /// Any other fenced code block, preserved verbatim.
    Code { info: String, body: Vec<String> },
}

/// Line in the source file (1-based, 0 = unknown). Never part of equality, so
/// a document equals its re-read written form.
#[derive(Debug, Clone, Copy, Default)]
pub struct SrcLine(pub usize);

impl PartialEq for SrcLine {
    fn eq(&self, _: &SrcLine) -> bool {
        true
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    /// Line of the header row.
    pub line: SrcLine,
    /// Heading level (number of `#`) if declared via `## Name :: shape`.
    pub heading_level: Option<usize>,
    pub name: Option<String>,
    pub shape: Shape,
    pub columns: Vec<Column>,
    pub rows: Vec<Vec<String>>,
    /// Summary rows below the data (```footer block after the table).
    pub footer: Vec<FooterRow>,
    /// Name of data layer a, the table itself (`## Name :: shape [January]`).
    pub layer_name: Option<String>,
    /// Slices and scenarios b, c, … stacked on the table (```slice Name,
    /// ```scenario Name) – the z axis.
    pub layers: Vec<DataLayer>,
    /// `fill` / `nofill` after the shape: whether the view fills the window
    /// with empty cells; otherwise as the shape says (playgrounds fill).
    pub fill: Option<bool>,
}

/// A slice or scenario: the table once more, for another month, a variant …
/// Rows and columns are those of the table; an empty cell takes the table's
/// formula there (and in a scenario also its value).
#[derive(Debug, Clone, PartialEq)]
pub struct DataLayer {
    pub name: String,
    pub kind: LayerKind,
    /// Raw cells, row by row like the table's (may be shorter).
    pub rows: Vec<Vec<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LayerKind {
    /// Own values; empty cells stay empty (months, years …).
    Slice,
    /// Changes to the table; empty cells show the table's values.
    Scenario,
}

impl LayerKind {
    pub fn name(self) -> &'static str {
        match self {
            LayerKind::Slice => "slice",
            LayerKind::Scenario => "scenario",
        }
    }
}

impl Table {
    /// The raw grids that follow the table's rows and columns: the data
    /// and every data layer (for inserting, deleting and moving lines).
    pub fn grids_mut(&mut self) -> impl Iterator<Item = &mut Vec<Vec<String>>> {
        std::iter::once(&mut self.rows).chain(self.layers.iter_mut().map(|l| &mut l.rows))
    }

    /// Every cell that may hold a formula: data, footer and data layers.
    pub fn cells_mut(&mut self) -> impl Iterator<Item = &mut String> {
        self.rows
            .iter_mut()
            .flatten()
            .chain(self.footer.iter_mut().flat_map(|f| &mut f.cells))
            .chain(self.layers.iter_mut().flat_map(|l| l.rows.iter_mut().flatten()))
    }
}

/// A summary row: a label and per column a cell written as in the data –
/// a value, a text or a formula (`=sum`, `=sumif(Status, "open", Amount)`).
#[derive(Debug, Clone, PartialEq)]
pub struct FooterRow {
    pub label: String,
    /// One raw cell per column.
    pub cells: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Shape {
    Vector,
    /// Free sheet like in Excel: columns A, B, C …, the view fills the
    /// screen with empty cells, empty edges are not written.
    Playground,
    Matrix(Vec<Dim>),
    Tensor(Vec<String>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Dim {
    Any,
    Fixed(usize),
    Named(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Align {
    #[default]
    None,
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Column {
    pub name: String,
    pub ty: Option<String>,
    pub align: Align,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Layer {
    /// Line of the opening fence.
    pub line: SrcLine,
    pub name: String,
    /// Explicit target table (`@Name`); otherwise the preceding table.
    pub target: Option<String>,
    pub rules: Vec<Rule>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Rule {
    /// `<target>: <body>`
    Rule { target: String, body: String },
    /// Comment or otherwise unparsed line, kept as is.
    Raw(String),
}

impl Shape {
    pub fn rank(&self) -> usize {
        match self {
            Shape::Vector => 1,
            Shape::Matrix(_) | Shape::Playground => 2,
            Shape::Tensor(axes) => axes.len(),
        }
    }
}

impl std::fmt::Display for Shape {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Shape::Vector => write!(f, "vector"),
            Shape::Playground => write!(f, "playground"),
            Shape::Matrix(dims) if dims.is_empty() => write!(f, "matrix"),
            Shape::Matrix(dims) => {
                let parts: Vec<String> = dims.iter().map(|d| d.to_string()).collect();
                write!(f, "matrix[{}]", parts.join(", "))
            }
            Shape::Tensor(axes) => write!(f, "tensor[{}]", axes.join(", ")),
        }
    }
}

impl std::fmt::Display for Dim {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Dim::Any => write!(f, "*"),
            Dim::Fixed(n) => write!(f, "{n}"),
            Dim::Named(s) => write!(f, "{s}"),
        }
    }
}
