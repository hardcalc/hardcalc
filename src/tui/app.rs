//! Terminal UI state and input handling. Drawing lives in `ui.rs`; this module
//! has no terminal I/O so it can be tested directly.

use crate::engine::{col_letters, in_layer, layer_letters, split_layer, CellId, Workbook, FOOTER_ROW};
use crate::layers::{self, Attached, CellStyle, Color, Formats};
use crate::chart::{self, ChartData, ChartSpec, ChartType};
use crate::model::{Align, Block, Column, Dim, Document, FooterRow, Shape, Table};
use crate::layout::{Element, ElementKind, RasterSpec, RowPick};
use crate::print::{self, Layout, PageSetup, Placed, PlacedElement, PrintCell, PrintItem, PrintTable, Scope, SpanStyle};
use crate::types::{is_builtin, Base, BUILTINS};
use crate::value::Value;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use unicode_width::UnicodeWidthStr;

mod actions;
mod autofill;
mod context_menu;
mod calc_column;
mod calc_view;
mod columns;
mod data_layers;
mod file_dialog;
mod files;
mod filter;
mod jumps;
pub use jumps::Step;
pub mod formatting;
mod menu;
pub use file_dialog::{is_document as file_dialog_is_document, FileDialog, Purpose};
mod functions_view;
mod point;
mod search;
mod shape;
mod sources;
mod values;
mod views;
pub use functions_view::{FnRow, FN_COLS};
pub use calc_view::{CalcRow, FormulaStyle, PreviewKind, CALC_COLS};
pub use context_menu::{ContextMenu, CONTEXT_MENU};
pub use values::{ValueItem, ValueList};
use crate::tui::keymap::{self, Cap, Cond};

pub const FORMAT: &str = "format";
pub const VALIDATE: &str = "validate";
/// Built-in view layer that marks formula cells; always listed, even without a block.
pub const FORMULAS: &str = "formulas";
/// Layer with cell comments (`B1: text`), printed as footnotes.
pub const COMMENTS: &str = "comments";
/// Layer with links (`A2: Expenses[Rent, Amount]`): ↗ in the cell, a click or F4 jumps.
pub const LINKS: &str = crate::links::LAYER;
/// The layers that act; the layer bar always shows them, in this order.
/// Layer that says how columns are shown (`Note: hide`, `Item: width 20`, `B: freeze`).
pub const VIEW: &str = layers::VIEW;
/// Layer whose rules hide rows (`Status: = open`).
pub const FILTER: &str = layers::FILTER;
pub const PREDEFINED_LAYERS: [&str; 6] = [FORMULAS, FORMAT, VALIDATE, FILTER, COMMENTS, LINKS];
const DOUBLE_CLICK: Duration = Duration::from_millis(400);
const UNDO_LIMIT: usize = 200;

/// Function-key bar, labelled like Midnight Commander: the labels of
/// F1 … F10. An empty label: the key has no function right now.
pub type FKeys = [&'static str; 10];

/// Function keys in the layout editor.
pub const LAYOUT_FKEYS: FKeys = ["Help", "Save", "Move", "Edit", "Dup", "Kind", "Page", "Delete", "Menu", "Quit"];

/// Function keys while the print preview is open.
pub const PREVIEW_FKEYS: FKeys = ["Help", "PDF", "Open", "Print", "Orient", "Paper", "Font-", "Font+", "Scope", "Close"];

/// Open print preview: the page layout and the visible part of it.
#[derive(Debug, Clone)]
pub struct Preview {
    pub layout: Layout,
    pub page: usize,
    /// (x, y) scroll offset within the page, in cells.
    pub scroll: (usize, usize),
    /// Index into [`PREVIEW_ZOOMS`]: how many pages are shown side by side.
    pub zoom: usize,
}

/// Zoom steps of the print preview: pages across × down. The first is the
/// single page, the others an overview that shows several pages at once.
pub const PREVIEW_ZOOMS: [(usize, usize); 6] = [(1, 1), (2, 1), (2, 2), (3, 2), (4, 3), (6, 4)];

impl Preview {
    /// Pages across × down at the current zoom.
    pub fn grid(&self) -> (usize, usize) {
        PREVIEW_ZOOMS[self.zoom.min(PREVIEW_ZOOMS.len() - 1)]
    }
}

/// Columns of the type overview.
pub const TYPE_COLS: [&str; 6] = ["Type", "Definition", "Base", "Unit", "Check", "Used in"];

/// One row of the type overview.
#[derive(Debug, Clone, PartialEq)]
pub enum TypeRow {
    /// Definition line `line` of the ```types block at `block`.
    User { block: usize, line: usize, name: String, def: String },
    /// Placeholder for a new type.
    New,
    Builtin { name: &'static str, desc: &'static str },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Mode {
    Normal,
    /// `cursor` is a char index into `buf`.
    Edit { buf: String, cursor: usize },
    Command { buf: String },
}

/// Clickable screen regions, recorded while drawing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Hit {
    Tab(usize),
    Cell(usize, usize),
    ColHeader(usize),
    RowHeader(usize),
    /// The table origin above the row numbers.
    Corner,
    Layer(usize),
    /// A data layer of the current table in the layer bar.
    DataLayer(usize),
    Filter(usize),
    TypesTab,
    PivotButton,
    FKey(u8),
    /// Window tab for a helper view (Inspector, Source).
    ViewTab(ViewKind),
    /// Choice `n` in the Inspector (enum value, true/false).
    Choice(usize),
    /// Category `n` of the chart in the window.
    ChartItem(usize),
    /// Entry `n` of the paper menu.
    PaperChoice(usize),
    /// The switch "layers for all tables / this table".
    LayerScope,
    /// Title of help page `n`.
    HelpPage(usize),
    /// An entry of the file dialog's list or a place of its sidebar.
    File(crate::tui::file_dialog::Spot),
    /// Button `n` of the unsaved-changes question (save, discard, cancel).
    Confirm(usize),
    /// `hardcalc` at the top left: opens the menu.
    Logo,
    /// Title `n` of the menu bar.
    MenuTop(usize),
    /// Entry `n` of the open menu.
    MenuItem(usize),
    /// Entry `n` of the context menu.
    ContextItem(usize),
    /// Suggestion `n` for the command being typed.
    Suggestion(usize),
    /// The fill handle at the corner of the selection.
    FillHandle,
    /// The ↗ of a cell with a link.
    Link(usize, usize),
    /// The way back after a jump, in the status line.
    Back,
    /// The ▾ / ▼ in a column header: opens the value list of the column.
    Values(usize),
    /// Line `n` of the open value list (0: all).
    ValueItem(usize),
    /// The bottom line of the value list: apply.
    ValuesApply,
}

/// What a mouse drag in the layout editor does.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DragMode {
    /// Move element `n`; (column, row) offset of the grab point in the element.
    Move(usize, (usize, i64)),
    /// Resize element `n` at its bottom-right corner.
    Resize(usize),
    /// Draw a new element from this (column, row).
    Create((usize, usize)),
}

/// Running drag and the rectangle it currently describes (grid columns / rows).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayoutDrag {
    pub mode: DragMode,
    pub cols: (usize, usize),
    pub rows: (usize, usize),
}

/// Screen geometry of the layout editor's canvas, recorded while drawing.
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutGeom {
    /// Canvas area on screen (below the ruler).
    pub area: Rect,
    /// Screen column of page column 0.
    pub x0: u16,
    /// Characters per line, grid columns, rows per page, first visible canvas line.
    pub cols: usize,
    pub grid: usize,
    pub body: usize,
    pub scroll: usize,
    /// Canvas line of every (0-based) row of all pages.
    pub rows: Vec<usize>,
    /// For every canvas line the first and last (0-based) row it shows, and
    /// whether it is a page break (then: the last row of the page above).
    pub lines: Vec<(usize, usize, bool)>,
}

impl LayoutGeom {
    /// Canvas lines for `pages` pages of `body` rows. With `zoom` 1 every row
    /// is a line. In the overview about `zoom` rows share a line, but every
    /// element keeps at least one line of its own (the stretches between the
    /// edges of the elements are scaled one by one), and small gaps vanish.
    pub fn canvas(body: usize, pages: usize, zoom: usize, elements: &[Element]) -> (Vec<usize>, Vec<(usize, usize, bool)>) {
        let mut rows = vec![0; pages * body];
        let mut lines: Vec<(usize, usize, bool)> = Vec::new();
        for page in 0..pages {
            let base = page * body;
            let mut cuts = std::collections::BTreeSet::from([base, base + body]);
            for e in elements {
                cuts.extend([e.rows.0 - 1, e.rows.1].into_iter().filter(|c| (base..base + body).contains(c)));
            }
            let cuts: Vec<usize> = cuts.into_iter().collect();
            let page_start = lines.len();
            // Rows of a gap too small for a line: shown with what follows.
            let mut pending: Vec<usize> = Vec::new();
            for w in cuts.windows(2) {
                let (a, len) = (w[0], w[1] - w[0]);
                let covered = elements.iter().any(|e| e.rows.0 - 1 <= a && a < e.rows.1);
                let n = match (zoom, covered) {
                    (1, _) => len,
                    (_, true) => ((len + zoom / 2) / zoom).max(1),
                    (_, false) => len / zoom,
                };
                if n == 0 {
                    pending.extend(a..a + len);
                    continue;
                }
                let first = lines.len();
                pending.drain(..).for_each(|r| rows[r] = first);
                lines.extend((0..n).map(|k| (a + (k * len).div_ceil(n), a + ((k + 1) * len).div_ceil(n) - 1, false)));
                for (k, &(from, to, _)) in lines[first..].iter().enumerate() {
                    (from..=to).for_each(|r| rows[r] = first + k);
                }
            }
            if lines.len() == page_start {
                lines.push((base, base + body - 1, false));
            }
            // A gap at the end of the page: shown with what comes before.
            pending.drain(..).for_each(|r| rows[r] = lines.len() - 1);
            if page + 1 < pages {
                lines.push((base + body - 1, base + body - 1, true));
            }
        }
        (rows, lines)
    }

    /// Grid (column, row) under a screen position; both 1-based. A click on a
    /// page break counts as the last row of the page above. In the overview
    /// it is the first row of the line.
    pub fn cell_at(&self, x: u16, y: u16) -> (usize, usize) {
        let cx = (x as i32 - self.x0 as i32).clamp(0, self.cols as i32 - 1) as usize;
        let unit = self.cols as f64 / self.grid as f64;
        // Same rounding as RasterSpec::span: column g starts at round((g - 1) · unit).
        let col = (1..=self.grid).rev().find(|&g| ((g - 1) as f64 * unit).round() as usize <= cx).unwrap_or(1);
        let line = self.scroll + y.saturating_sub(self.area.y) as usize;
        let row = match self.lines.get(line) {
            Some(&(first, _, _)) => first,
            // Below the last page: on, row by row.
            None => self.rows.len() + (line - self.lines.len()),
        };
        (col, row + 1)
    }

    /// Canvas line of a (1-based) row: pages are separated by a break line.
    pub fn line_of(&self, row: usize) -> usize {
        match self.rows.get(row - 1) {
            Some(&line) => line,
            None => self.lines.len() + (row - 1 - self.rows.len()),
        }
    }

    /// The last (1-based) row shown on the same canvas line as `row`.
    pub fn line_end(&self, row: usize) -> usize {
        self.lines.get(self.line_of(row)).map_or(row, |&(_, last, _)| (last + 1).max(row))
    }
}

/// One line of the Inspector.
#[derive(Debug, Clone, PartialEq)]
pub enum InspectorLine {
    Heading(String),
    Field(&'static str, String),
    Error(String),
    /// Selectable value for the cell; `current` marks the cell's value.
    Choice { value: String, current: bool },
    Blank,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    pub text: String,
    pub error: bool,
}

/// 2D view of a tensor: one axis per screen direction, the others fixed.
#[derive(Debug, Clone)]
pub struct Pivot {
    pub row_axis: usize,
    pub col_axis: usize,
    /// Distinct key values per axis, in order of appearance.
    pub values: Vec<Vec<String>>,
    /// Selected value index for every axis (used for the fixed ones).
    pub fixed: Vec<usize>,
}

/// What a window shows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ViewKind {
    /// A table (or its pivot).
    Table,
    Types,
    /// Details of the main window's cursor cell.
    Inspector,
    /// The document's Markdown source.
    Source,
    /// The n-th ```chart block of the document.
    Chart(usize),
    /// Editor for the n-th ```layout block (main window only).
    Layout(usize),
    /// Own and built-in functions.
    Functions,
    /// The calculation sheet (```hardcalc blocks).
    Calc,
    /// The calculation sheet typeset (right window).
    CalcPreview,
}

/// Whole columns or rows selected through their headers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Header {
    Column,
    Row,
    /// The table origin (row 0, column 0): the whole table; typing renames it.
    Corner,
}

/// State of one window.
#[derive(Debug, Clone)]
pub struct View {
    pub kind: ViewKind,
    pub sheet: usize,
    /// (row, column) in the view.
    pub cursor: (usize, usize),
    /// Other corner of the selection, if a range is selected.
    pub anchor: Option<(usize, usize)>,
    /// The cursor is on the column / row header: whole columns / rows are
    /// selected (from the anchor's to the cursor's).
    pub header: Option<Header>,
    /// First visible (row, column); line offset for Inspector and Source.
    pub scroll: (usize, usize),
    /// Whether drawing should scroll to keep the cursor visible.
    pub follow: bool,
    pub page_rows: usize,
    pub pivot: Option<Pivot>,
    /// Source view: start line of the block it last followed.
    pub mark: Option<usize>,
    /// The data layer shown (0: the table itself).
    pub layer: usize,
}

impl View {
    pub fn new(kind: ViewKind, sheet: usize) -> View {
        View {
            kind,
            sheet,
            cursor: (0, 0),
            anchor: None,
            header: None,
            scroll: (0, 0),
            follow: true,
            page_rows: 10,
            pivot: None,
            mark: None,
            layer: 0,
        }
    }

    /// Main views (tables, types, layouts); Inspector, Source and charts are helpers.
    pub fn is_data(&self) -> bool {
        matches!(self.kind, ViewKind::Table | ViewKind::Types | ViewKind::Functions | ViewKind::Calc | ViewKind::Layout(_))
    }
}

pub struct CellView {
    pub text: String,
    pub align: Align,
    pub style: CellStyle,
    pub error: bool,
    pub violation: bool,
}

pub struct App {
    pub path: PathBuf,
    pub doc: Document,
    pub wb: Workbook,
    attached: Vec<Attached>,
    formats: Formats,
    /// Rules of a user-defined `layer formulas` block (applied to formula cells only).
    formula_formats: Formats,
    /// (sheet, target, text) of all comments.
    comment_rules: Vec<(usize, layers::Target, String)>,
    pub violations: HashMap<CellId, String>,
    /// The rules of the `filter` layers.
    filters: layers::Filters,
    /// The rules of the `view` layers, and per table what they say.
    views: layers::Views,
    column_views: HashMap<usize, layers::ColumnView>,
    /// (sheet, data layer) → per data row, whether the filter hides it.
    hidden_rows: HashMap<(usize, usize), Vec<bool>>,
    /// While a layout element with `where:` is placed: its table and the rows it leaves out.
    print_where: Option<(usize, Vec<bool>)>,
    /// Cells with a link and their destination (shown layers only).
    pub links: HashMap<CellId, String>,
    /// The evaluated calculation sheet, until the document changes.
    calc_cache: Option<Vec<crate::sheet::Out>>,
    /// How the calculation preview draws formulas (`:formulas`).
    pub formula_style: FormulaStyle,
    /// How the file is compressed; kept when saving.
    pub compression: crate::storage::Compression,
    /// The TextBundle the document came from: its assets and info.json are kept.
    pub bundle: Option<crate::bundle::Bundle>,
    /// The open context menu.
    pub context: Option<ContextMenu>,
    /// The text last searched for (`:find`), for Ctrl+G.
    pub last_find: Option<String>,
    /// The open value list of a column (filter by ticking values).
    pub values: Option<ValueList>,
    /// Which conditional inputs work in this terminal.
    pub caps: keymap::Caps,
    /// Where observed capabilities are remembered.
    pub caps_path: Option<PathBuf>,
    /// The key test: index of the input asked for next.
    pub key_test: Option<usize>,
    pub layer_names: Vec<String>,
    /// Layers hidden in all tables.
    pub hidden_layers: HashSet<String>,
    /// Per-table exceptions: (sheet, layer) → visible.
    pub layer_overrides: HashMap<(usize, String), bool>,
    /// Layer switches apply to all tables (true) or only to the current one.
    pub layers_global: bool,
    /// Exclusive layer view: cells show this layer's content, data recedes.
    pub solo: Option<String>,
    /// How the colour theme is chosen, and what the terminal reported.
    pub theme: super::theme::ThemeSetting,
    pub detected_light: Option<bool>,
    /// `:keys`: show each received key in the status line (for diagnosing terminals).
    pub show_keys: bool,
    /// Banded cell backgrounds (`:bands`).
    pub bands: bool,
    /// The slice / scenario last shown per table (kept across tab changes, printed).
    pub levels: HashMap<usize, usize>,
    /// Start of a possible `ESC ESC [ A` sequence (Alt+arrow in "Option as
    /// Meta" mode): time of the Esc, and whether `[` followed.
    esc_seq: Option<(Instant, bool)>,
    /// The focused window's view.
    pub view: View,
    /// The other window's view while the screen is split.
    pub other: Option<View>,
    /// Whether the focused view is the left window.
    pub left_focused: bool,
    pub mode: Mode,
    pub message: Option<Message>,
    pub dirty: bool,
    pub quit: bool,
    pub show_help: bool,
    /// Page of the F1 help (see `help::PAGES`).
    pub help_page: usize,
    /// Clickable regions; the flag tells whether the region belongs to the focused window.
    pub hits: Vec<(Rect, Hit)>,
    /// Regions of the unfocused window (clicking there focuses it first).
    pub other_hits: Vec<(Rect, Hit)>,
    /// Screen areas of the windows: (is left window, area).
    pub window_areas: Vec<(bool, Rect)>,
    /// Width of the left window in percent of the split (40–60).
    pub split_percent: u16,
    /// The split area and its separator column, as last drawn.
    pub split_body: Option<Rect>,
    pub split_sep: Rect,
    /// The separator is being dragged with the mouse.
    pub split_drag: bool,
    clipboard: Vec<Vec<String>>,
    undo: Vec<Step>,
    redo: Vec<Step>,
    last_click: Option<(Instant, u16, u16)>,
    dragging: bool,
    /// Reference being picked while editing a formula.
    point: Option<point::Point>,
    /// The edit began by typing into the cell (not Enter / F2 / double-click):
    /// ← and → keep the input and move to the neighbouring cell, as ↑ ↓ do.
    /// Home / End switch to moving the caret.
    quick_entry: bool,
    /// Dragging picks a range for the formula being edited.
    point_drag: bool,
    /// Dragging the fill handle: the selection it started from …
    fill_drag: Option<((usize, usize), (usize, usize))>,
    /// … and the cell it is over.
    fill_to: Option<(usize, usize)>,
    /// Last `:sort` (sheet, column, descending), cleared by any other action.
    last_sort: Option<(usize, usize, bool, bool)>,
    /// Quit was requested once with unsaved changes; a second request quits.
    quit_armed: bool,
    /// Keep the cursor where a commit placed it instead of moving on.
    skip_move: bool,
    pub preview: Option<Preview>,
    /// Whether the image view was on before the overview switched it on.
    image_before_zoom: Option<bool>,
    /// The last command line run from the command line (`:` alone repeats it).
    pub last_command: Option<String>,
    /// Page setup, kept between previews.
    pub print_setup: PageSetup,
    pub layout_drag: Option<LayoutDrag>,
    /// Layout editor: the selected element is "picked up" – arrows move it (F3).
    pub layout_grab: bool,
    pub layout_geom: Option<LayoutGeom>,
    /// Layout editor: the overview – elements as labelled boxes, a page per screen.
    pub layout_overview: bool,
    /// Layout editor: the rows of the elements as drawn (for the status line).
    pub layout_rows: Vec<(usize, usize)>,
    /// Open paper menu: the highlighted entry of [`print::paper_choices`].
    pub paper_menu: Option<usize>,
    /// File dialog for opening / appending (Ctrl+E, :e, :append).
    pub file_dialog: Option<FileDialog>,
    /// Open menu: (menu, entry).
    pub menu: Option<(usize, usize)>,
    /// Suggestion chosen with ↑/↓ while typing a command.
    pub cmd_choice: Option<usize>,
    /// Question "save or discard unsaved changes?" before this action.
    pub confirm: Option<files::Pending>,
    /// Action to run once the document is saved (after naming it).
    after_save: Option<files::Pending>,
    /// Inline-image protocol of the terminal, if any (set by the event loop).
    pub graphics: Option<super::graphics::Protocol>,
    /// Print preview shows the rasterized PDF page instead of the text grid.
    pub image_mode: bool,
    /// Where the preview image goes on screen (set while drawing).
    pub image_slot: Option<Rect>,
    /// The terminal shows 24-bit colour (else the 256-colour palette is used).
    pub truecolor: bool,
    /// Soft animations (sliding menus and dialogs, cube turns, flowing formula sources).
    pub anim: super::anim::Anim,
    /// Last block-graphics preview: key and scaled page image.
    thumbnail: Option<(u64, super::graphics::RgbImage)>,
}

fn tables(doc: &Document) -> impl Iterator<Item = &Table> {
    doc.blocks.iter().filter_map(|b| if let Block::Table(t) = b { Some(t) } else { None })
}

fn table_mut(doc: &mut Document, sheet: usize) -> Option<&mut Table> {
    doc.blocks.iter_mut().filter_map(|b| if let Block::Table(t) = b { Some(t) } else { None }).nth(sheet)
}

fn new_table(name: &str) -> Table {
    Table {
        compact: true,
        line: Default::default(),
        heading_level: Some(2),
        name: Some(name.to_string()),
        shape: Shape::Matrix(Vec::new()),
        columns: ["A", "B", "C"].iter().map(|n| Column { name: n.to_string(), ty: None, align: Align::None }).collect(),
        rows: vec![vec![String::new(); 3]; 3],
        footer: Vec::new(),
        layer_name: None,
        layers: Vec::new(),
        fill: None,
    }
}

/// Short description of the format attributes that apply, e.g. `align right, bold`.
fn style_summary(st: &CellStyle) -> String {
    let mut parts = Vec::new();
    if let Some(a) = st.align {
        parts.push(format!("align {}", format!("{a:?}").to_lowercase()));
    }
    let color = |c: &Color| match c {
        Color::Named(n) => n.clone(),
        Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
    };
    if let Some(c) = &st.fg {
        parts.push(format!("fg {}", color(c)));
    }
    if let Some(c) = &st.bg {
        parts.push(format!("bg {}", color(c)));
    }
    for (on, name) in [(st.bold, "bold"), (st.italic, "italic"), (st.underline, "underline")] {
        if on {
            parts.push(name.into());
        }
    }
    if let Some(p) = st.percent {
        parts.push(format!("percent({p})"));
    }
    if let Some(d) = st.decimals {
        parts.push(format!("decimals({d})"));
    }
    parts.join(", ")
}

/// Opens a file with the system's default application.
fn open_external(path: &Path) -> std::io::Result<()> {
    use std::process::{Command, Stdio};
    #[cfg(target_os = "macos")]
    let mut cmd = Command::new("open");
    #[cfg(target_os = "windows")]
    let mut cmd = {
        let mut c = Command::new("cmd");
        c.args(["/C", "start", ""]);
        c
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut cmd = Command::new("xdg-open");
    cmd.arg(path).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().map(|_| ())
}

/// The light band shade of row `r` (`None` for the unshaded rows).
fn tui_band(r: usize) -> Option<ratatui::style::Color> {
    super::theme::band(r % 2 == 1, false, true, true)
}

/// Sends a PDF to the default printer via CUPS `lp`.
fn print_file(path: &Path) -> Result<String, String> {
    if cfg!(windows) {
        return Err("printing directly is not supported on Windows yet – use F3 Open and print from the viewer".into());
    }
    // Colour is asked for explicitly: a printer driver may default to grey.
    let out = std::process::Command::new("lp").args(["-o", "print-color-mode=color"]).arg(path).output().map_err(|e| format!("cannot run lp: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(format!("lp failed: {}", String::from_utf8_lossy(&out.stderr).trim()))
    }
}

/// Element `n` becomes `changed`; the elements below move along in the grid.
///
/// An element moves when an element above it that shares a column moved: it
/// keeps its distance to the lowest of those. Elements that stay put limit
/// how far it moves up (one free row stays if there was one), so growing and
/// shrinking again restores the layout. Elements not below anything that
/// moved stay where they are.
fn reflow(elements: &[Element], n: usize, changed: &Element) -> Vec<Element> {
    let overlap = |a: &Element, b: &Element| a.cols.0 <= b.cols.1 && b.cols.0 <= a.cols.1;
    let mut out = elements.to_vec();
    out[n] = changed.clone();
    // Shift of each element's bottom edge (new − old).
    let mut shift = vec![0i64; elements.len()];
    shift[n] = changed.rows.1 as i64 - elements[n].rows.1 as i64;
    let mut order: Vec<usize> = (0..elements.len()).filter(|&m| m != n).collect();
    order.sort_by_key(|&m| elements[m].rows.0);
    for m in order {
        let e = &elements[m];
        let above: Vec<usize> = (0..elements.len()).filter(|&f| f != m && overlap(&elements[f], e) && elements[f].rows.1 < e.rows.0).collect();
        let pushed: Vec<i64> = above.iter().filter(|&&f| shift[f] != 0).map(|&f| e.rows.0 as i64 + shift[f]).collect();
        let Some(&wanted) = pushed.iter().max() else { continue };
        let floor = above
            .iter()
            .filter(|&&f| shift[f] == 0)
            .map(|&f| {
                let gap = (e.rows.0 - elements[f].rows.1 - 1).min(1);
                (out[f].rows.1 + 1 + gap) as i64
            })
            .max()
            .unwrap_or(1);
        let top = wanted.max(floor).max(1) as usize;
        let height = e.rows.1 - e.rows.0;
        out[m].rows = (top, top + height);
        shift[m] = top as i64 - e.rows.0 as i64;
    }
    out
}

/// The element nearest to element `from` in direction (dc, dr) on the page:
/// smallest gap between the edges in that direction, with sideways gaps
/// counting three times (so elements in line win), ties broken by the
/// distance of the centres. Grid columns count as about 8 characters, rows
/// as 2 (characters are about twice as high as wide).
fn neighbour(spec: &RasterSpec, from: usize, (dc, dr): (i32, i32)) -> Option<usize> {
    // (left, right, top, bottom) in character widths.
    let edges = |e: &Element| ((e.cols.0 - 1) as f64 * 8.0, e.cols.1 as f64 * 8.0, (e.rows.0 - 1) as f64 * 2.0, e.rows.1 as f64 * 2.0);
    let centre = |(l, r, t, b): (f64, f64, f64, f64)| ((l + r) / 2.0, (t + b) / 2.0);
    let gap = |a0: f64, a1: f64, b0: f64, b1: f64| (b0 - a1).max(a0 - b1).max(0.0);
    let me = edges(spec.elements.get(from)?);
    let (x0, y0) = centre(me);
    spec.elements
        .iter()
        .enumerate()
        .filter(|(n, _)| *n != from)
        .filter_map(|(n, e)| {
            let other = edges(e);
            let (x, y) = centre(other);
            let ahead = if dc != 0 { (x - x0) * dc as f64 } else { (y - y0) * dr as f64 };
            if ahead <= 0.0 {
                return None;
            }
            let (along, across) = if dc != 0 {
                (gap(me.0, me.1, other.0, other.1), gap(me.2, me.3, other.2, other.3))
            } else {
                (gap(me.2, me.3, other.2, other.3), gap(me.0, me.1, other.0, other.1))
            };
            Some((n, along + 3.0 * across, (x - x0).hypot(y - y0)))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1).then(a.2.total_cmp(&b.2)))
        .map(|(n, _, _)| n)
}

/// Appends a definition to the last ```types block, creating one after the
/// document title if there is none.
fn add_type_line(doc: &mut Document, line: String) {
    if let Some(Block::Types(lines)) = doc.blocks.iter_mut().rev().find(|b| matches!(b, Block::Types(_))) {
        lines.push(line);
        return;
    }
    let at = usize::from(matches!(doc.blocks.first(), Some(Block::Text(_))));
    doc.blocks.insert(at, Block::Types(vec![line]));
}

/// A table name as typed, with runs of spaces collapsed.
fn table_name(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Letters, digits, spaces, `_` and `-`, starting with a letter, digit or `_`.
/// Names that are no identifiers are quoted in formulas: `"My sheet"!B2`.
fn valid_table_name(s: &str) -> bool {
    s.chars().next().is_some_and(|c| c.is_alphanumeric() || c == '_')
        && s.chars().all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | ' '))
}

/// Moves `v[lo..=hi]` one place left (d < 0) or right; the neighbour takes the free place.
fn shift_block<T>(v: &mut [T], lo: usize, hi: usize, d: isize) {
    if d < 0 {
        v[lo - 1..=hi].rotate_left(1);
    } else {
        v[lo..=hi + 1].rotate_right(1);
    }
}

/// Keeps a fixed column count in `matrix[R, C]` in step with the columns.
fn sync_shape(t: &mut Table) {
    if let Shape::Matrix(dims) = &mut t.shape
        && let Some(Dim::Fixed(n)) = dims.get_mut(1)
    {
        *n = t.columns.len();
    }
}

/// Size of a filling table view (as in Excel).
const FILL_ROWS: usize = 1_048_576;
const FILL_COLS: usize = 16_384;

/// Adds empty rows and columns (named by their letter) up to the given size.
fn grow(t: &mut Table, rows: usize, cols: usize) {
    while t.columns.len() < cols {
        let mut name = col_letters(t.columns.len());
        while t.columns.iter().any(|c| c.name == name) {
            name.push('_');
        }
        t.columns.push(Column { name, ty: None, align: Align::None });
    }
    for row in &mut t.rows {
        row.resize(t.columns.len(), String::new());
    }
    for f in &mut t.footer {
        f.cells.resize(t.columns.len(), String::new());
    }
    for row in t.layers.iter_mut().flat_map(|l| &mut l.rows) {
        row.resize(t.columns.len(), String::new());
    }
    while t.rows.len() < rows {
        t.rows.push(vec![String::new(); t.columns.len()]);
    }
    sync_shape(t);
}

/// `B12` → (row 11, col 1)
fn parse_a1(s: &str) -> Option<(usize, usize)> {
    let split = s.find(|c: char| c.is_ascii_digit())?;
    let letters = s[..split].to_ascii_uppercase();
    if letters.is_empty() || !letters.chars().all(|c| c.is_ascii_uppercase()) {
        return None;
    }
    let col = letters.chars().fold(0, |acc, c| acc * 26 + (c as usize - 'A' as usize + 1)) - 1;
    let row: usize = s[split..].parse().ok()?;
    (row >= 1).then(|| (row - 1, col))
}

impl App {
    /// Opens `path`, or starts a new document if it does not exist.
    pub fn open(path: &Path) -> std::io::Result<App> {
        // Spreadsheets are imported into a new, unsaved hardcalc document next to them.
        if crate::import::is_spreadsheet(path) {
            let (doc, report) = crate::import::import_file(path)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            let mut app = App::new(crate::import::target_path(path), doc);
            app.dirty = true;
            app.info(format!("imported {report} – F2 saves as {}", app.path.display()));
            return Ok(app);
        }
        if crate::bundle::Kind::from_path(path).is_some() && path.exists() {
            return App::open_bundle(path);
        }
        let mut notes = Vec::new();
        let (doc, compression) = match crate::storage::read_text(path) {
            Ok((src, c)) => {
                notes = crate::parse::notes(&src);
                (crate::parse(&src).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, format!("{}: {e}", path.display())))?, c)
            }
            // A new file: compressed as its name says.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Document::default(), crate::storage::Compression::from_path(path)),
            Err(e) => return Err(e),
        };
        // A .zip is only read: saving writes a document next to it.
        let zip = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("zip"));
        let mut app = App::new(if zip { path.with_extension(crate::storage::EXTENSION) } else { path.to_path_buf() }, doc);
        app.compression = compression;
        if zip {
            app.dirty = true;
            app.info(format!("read from the zip – F2 saves as {}", app.path.display()));
        } else if compression != crate::storage::Compression::None && crate::storage::Compression::from_path(path) != compression {
            app.info(format!("{} is {}-compressed – saving keeps that", path.display(), compression.name()));
        } else if let Some(first) = notes.first() {
            // What `hardcalc check` warns about: read, but perhaps not as meant.
            let more = if notes.len() > 1 { format!(" (and {} more – hardcalc check lists them)", notes.len() - 1) } else { String::new() };
            app.info(format!("line {}: {}{more}", first.line, first.message));
        }
        Ok(app)
    }

    /// Opens a TextBundle / TextPack. One written by another program is
    /// saved as a new file (unless it is transient).
    fn open_bundle(path: &Path) -> std::io::Result<App> {
        let bundle = crate::bundle::Bundle::read(path)?;
        let (doc, opened) = crate::bundle::document(&bundle)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, format!("{}: {e}", path.display())))?;
        let own = bundle.may_overwrite();
        let target = if own { path.to_path_buf() } else { crate::bundle::new_file_for(path) };
        let mut app = App::new(target, doc);
        if bundle.newer_format() {
            app.dirty = true;
            app.info(format!("made by a newer hardcalc – F2 saves as {}", app.path.display()));
        } else if !own {
            let creator = bundle.creator().unwrap_or("another program").to_string();
            app.dirty = true;
            app.info(format!("made by {creator} – F2 saves as {}", app.path.display()));
        } else if !opened.from_tsv.is_empty() {
            app.dirty = true;
            app.info(format!("rows taken from changed TSV files: {} – F2 saves", opened.from_tsv.join(", ")));
        }
        app.bundle = Some(bundle);
        Ok(app)
    }

    pub fn new(path: PathBuf, mut doc: Document) -> App {
        if tables(&doc).next().is_none() {
            doc.blocks.push(Block::Table(new_table("Sheet1")));
        }
        let (wb, _) = Workbook::from_document(&doc);
        let mut app = App {
            path,
            doc,
            wb,
            attached: Vec::new(),
            formats: Formats::default(),
            formula_formats: Formats::default(),
            comment_rules: Vec::new(),
            violations: HashMap::new(),
            filters: layers::Filters::default(),
            views: layers::Views::default(),
            column_views: HashMap::new(),
            hidden_rows: HashMap::new(),
            print_where: None,
            links: HashMap::new(),
            calc_cache: None,
            formula_style: FormulaStyle::default(),
            compression: crate::storage::Compression::None,
            bundle: None,
            context: None,
            values: None,
            last_find: None,
            caps: keymap::Caps::default(),
            caps_path: None,
            key_test: None,
            layer_names: Vec::new(),
            hidden_layers: HashSet::new(),
            layer_overrides: HashMap::new(),
            layers_global: true,
            solo: None,
            theme: super::theme::ThemeSetting::Auto,
            detected_light: None,
            show_keys: false,
            bands: true,
            levels: HashMap::new(),
            esc_seq: None,
            view: View::new(ViewKind::Table, 0),
            other: None,
            left_focused: true,
            mode: Mode::Normal,
            message: None,
            dirty: false,
            quit: false,
            show_help: false,
            help_page: 0,
            hits: Vec::new(),
            other_hits: Vec::new(),
            window_areas: Vec::new(),
            split_percent: 50,
            split_body: None,
            split_sep: Rect::default(),
            split_drag: false,
            clipboard: Vec::new(),
            undo: Vec::new(),
            redo: Vec::new(),
            last_click: None,
            dragging: false,
            point: None,
            quick_entry: false,
            point_drag: false,
            fill_drag: None,
            fill_to: None,
            last_sort: None,
            quit_armed: false,
            skip_move: false,
            preview: None,
            image_before_zoom: None,
            last_command: None,
            print_setup: PageSetup::default(),
            layout_drag: None,
            layout_grab: false,
            layout_geom: None,
            layout_overview: false,
            layout_rows: Vec::new(),
            paper_menu: None,
            file_dialog: None,
            menu: None,
            cmd_choice: None,
            confirm: None,
            after_save: None,
            graphics: None,
            image_mode: false,
            image_slot: None,
            truecolor: false,
            anim: Default::default(),
            thumbnail: None,
        };
        app.rebuild();
        app.snap_to_shown();
        app
    }

    /// Recreates the workbook from `doc` (after structural changes or undo).
    fn rebuild(&mut self) {
        self.wb = Workbook::from_document(&self.doc).0;
        self.calc_cache = None;
        self.attached = layers::attach(&self.doc, &self.wb);
        self.formats = Formats::new(&self.attached, FORMAT);
        self.formula_formats = Formats::new(&self.attached, FORMULAS);
        self.filters = layers::Filters::new(&self.attached, FILTER);
        self.views = layers::Views::new(&self.attached, VIEW);
        self.column_views = (0..self.wb.sheets.len()).filter(|&s| self.views.applies(s)).map(|s| (s, self.views.columns(&self.wb, s))).collect();
        self.comment_rules = self
            .attached
            .iter()
            .filter(|a| a.layer.name == COMMENTS)
            .flat_map(|a| {
                a.layer.rules.iter().filter_map(move |r| match r {
                    crate::model::Rule::Rule { target, body } => Some((a.sheet, layers::Target::parse(target), body.clone())),
                    crate::model::Rule::Raw(_) => None,
                })
            })
            .collect();
        // The predefined layers first, even without a block; then the own ones.
        self.layer_names = PREDEFINED_LAYERS.iter().map(|n| n.to_string()).collect();
        for name in layers::layer_names(&self.doc) {
            if !self.layer_names.contains(&name) {
                self.layer_names.push(name);
            }
        }
        self.revalidate();
        self.clamp_view();
        if self.other.is_some() {
            self.with_window(!self.left_focused, |app| app.clamp_view());
        }
    }

    /// Keeps the focused view valid after the document changed.
    fn clamp_view(&mut self) {
        self.view.sheet = self.view.sheet.min(self.wb.sheets.len().saturating_sub(1));
        self.view.layer = self.view.layer.min(self.layer_count());
        if self.view.pivot.is_some() {
            self.refresh_pivot();
        }
        self.clamp_cursor();
    }

    // ------------------------------------------------------------ windows

    pub fn is_split(&self) -> bool {
        self.other.is_some()
    }

    /// Runs `f` with the left (`left == true`) or right window focused,
    /// restoring the focus afterwards.
    pub fn with_window<R>(&mut self, left: bool, f: impl FnOnce(&mut App) -> R) -> R {
        if left == self.left_focused || self.other.is_none() {
            return f(self);
        }
        self.swap_focus();
        let result = f(self);
        self.swap_focus();
        result
    }

    /// Moves the focus to the other window (Tab).
    pub fn swap_focus(&mut self) {
        if let Some(other) = &mut self.other {
            std::mem::swap(&mut self.view, other);
            self.left_focused = !self.left_focused;
        }
    }

    /// Ctrl+O: opens the right window (Inspector) or closes it.
    pub fn toggle_split(&mut self) {
        if self.other.is_some() {
            if !self.left_focused {
                self.swap_focus();
            }
            self.other = None;
        } else {
            self.other = Some(View::new(ViewKind::Inspector, self.view.sheet));
        }
    }

    /// Ctrl+U: exchanges the windows – only when both show data, since the
    /// left window is the main window.
    pub fn swap_windows(&mut self) {
        match &self.other {
            None => self.error("Ctrl+U swaps the windows – Ctrl+O opens the second one"),
            Some(o) if !(o.is_data() && self.view.is_data()) => {
                self.error("the left window is the main window – Ctrl+U swaps only when both show data")
            }
            Some(_) => self.left_focused = !self.left_focused,
        }
    }

    /// Window tabs: data views for the left window, plus helpers on the right.
    pub fn window_tabs(&self, left: bool) -> Vec<(String, Hit, bool)> {
        let mut tabs = Vec::new();
        if !left {
            tabs.push(("ⓘ inspector".into(), Hit::ViewTab(ViewKind::Inspector), self.view.kind == ViewKind::Inspector));
            tabs.push(("‹› source".into(), Hit::ViewTab(ViewKind::Source), self.view.kind == ViewKind::Source));
            if self.has_calc() {
                tabs.push(("∑ preview".into(), Hit::ViewTab(ViewKind::CalcPreview), self.view.kind == ViewKind::CalcPreview));
            }
        }
        for (i, s) in self.wb.sheets.iter().enumerate() {
            let name = s.name.clone().unwrap_or_else(|| format!("#{}", i + 1));
            tabs.push((name, Hit::Tab(i), self.view.kind == ViewKind::Table && self.view.sheet == i));
        }
        tabs.push(("⚙ types".into(), Hit::TypesTab, self.types_view()));
        tabs.push(("ƒ functions".into(), Hit::ViewTab(ViewKind::Functions), self.functions_view()));
        if self.has_calc() {
            tabs.push(("∑ calc".into(), Hit::ViewTab(ViewKind::Calc), self.calc_view()));
        }
        if left {
            for (i, (name, _)) in self.layout_blocks().into_iter().enumerate() {
                tabs.push((format!("▤ {name}"), Hit::ViewTab(ViewKind::Layout(i)), self.view.kind == ViewKind::Layout(i)));
            }
        }
        if !left {
            for (i, (name, _)) in self.chart_blocks().into_iter().enumerate() {
                tabs.push((format!("▥ {name}"), Hit::ViewTab(ViewKind::Chart(i)), self.view.kind == ViewKind::Chart(i)));
            }
        }
        tabs
    }

    /// Shows a helper (Inspector, Source, chart) in the right window, opening
    /// it if needed; layouts open in the main window.
    pub fn show_helper(&mut self, kind: ViewKind) {
        if matches!(kind, ViewKind::Layout(_) | ViewKind::Functions | ViewKind::Calc) {
            return self.with_window(true, |app| app.set_view(kind, app.view.sheet));
        }
        if self.other.is_none() {
            self.other = Some(View::new(kind, self.view.sheet));
            return;
        }
        self.with_window(false, |app| {
            if app.view.kind != kind {
                app.set_view(kind, app.view.sheet);
            }
        });
    }

    // ------------------------------------------------------------ charts

    /// (name, lines) of all ```chart blocks in document order.
    pub fn chart_blocks(&self) -> Vec<(String, Vec<String>)> {
        self.doc
            .blocks
            .iter()
            .filter_map(|b| if let Block::Chart { name, lines } = b { Some((name.clone(), lines.clone())) } else { None })
            .collect()
    }

    pub fn chart_data(&mut self, i: usize) -> Result<ChartData, String> {
        let (name, lines) = self.chart_blocks().into_iter().nth(i).ok_or("chart no longer exists")?;
        let spec = ChartSpec::parse(&name, &lines)?;
        chart::resolve(&spec, &mut self.wb)
    }

    /// Category of `data` that belongs to the main window's cursor row.
    pub fn chart_highlight(&self, data: &ChartData) -> Option<usize> {
        let main = if self.left_focused || self.other.is_none() { &self.view } else { self.other.as_ref()? };
        if main.kind != ViewKind::Table || main.sheet != data.sheet || main.pivot.is_some() {
            return None;
        }
        let row = main.cursor.0;
        if data.rows_are_categories {
            return (row < data.categories.len()).then_some(row);
        }
        let key = self.wb.sheets[data.sheet].raw.get(row)?.get(data.x_column?)?.trim();
        data.categories.iter().position(|c| c == key)
    }

    /// Clicking category `i` moves the main window's cursor to its row.
    fn select_chart_item(&mut self, chart: usize, i: usize) {
        let Ok(data) = self.chart_data(chart) else { return };
        let row = if data.rows_are_categories {
            Some(i)
        } else {
            data.x_column.and_then(|xc| {
                let key = data.categories.get(i)?;
                self.wb.sheets[data.sheet].raw.iter().position(|r| r[xc].trim() == key)
            })
        };
        if let Some(row) = row {
            self.with_window(true, |app| {
                if app.view.kind != ViewKind::Table || app.view.sheet != data.sheet {
                    app.switch_sheet(data.sheet);
                }
                app.view.pivot = None;
                app.move_to(row, app.view.cursor.1);
            });
        }
    }

    /// Sets `key: value` in chart block `i` (one undoable step).
    fn set_chart_prop(&mut self, i: usize, key: &str, value: &str) {
        let line = format!("{key}: {value}");
        self.with_doc(|doc| {
            let block = doc.blocks.iter_mut().filter(|b| matches!(b, Block::Chart { .. })).nth(i);
            if let Some(Block::Chart { lines, .. }) = block {
                match lines.iter_mut().find(|l| l.split_once(':').is_some_and(|(k, _)| k.trim() == key)) {
                    Some(l) => *l = line,
                    None => lines.insert(0, line),
                }
            }
        });
    }

    /// `:chart [type]`: a chart of the selected columns (or of a tensor) after its table.
    fn create_chart(&mut self, kind: Option<&str>) {
        if self.view.kind != ViewKind::Table {
            return self.error("place the cursor in a table to create a chart");
        }
        let kind = match kind {
            Some(k) => match ChartType::ALL.into_iter().find(|t| t.name() == k) {
                Some(t) => Some(t),
                None => return self.error(format!("chart types: {}", ChartType::ALL.map(|t| t.name()).join(", "))),
            },
            None => None,
        };
        let sheet = self.view.sheet;
        let table = self.table().clone();
        let Some(table_name) = table.name.clone() else {
            return self.error("give the table a name first (## Name :: matrix)");
        };
        let mut lines = Vec::new();
        if let Shape::Tensor(axes) = &table.shape {
            lines.push(format!("type: {}", kind.unwrap_or(ChartType::Line).name()));
            lines.push(format!("data: {table_name}"));
            lines.push(format!("x: {}", axes[0]));
            if let Some(series) = axes.get(1) {
                lines.push(format!("series: {series}"));
            }
            let filters: Vec<String> = (2..axes.len())
                .filter_map(|a| table.rows.first().map(|r| format!("{}={}", axes[a], r[a].trim())))
                .collect();
            if !filters.is_empty() {
                lines.push(format!("filter: {}", filters.join(", ")));
            }
        } else {
            let ((_, c0), (_, c1)) = self.selection();
            let cols = self.dims().1;
            let numeric = |app: &mut App, c: usize| {
                (0..app.wb.sheets[sheet].cells.len()).any(|r| matches!(app.wb.value((sheet, r, c)), Value::Num(..)))
            };
            let data: Vec<usize> = (c0..=c1.min(cols.saturating_sub(1))).filter(|&c| numeric(self, c)).collect();
            if data.is_empty() {
                return self.error("select one or more columns with numbers");
            }
            lines.push(format!("type: {}", kind.unwrap_or(ChartType::Bar).name()));
            let refs: Vec<String> = data.iter().map(|&c| format!("{table_name}.{}", table.columns[c].name)).collect();
            lines.push(format!("data: {}", refs.join(", ")));
            if !data.contains(&0) && !numeric(self, 0) {
                lines.push(format!("labels: {table_name}.{}", table.columns[0].name));
            }
        }
        let existing: Vec<String> = self.chart_blocks().into_iter().map(|(n, _)| n).collect();
        let mut name = format!("{table_name} chart");
        let mut k = 2;
        while existing.contains(&name) {
            name = format!("{table_name} chart {k}");
            k += 1;
        }
        // Insert after the table and the layers that follow it.
        let mut at = 0;
        let mut seen = 0;
        for (i, b) in self.doc.blocks.iter().enumerate() {
            if let Block::Table(_) = b {
                if seen == sheet {
                    at = i + 1;
                }
                seen += 1;
            } else if at == i && matches!(b, Block::Layer(_) | Block::Chart { .. }) {
                at = i + 1;
            }
        }
        let index = self.doc.blocks[..at].iter().filter(|b| matches!(b, Block::Chart { .. })).count();
        self.with_doc(|doc| doc.blocks.insert(at, Block::Chart { name: name.clone(), lines }));
        self.show_helper(ViewKind::Chart(index));
        self.info(format!("chart '{name}' created – t in the chart switches the type, edit it in ‹› source"));
    }

    // ------------------------------------------------------------ inspector

    /// Details of the main (left) window's cursor cell.
    pub fn inspector(&mut self) -> Vec<InspectorLine> {
        self.with_window(true, |app| app.inspect_cursor())
    }

    fn cell_name(&self, id: CellId, here: usize) -> String {
        let (c, z) = split_layer(id.2);
        let layer = if z > 0 { layer_letters(z) } else { String::new() };
        let a1 = match id.1.checked_sub(FOOTER_ROW) {
            Some(k) => {
                let s = &self.wb.sheets[id.0];
                let col = s.columns.get(c).map_or(String::new(), |c| c.name.clone());
                let label = s.footer.get(k).map_or(String::new(), |f| f.label.clone());
                let layer = if z > 0 { format!(".{}", s.layer_label(z)) } else { String::new() };
                format!("{col}[{label}]{layer}")
            }
            None => format!("{}{}{layer}", col_letters(c), id.1 + 1),
        };
        if id.0 == here {
            a1
        } else {
            let sheet = self.wb.sheets[id.0].name.clone().unwrap_or_else(|| format!("#{}", id.0 + 1));
            format!("{sheet}!{a1}")
        }
    }

    fn cell_list(&self, ids: &[CellId], here: usize) -> String {
        const MAX: usize = 8;
        if ids.is_empty() {
            return "–".into();
        }
        let mut names: Vec<String> = ids.iter().take(MAX).map(|&id| self.cell_name(id, here)).collect();
        if ids.len() > MAX {
            names.push(format!("… +{}", ids.len() - MAX));
        }
        names.join(", ")
    }

    fn inspect_cursor(&mut self) -> Vec<InspectorLine> {
        use InspectorLine::*;
        if self.types_view() {
            return match self.type_rows().get(self.view.cursor.0).cloned() {
                Some(TypeRow::User { name, def, .. }) => {
                    let mut out = vec![Heading(format!("type {name}")), Field("Definition", def.clone())];
                    match self.wb.types.resolve(&def) {
                        Ok(t) => {
                            out.push(Field("Base", t.base.to_string()));
                            if !t.unit.is_none() {
                                out.push(Field("Unit", t.unit.to_string()));
                            }
                            if let Some(c) = t.check {
                                out.push(Field("Check", c));
                            }
                            if let Base::Enum(values) = t.base {
                                out.push(Blank);
                                out.push(Heading("values".into()));
                                out.extend(values.into_iter().map(|v| Field("", v)));
                            }
                        }
                        Err(e) => out.push(Error(e)),
                    }
                    out.push(Field("Used in", self.type_usage(&name)));
                    out
                }
                Some(TypeRow::Builtin { name, desc }) => vec![Heading(format!("built-in {name}")), Field("", desc.into())],
                _ => vec![Heading("new type".into()), Field("", "type a name in column Type".into())],
            };
        }
        let Some(id) = self.target() else {
            return vec![Heading(format!("{} (empty)", self.cursor_label())), Field("", "type to add data here".into())];
        };
        self.wb.eval_all();
        let (s, r, c) = id;
        let sheet = self.wb.sheets[s].name.clone().unwrap_or_else(|| format!("#{}", s + 1));
        let raw = self.wb.sheets[s].raw[r][c].clone();
        let mut out = vec![Heading(format!("{} · {sheet}", self.cell_name(id, s))), Field("Raw", raw.clone())];
        let v = self.wb.value(id);
        out.push(match &v {
            Value::Error(e) => Error(e.clone()),
            Value::Null => Field("Value", "(empty)".into()),
            Value::Num(n, u) if u.is_none() => Field("Value", format!("{n} (number)")),
            Value::Num(n, u) => Field("Value", format!("{n} {u} (number with unit)")),
            Value::Text(t) => Field("Value", format!("\"{t}\" (text)")),
            Value::Bool(b) => Field("Value", format!("{b} (bool)")),
            other => Field("Value", other.display(None, false)),
        });
        // The stack of data layers at this cell.
        if self.layer_count() > 0 {
            let stack: Vec<String> = (0..=self.layer_count())
                .map(|z| {
                    let v = self.wb.display((s, r, in_layer(c, z)));
                    let own = if z > 0 && self.own_raw(z, r, c).trim().is_empty() { "·" } else { "" };
                    format!("{} {}{own}", layer_letters(z), if v.is_empty() { "–".into() } else { v })
                })
                .collect();
            out.push(Field("Layers", stack.join("  ")));
        }

        out.push(Blank);
        let col = &tables(&self.doc).nth(s).expect("sheet has a table").columns[c];
        out.push(Field("Column", col.name.clone()));
        out.push(Field("Type", col.ty.clone().unwrap_or_else(|| "auto (inferred per cell)".into())));
        let ty = self.wb.sheets[s].columns[c].ty.clone();
        if ty.base != Base::Auto {
            out.push(Field("Base", ty.base.to_string()));
        }
        if !ty.unit.is_none() {
            out.push(Field("Unit", ty.unit.to_string()));
        }
        if let Some(check) = &ty.check {
            out.push(Field("Check", check.clone()));
        }
        let choices: Vec<String> = match &ty.base {
            Base::Enum(values) => values.clone(),
            Base::Bool => vec!["true".into(), "false".into()],
            _ => Vec::new(),
        };
        if !choices.is_empty() && !raw.starts_with('=') {
            out.push(Blank);
            out.push(Heading("choose (Enter / click)".into()));
            for value in choices {
                let current = match &v {
                    Value::Bool(b) => value == b.to_string(),
                    _ => raw.trim() == value,
                };
                out.push(Choice { value, current });
            }
        }

        out.push(Blank);
        if raw.starts_with('=') {
            out.push(Field("Uses", self.cell_list(&self.wb.precedents_of(id), s)));
        }
        out.push(Field("Used by", self.cell_list(&self.wb.dependents_of(id), s)));
        if self.layer_visible_in(FORMAT, id.0) {
            let summary = style_summary(&self.formats.style(&mut self.wb, id));
            if !summary.is_empty() {
                out.push(Field("Format", summary));
            }
        }
        for comment in self.comments_for(id) {
            out.push(Field("Comment", comment));
        }
        if let Some(msg) = self.violations.get(&id) {
            out.push(Error(format!("validate: {msg}")));
        }
        out
    }

    /// Comments on the cell at the cursor, for the status line.
    pub fn cursor_comment(&mut self) -> Option<String> {
        let id = self.target()?;
        let c = self.comments_for(id);
        (!c.is_empty()).then(|| c.join(" · "))
    }

    /// Writes choice `n` of the Inspector into the main window's cell.
    pub fn apply_choice(&mut self, n: usize) {
        let choices: Vec<String> = self
            .inspector()
            .into_iter()
            .filter_map(|l| if let InspectorLine::Choice { value, .. } = l { Some(value) } else { None })
            .collect();
        if let Some(value) = choices.get(n).cloned() {
            self.with_window(true, |app| app.commit_value(value));
        }
    }

    fn choice_count(&mut self) -> usize {
        self.inspector().iter().filter(|l| matches!(l, InspectorLine::Choice { .. })).count()
    }

    // ------------------------------------------------------------ source

    /// The document as written to disk, and the line range of the block shown
    /// in the main window.
    pub fn source_lines(&self) -> (Vec<String>, std::ops::Range<usize>) {
        let main = self.main_view();
        let mut lines = Vec::new();
        let mut range = 0..0;
        let mut table = 0;
        let mut types_marked = false;
        for (i, block) in self.doc.blocks.iter().enumerate() {
            if i > 0 {
                lines.push(String::new());
            }
            let start = lines.len();
            let text = crate::write(&Document { blocks: vec![block.clone()], version: None });
            lines.extend(text.lines().map(str::to_string));
            let marked = match block {
                Block::Table(_) => {
                    table += 1;
                    main.kind == ViewKind::Table && main.sheet == table - 1
                }
                // Layers right after the marked table belong to it.
                Block::Layer(l) if main.kind == ViewKind::Table && l.target.is_none() && !range.is_empty() && range.end + 1 == start => true,
                Block::Types(_) if main.kind == ViewKind::Types && !types_marked => {
                    types_marked = true;
                    true
                }
                Block::Functions(_) if main.kind == ViewKind::Functions && !types_marked => {
                    types_marked = true;
                    true
                }
                _ => false,
            };
            if marked {
                range = if range.end + 1 == start && !range.is_empty() { range.start..lines.len() } else { start..lines.len() };
            }
        }
        (lines, range)
    }

    /// Keys for the Inspector and Source windows; returns whether the key was used.
    fn key_helper(&mut self, key: KeyEvent) -> bool {
        match (self.view.kind, key.code) {
            (ViewKind::Inspector, KeyCode::Up) => self.view.cursor.0 = self.view.cursor.0.saturating_sub(1),
            (ViewKind::Inspector, KeyCode::Down) => {
                let n = self.choice_count();
                self.view.cursor.0 = (self.view.cursor.0 + 1).min(n.saturating_sub(1));
            }
            (ViewKind::Inspector, KeyCode::Enter | KeyCode::Char(' ')) => self.apply_choice(self.view.cursor.0),
            (ViewKind::Chart(i), KeyCode::Char('t')) => {
                let next = match self.chart_data(i) {
                    Ok(d) => {
                        let k = ChartType::ALL.iter().position(|t| *t == d.kind).unwrap_or(0);
                        ChartType::ALL[(k + 1) % ChartType::ALL.len()]
                    }
                    Err(_) => ChartType::Bar,
                };
                self.set_chart_prop(i, "type", next.name());
            }
            // Up/Down move the table cursor, which the chart highlights.
            (ViewKind::Chart(_), KeyCode::Up) => self.with_window(true, |app| app.move_by(-1, 0, false)),
            (ViewKind::Chart(_), KeyCode::Down) => self.with_window(true, |app| app.move_by(1, 0, false)),
            (ViewKind::Source | ViewKind::CalcPreview, KeyCode::Up) => self.scroll_lines(-1),
            (ViewKind::Source | ViewKind::CalcPreview, KeyCode::Down) => self.scroll_lines(1),
            (ViewKind::Source | ViewKind::CalcPreview, KeyCode::PageUp) => self.scroll_lines(-(self.view.page_rows as isize)),
            (ViewKind::Source | ViewKind::CalcPreview, KeyCode::PageDown) => self.scroll_lines(self.view.page_rows as isize),
            (ViewKind::Source | ViewKind::CalcPreview, KeyCode::Home) => self.scroll_lines(isize::MIN / 2),
            (ViewKind::Source | ViewKind::CalcPreview, KeyCode::End) => self.scroll_lines(isize::MAX / 2),
            // The formula style: 2d → text → ascii.
            (ViewKind::CalcPreview, KeyCode::Char('f')) => {
                self.formula_style = match self.formula_style {
                    FormulaStyle::TwoD => FormulaStyle::Text,
                    FormulaStyle::Text => FormulaStyle::Ascii,
                    FormulaStyle::Ascii => FormulaStyle::TwoD,
                };
                self.info(format!("formulas: {}", self.formula_style.name()));
            }
            _ => return false,
        }
        true
    }

    fn scroll_lines(&mut self, delta: isize) {
        self.view.scroll.0 = (self.view.scroll.0 as isize).saturating_add(delta).max(0) as usize;
        self.view.follow = false;
    }

    fn revalidate(&mut self) {
        self.sync_filter_switch();
        self.violations.clear();
        let (violations, _) = layers::validate(&self.attached, &mut self.wb, VALIDATE);
        for v in violations {
            if self.layer_visible_in(VALIDATE, v.cell.0) {
                self.violations.entry(v.cell).or_insert(v.message);
            }
        }
        self.refilter();
        self.relink();
    }

    /// The table the layer bar refers to: the focused data window's, or the
    /// main window's while a helper is focused.
    pub fn layer_sheet(&self) -> usize {
        if self.view.is_data() { self.view.sheet } else { self.main_view().sheet }
    }

    /// Whether layer `name` is shown in table `sheet`.
    pub fn layer_visible_in(&self, name: &str, sheet: usize) -> bool {
        self.layer_overrides.get(&(sheet, name.to_string())).copied().unwrap_or(!self.hidden_layers.contains(name))
    }

    /// Whether layer `name` has no rules for the current table yet (the
    /// built-in `formulas` always acts). The layer bar shows it grey.
    pub fn layer_empty(&self, name: &str) -> bool {
        let sheet = self.layer_sheet();
        name != FORMULAS
            && !self
                .attached
                .iter()
                .any(|a| a.sheet == sheet && a.layer.name == name && a.layer.rules.iter().any(|r| matches!(r, crate::model::Rule::Rule { .. })))
    }

    /// Whether layer `name` is shown in the current table.
    pub fn layer_visible(&self, name: &str) -> bool {
        self.layer_visible_in(name, self.layer_sheet())
    }

    /// Whether the current table has its own setting for layer `name`.
    pub fn layer_is_local(&self, name: &str) -> bool {
        self.layer_overrides.contains_key(&(self.layer_sheet(), name.to_string()))
    }

    /// Shows or hides a layer – in all tables or only in the current one,
    /// depending on [`layers_global`](Self::layers_global).
    pub fn toggle_layer(&mut self, name: &str) {
        let sheet = self.layer_sheet();
        let show = !self.layer_visible_in(name, sheet);
        if self.layers_global {
            if show {
                self.hidden_layers.remove(name);
            } else {
                self.hidden_layers.insert(name.to_string());
            }
            // A global switch replaces all per-table exceptions.
            self.layer_overrides.retain(|(_, n), _| n != name);
        } else if show != self.hidden_layers.contains(name) {
            // Back to the global setting: no exception needed.
            self.layer_overrides.remove(&(sheet, name.to_string()));
        } else {
            self.layer_overrides.insert((sheet, name.to_string()), show);
        }
        self.revalidate();
        if name == FILTER {
            self.snap_to_shown();
        }
        if name == VIEW {
            self.snap_to_shown_col();
        }
    }

    pub fn toggle_layer_scope(&mut self) {
        self.layers_global = !self.layers_global;
        let msg = if self.layers_global {
            "layer switches now apply to all tables"
        } else {
            "layer switches now apply to this table only (* marks its own settings)"
        };
        self.info(msg);
    }

    pub fn table(&self) -> &Table {
        tables(&self.doc).nth(self.view.sheet).expect("sheet index is valid")
    }

    fn info(&mut self, text: impl Into<String>) {
        self.message = Some(Message { text: text.into(), error: false });
    }

    fn error(&mut self, text: impl Into<String>) {
        self.message = Some(Message { text: text.into(), error: true });
    }

    // ------------------------------------------------------------ view model

    /// (rows, columns) of data in the current view.
    pub fn dims(&self) -> (usize, usize) {
        if self.types_view() {
            return (self.type_rows().len(), TYPE_COLS.len());
        }
        if self.functions_view() {
            return (self.function_rows().len(), FN_COLS.len());
        }
        if self.calc_view() {
            return (self.calc_rows().len(), CALC_COLS.len());
        }
        // A layout's "rows" are its elements (the cursor selects one).
        if let ViewKind::Layout(i) = self.view.kind {
            return (self.raster_spec(i).elements.len(), 1);
        }
        match &self.view.pivot {
            Some(p) => (p.values[p.row_axis].len(), p.values[p.col_axis].len()),
            None => {
                let s = &self.wb.sheets[self.view.sheet];
                (s.cells.len(), s.columns.len())
            }
        }
    }

    /// Whether the table view is filled with empty cells to the edges of the
    /// window (and beyond, when scrolling) – the default for playgrounds.
    pub fn fills(&self) -> bool {
        self.view.kind == ViewKind::Table
            && self.view.pivot.is_none()
            && self.table().fill.unwrap_or_else(|| self.fills_by_default())
    }

    /// Without `fill` / `nofill`: playgrounds without footer rows fill.
    fn fills_by_default(&self) -> bool {
        let s = &self.wb.sheets[self.view.sheet];
        s.shape == Shape::Playground && s.footer.is_empty()
    }

    /// Navigable size: in a table, one placeholder row and column follow the
    /// data; a filling table reaches as far as Excel's.
    pub fn view_dims(&self) -> (usize, usize) {
        if self.fills() {
            return (FILL_ROWS, FILL_COLS);
        }
        let (rows, cols) = self.dims();
        if self.view.pivot.is_some() || self.view.kind != ViewKind::Table {
            (rows, cols)
        } else {
            (rows + 1 + self.wb.sheets[self.view.sheet].footer.len(), cols + 1)
        }
    }

    /// Whether (r, c) is in the placeholder row or column.
    pub fn is_placeholder(&self, r: usize, c: usize) -> bool {
        let (rows, cols) = self.dims();
        self.view.pivot.is_none() && self.view.kind == ViewKind::Table && ((r >= rows && self.footer_at(r).is_none()) || c >= cols)
    }

    /// View row of the first footer row (below the + row); `None` where
    /// footers are not shown (pivot, filling table, other views).
    fn footer_start(&self) -> Option<usize> {
        (self.view.kind == ViewKind::Table && self.view.pivot.is_none() && !self.fills()).then(|| self.dims().0 + 1)
    }

    /// The footer row shown at view row `r`.
    pub fn footer_at(&self, r: usize) -> Option<usize> {
        let k = r.checked_sub(self.footer_start()?)?;
        (k < self.wb.sheets[self.view.sheet].footer.len()).then_some(k)
    }

    /// The footer row at the cursor.
    fn cursor_footer(&self) -> Option<usize> {
        self.footer_at(self.view.cursor.0)
    }

    /// (top line, name line) of a column header.
    pub fn col_header(&self, c: usize) -> (String, String) {
        if self.types_view() {
            return (String::new(), TYPE_COLS[c].to_string());
        }
        if self.functions_view() {
            return (String::new(), FN_COLS[c].to_string());
        }
        if self.calc_view() {
            return (String::new(), CALC_COLS[c].to_string());
        }
        match &self.view.pivot {
            Some(p) => {
                let axis = if c == 0 { self.axis_name(p.col_axis) } else { String::new() };
                (axis, p.values[p.col_axis][c].clone())
            }
            // A filtered column carries ▼ beside its letter.
            None if self.column_filtered(c) => {
                let col = &self.wb.sheets[self.view.sheet].columns[c];
                let name = if col.ty.unit.is_none() { col.name.clone() } else { format!("{} [{}]", col.name, col.ty.unit) };
                if name == col_letters(c) { ("▼".into(), name) } else { (format!("{} ▼", col_letters(c)), name) }
            }
            // The column with the cursor offers its value list: ▾.
            None if self.values_button(c) => {
                let col = &self.wb.sheets[self.view.sheet].columns[c];
                let name = if col.ty.unit.is_none() { col.name.clone() } else { format!("{} [{}]", col.name, col.ty.unit) };
                (format!("{} ▾", col_letters(c)), name)
            }
            None => {
                let playground = self.wb.sheets[self.view.sheet].shape == Shape::Playground;
                match self.wb.sheets[self.view.sheet].columns.get(c) {
                    // Playground columns are named by their letter: no second line needed.
                    Some(col) if playground && col.name == col_letters(c) => (String::new(), col.name.clone()),
                    // The cells omit the unit of the column type; the header shows it.
                    Some(col) if !col.ty.unit.is_none() => (col_letters(c), format!("{} [{}]", col.name, col.ty.unit)),
                    Some(col) => (col_letters(c), col.name.clone()),
                    None if playground => (String::new(), col_letters(c)),
                    None if self.fills() && c > self.dims().1 => (col_letters(c), String::new()),
                    None => (col_letters(c), "+".into()),
                }
            }
        }
    }

    pub fn row_header(&self, r: usize) -> String {
        if self.calc_view() {
            return match self.calc_rows().get(r) {
                Some(CalcRow::Line { .. }) => (r + 1).to_string(),
                _ => "+".into(),
            };
        }
        if self.functions_view() {
            return match self.function_rows().get(r) {
                Some(FnRow::User { .. }) => (r + 1).to_string(),
                Some(FnRow::New) => "+".into(),
                _ => "·".into(),
            };
        }
        if self.types_view() {
            return match self.type_rows().get(r) {
                Some(TypeRow::User { .. }) => (r + 1).to_string(),
                Some(TypeRow::New) => "+".into(),
                _ => "·".into(),
            };
        }
        if let Some(k) = self.footer_at(r) {
            return self.wb.sheets[self.view.sheet].footer[k].label.clone();
        }
        match &self.view.pivot {
            Some(p) => p.values[p.row_axis][r].clone(),
            None if r >= self.dims().0 && !(self.fills() && r > self.dims().0) && self.wb.sheets[self.view.sheet].shape != Shape::Playground => {
                "+".into()
            }
            None => (r + 1).to_string(),
        }
    }

    /// Label above the row headers.
    pub fn corner(&self) -> String {
        match &self.view.pivot {
            Some(p) => self.axis_name(p.row_axis),
            // The table origin is the head of the data layers.
            None if self.view.kind == ViewKind::Table && self.layer_count() > 0 => self.layer_title(self.shown_layer()),
            None => String::new(),
        }
    }

    pub fn axis_name(&self, axis: usize) -> String {
        match &self.wb.sheets[self.view.sheet].shape {
            Shape::Tensor(axes) => axes.get(axis).cloned().unwrap_or_default(),
            _ => String::new(),
        }
    }

    /// Cell reference shown in the formula bar.
    pub fn cursor_label(&self) -> String {
        if self.types_view() {
            return "types".into();
        }
        if self.functions_view() {
            return "ƒ".into();
        }
        if self.calc_view() {
            return format!("∑ {}", self.view.cursor.0 + 1);
        }
        if let ViewKind::Layout(i) = self.view.kind {
            let n = self.raster_spec(i).elements.len();
            return if n == 0 { "▤ –".into() } else { format!("▤ {}/{n}", self.view.cursor.0 + 1) };
        }
        // A footer cell by its name, as formulas refer to it: Amount[Total].
        if let Some(k) = self.cursor_footer() {
            let s = &self.wb.sheets[self.view.sheet];
            return match (self.on_header(), s.columns.get(self.view.cursor.1)) {
                (None, Some(col)) if self.shown_layer() > 0 => format!("{}[{}].{}", col.name, s.footer[k].label, s.layer_label(self.shown_layer())),
                (None, Some(col)) => format!("{}[{}]", col.name, s.footer[k].label),
                _ => s.footer[k].label.clone(),
            };
        }
        // Whole columns / rows in Excel notation: B:B, B:D, 2:2, 2:4.
        let ((r0, c0), (r1, c1)) = self.selection();
        match self.on_header() {
            Some(Header::Corner) => return "table".into(),
            Some(Header::Column) => return format!("{}:{}", col_letters(c0), col_letters(c1)),
            Some(Header::Row) => return format!("{}:{}", r0 + 1, r1 + 1),
            None => {}
        }
        if self.view.pivot.is_none() {
            let layer = if self.layer_count() > 0 { layer_letters(self.shown_layer()) } else { String::new() };
            return format!("{}{}{layer}", col_letters(self.view.cursor.1), self.view.cursor.0 + 1);
        }
        match self.target() {
            Some((_, r, c)) => format!("{}{}", col_letters(c), r + 1),
            None => "–".into(),
        }
    }

    /// The underlying table cell at the cursor (in a pivot only if exactly one row matches).
    pub fn target(&self) -> Option<CellId> {
        if self.view.kind != ViewKind::Table {
            return None;
        }
        let (r, c) = self.view.cursor;
        match &self.view.pivot {
            Some(_) => match self.pivot_matches(r, c).as_slice() {
                [row] => Some((self.view.sheet, *row, self.wb.sheets[self.view.sheet].columns.len() - 1)),
                _ => None,
            },
            None => (r < self.dims().0 && c < self.dims().1).then_some((self.view.sheet, r, c)),
        }
    }

    /// Raw text of the cell at the cursor.
    pub fn cursor_raw(&self) -> String {
        if self.on_header() == Some(Header::Corner) {
            return self.wb.sheets[self.view.sheet].name.clone().unwrap_or_default();
        }
        if self.on_header() == Some(Header::Column) {
            return tables(&self.doc)
                .nth(self.view.sheet)
                .and_then(|t| t.columns.get(self.view.cursor.1))
                .map(|c| match &c.ty {
                    Some(ty) => format!("{}:{ty}", c.name),
                    None => c.name.clone(),
                })
                .unwrap_or_default();
        }
        if let Some(k) = self.cursor_footer() {
            let row = &self.table().footer[k];
            return match self.on_header() {
                Some(Header::Row) => row.label.clone(),
                _ => row.cells.get(self.view.cursor.1).cloned().unwrap_or_default(),
            };
        }
        // In the layer view the input is the layer's rule – also beyond the data.
        if let Some(layer) = self.solo_rule_layer() {
            let target = self.solo_target();
            return self.layer_rule(&layer, self.view.sheet, &target).unwrap_or_default();
        }
        if let ViewKind::Layout(_) = self.view.kind {
            return self.selected_element().map(|(_, _, e)| e.text()).unwrap_or_default();
        }
        if self.functions_view() {
            return self.function_raw();
        }
        if self.calc_view() {
            return self.calc_raw();
        }
        if self.types_view() {
            let (r, c) = self.view.cursor;
            return match self.type_rows().get(r) {
                Some(TypeRow::User { name, def, .. }) => match c {
                    0 => name.clone(),
                    1 => def.clone(),
                    _ => format!("{name} = {def}"),
                },
                Some(TypeRow::Builtin { name, desc }) => format!("{name}  –  {desc}"),
                _ => String::new(),
            };
        }
        self.target().map(|(_, r, c)| self.effective_raw(r, c)).unwrap_or_default()
    }

    /// The text an edit of the cell under the cursor starts with: numbers as
    /// the display shows them (`1200,50` with the decimal comma); files keep
    /// the point.
    fn cursor_edit_text(&self) -> String {
        let raw = self.cursor_raw();
        let data = self.on_header().is_none()
            && self.solo_rule_layer().is_none()
            && !matches!(self.view.kind, ViewKind::Layout(_))
            && !self.functions_view()
            && !self.calc_view()
            && !self.types_view();
        match self.wb.sheets.get(self.view.sheet).and_then(|s| s.columns.get(self.view.cursor.1)) {
            Some(col) if data && !raw.trim_start().starts_with('=') => crate::types::shown_cell(&raw, &col.ty),
            _ => raw,
        }
    }

    /// `Name : Type` of the column under the cursor.
    pub fn cursor_column_info(&self) -> String {
        if self.types_view() {
            return "type definitions".into();
        }
        if self.functions_view() {
            return "function definitions".into();
        }
        if self.calc_view() {
            return "calculation sheet".into();
        }
        if let ViewKind::Layout(_) = self.view.kind {
            return match self.selected_element() {
                Some((_, n, mut e)) => {
                    // Where it is shown: `height: auto` may have moved it.
                    e.rows = self.layout_rows.get(n).copied().unwrap_or(e.rows);
                    let auto = if e.flows() { " · height: auto" } else { "" };
                    format!("{} · columns {}–{} · rows {}–{}{auto}", e.kind.name(), e.cols.0, e.cols.1, e.rows.0, e.rows.1)
                }
                None => "drag a rectangle to place an element".into(),
            };
        }
        if self.cursor_footer().is_some() {
            return match (self.on_header(), self.view.cursor.1 < self.dims().1) {
                (None, true) => "footer: =sum =avg =min =max =count, a formula, value or text".into(),
                _ => "footer row – type to rename, F8 deletes".into(),
            };
        }
        let Some((_, _, c)) = self.target() else { return String::new() };
        if let Some(layer) = self.solo_rule_layer() {
            return format!("{layer} rule for {}", self.solo_target());
        }
        let col = &self.table().columns[c];
        let info = match &col.ty {
            Some(ty) => format!("{} : {ty}", col.name),
            None => col.name.clone(),
        };
        let (r, _) = self.view.cursor;
        if self.inherited(r, c) && !self.effective_raw(r, c).trim().is_empty() {
            return format!("{info} · from layer a – typing makes it this layer's");
        }
        info
    }

    /// Error or validation message for the cell at the cursor.
    pub fn cursor_problem(&mut self) -> Option<String> {
        if let ViewKind::Layout(i) = self.view.kind {
            let spec = self.raster_spec(i);
            if let Some(e) = spec.errors.first() {
                return Some(e.clone());
            }
            let (shown, placed) = self.resolved_layout(i);
            let n = self.view.cursor.0;
            let (e, p) = (shown.elements.get(n)?, placed.get(n)?);
            let body = print::layout_rows_per_page(&spec.setup(i));
            if !e.flows() && (e.rows.0 - 1) / body != (e.rows.1 - 1) / body {
                return Some(format!("rows {}–{} cross a page break (pages hold {body} rows) – the element is cut there", e.rows.0, e.rows.1));
            }
            return (p.hidden > 0).then(|| {
                format!("{} rows do not fit – F3 then = fits the height, a makes it automatic; or show a part: rows: 10 · top 5 by … · total", p.hidden)
            });
        }
        if self.functions_view() {
            return self.function_problem();
        }
        if self.calc_view() {
            return self.calc_problem();
        }
        if self.types_view() {
            return match self.type_rows().get(self.view.cursor.0) {
                Some(TypeRow::User { name, def, .. }) => self.wb.types.resolve(def).err().map(|e| format!("{name}: {e}")),
                _ => None,
            };
        }
        if let Some(k) = self.cursor_footer()
            && self.view.cursor.1 < self.dims().1
            && let Value::Error(e) = self.wb.value((self.view.sheet, FOOTER_ROW + k, in_layer(self.view.cursor.1, self.shown_layer())))
        {
            return Some(e);
        }
        let id = self.target()?;
        if let Value::Error(e) = self.wb.value((id.0, id.1, in_layer(id.2, self.shown_layer()))) {
            return Some(e);
        }
        self.violations.get(&id).map(|v| format!("validate: {v}"))
    }

    pub fn cell_view(&mut self, r: usize, c: usize) -> CellView {
        if self.types_view() {
            return self.types_cell_view(r, c);
        }
        if self.functions_view() {
            return self.functions_cell_view(r, c);
        }
        if self.calc_view() {
            return self.calc_cell_view(r, c);
        }
        if let Some(k) = self.footer_at(r)
            && c < self.dims().1
        {
            return self.footer_cell_view(k, c);
        }
        if self.is_placeholder(r, c) {
            // The layer view shows the rules set on cells beyond the data.
            if let Some(layer) = self.solo_rule_layer() {
                let rules = self.literal_rules_for(&layer, (self.view.sheet, r, c));
                if !rules.is_empty() {
                    let fg = if layer == COMMENTS { "magenta" } else { "yellow" };
                    return CellView {
                        text: rules.join(" · "),
                        align: Align::Left,
                        style: CellStyle { fg: Some(Color::Named(fg.into())), ..CellStyle::default() },
                        error: false,
                        violation: false,
                    };
                }
            }
            return CellView {
                text: String::new(),
                align: Align::Left,
                style: CellStyle::default(),
                error: false,
                violation: false,
            };
        }
        if self.view.pivot.is_none() {
            return self.table_cell_view((self.view.sheet, r, c));
        }
        let last = self.wb.sheets[self.view.sheet].columns.len() - 1;
        let matches = self.pivot_matches(r, c);
        match matches.as_slice() {
            [] => CellView {
                text: String::new(),
                align: Align::Right,
                style: CellStyle::default(),
                error: false,
                violation: false,
            },
            [row] => self.table_cell_view((self.view.sheet, *row, last)),
            rows => {
                // Several rows share these keys: show their sum.
                let mut sum: Option<Value> = None;
                for &row in rows {
                    sum = Some(match (sum, self.wb.value((self.view.sheet, row, last))) {
                        (None, v) => v,
                        (Some(Value::Num(a, ua)), Value::Num(b, ub)) if ua == ub => Value::Num(a + b, ua),
                        _ => Value::err("cannot aggregate"),
                    });
                }
                let v = sum.unwrap_or(Value::Null);
                CellView {
                    text: format!("Σ {}", v.display(None, self.wb.decimal_comma)),
                    align: Align::Right,
                    style: CellStyle::default(),
                    error: v.is_error(),
                    violation: false,
                }
            }
        }
    }

    /// A footer cell: its value in bold, aligned like the column.
    fn footer_cell_view(&mut self, k: usize, c: usize) -> CellView {
        let id = (self.view.sheet, FOOTER_ROW + k, in_layer(c, self.shown_layer()));
        let v = self.wb.value(id);
        let text = if v.is_error() { "#ERR".to_string() } else { self.wb.display(id) };
        let col_align = self.table().columns.get(c).map(|c| c.align).unwrap_or_default();
        let natural = if matches!(v, Value::Num(..) | Value::Date(_)) { Align::Right } else { Align::Left };
        let align = if col_align != Align::None { col_align } else { natural };
        CellView { text, align, style: CellStyle { bold: true, ..CellStyle::default() }, error: v.is_error(), violation: false }
    }

    fn table_cell_view(&mut self, id: CellId) -> CellView {
        let z = if id.0 == self.view.sheet { self.shown_layer() } else { 0 };
        self.table_cell_view_in(id, z)
    }

    /// The cell `id` as shown in slice / scenario `z` of its table.
    fn table_cell_view_in(&mut self, id: CellId, z: usize) -> CellView {
        if let Some(layer) = self.solo.clone() {
            return self.solo_cell_view(&layer, id);
        }
        // Formats, comments and checks belong to the cell in every slice.
        let vid = (id.0, id.1, in_layer(id.2, z));
        let v = self.wb.value(vid);
        let mut style =
            if self.layer_visible_in(FORMAT, id.0) { self.formats.style(&mut self.wb, id) } else { CellStyle::default() };
        let own = |app: &Self| {
            tables(&app.doc).nth(id.0).and_then(|t| t.layers.get(z.wrapping_sub(1))).and_then(|l| l.rows.get(id.1)).and_then(|r| r.get(id.2)).cloned().unwrap_or_default()
        };
        let inherited = z > 0 && own(self).trim().is_empty();
        let is_formula = if z > 0 && !inherited {
            own(self).trim_start().starts_with('=')
        } else {
            matches!(self.wb.sheets[id.0].cells[id.1][id.2], crate::engine::Cell::Formula { .. })
        };
        if is_formula && self.layer_visible_in(FORMULAS, id.0) {
            // The formulas layer lies on top of `format` so formulas stay recognizable.
            let marker = if self.formula_formats.is_empty() {
                CellStyle { fg: Some(Color::Named("cyan".into())), italic: true, ..CellStyle::default() }
            } else {
                self.formula_formats.style(&mut self.wb, id)
            };
            style.merge(&marker);
        }
        if !self.comments_for(id).is_empty() {
            style.underline = true;
        }
        // Values a scenario takes from layer a are shown dimmed.
        if inherited && !is_formula && style.fg.is_none() && !matches!(v, Value::Null) {
            style.fg = Some(Color::Named("darkgray".into()));
        }
        let text = match &v {
            Value::Error(_) => "#ERR".to_string(),
            _ => style.display(&v, self.wb.decimal_comma).unwrap_or_else(|| self.wb.display(vid)),
        };
        let col_align = tables(&self.doc).nth(id.0).and_then(|t| t.columns.get(id.2)).map(|c| c.align).unwrap_or_default();
        let natural = if matches!(v, Value::Num(..) | Value::Date(_)) { Align::Right } else { Align::Left };
        let align = style.align.or((col_align != Align::None).then_some(col_align)).unwrap_or(natural);
        CellView { text, align, error: v.is_error(), violation: self.violations.contains_key(&id), style }
    }

    /// Normalized selection rectangle ((top, left), (bottom, right)).
    pub fn selection(&self) -> ((usize, usize), (usize, usize)) {
        let a = self.view.anchor.unwrap_or(self.view.cursor);
        let b = self.view.cursor;
        let ((r0, c0), (r1, c1)) = ((a.0.min(b.0), a.1.min(b.1)), (a.0.max(b.0), a.1.max(b.1)));
        let (rows, cols) = self.dims();
        match self.on_header() {
            Some(Header::Column) => ((0, c0), (rows.saturating_sub(1), c1)),
            Some(Header::Row) => ((r0, 0), (r1, cols.saturating_sub(1))),
            Some(Header::Corner) => ((0, 0), (rows.saturating_sub(1), cols.saturating_sub(1))),
            None => ((r0, c0), (r1, c1)),
        }
    }

    /// Whether more than the cursor cell is selected.
    pub fn has_range(&self) -> bool {
        self.view.anchor.is_some() || self.on_header().is_some()
    }

    /// The header the cursor is on (tables only, not in the pivot view).
    pub fn on_header(&self) -> Option<Header> {
        self.view.header.filter(|_| self.view.kind == ViewKind::Table && self.view.pivot.is_none())
    }

    // ------------------------------------------------------------ types view

    pub fn type_rows(&self) -> Vec<TypeRow> {
        let mut rows = Vec::new();
        for (b, block) in self.doc.blocks.iter().enumerate() {
            let Block::Types(lines) = block else { continue };
            for (l, text) in lines.iter().enumerate() {
                let t = text.trim();
                if t.is_empty() || t.starts_with('#') {
                    continue;
                }
                let (name, def) = t.split_once('=').map(|(n, d)| (n.trim(), d.trim())).unwrap_or((t, ""));
                rows.push(TypeRow::User { block: b, line: l, name: name.into(), def: def.into() });
            }
        }
        rows.push(TypeRow::New);
        rows.extend(BUILTINS.iter().map(|&(name, desc)| TypeRow::Builtin { name, desc }));
        rows
    }

    /// `Table.Column` list of the columns declared with type `name`.
    fn type_usage(&self, name: &str) -> String {
        let mut uses = Vec::new();
        for (i, t) in tables(&self.doc).enumerate() {
            for c in &t.columns {
                if c.ty.as_deref().map(str::trim) == Some(name) {
                    let table = t.name.clone().unwrap_or_else(|| format!("#{}", i + 1));
                    uses.push(format!("{table}.{}", c.name));
                }
            }
        }
        uses.join(", ")
    }

    fn types_cell_view(&self, r: usize, c: usize) -> CellView {
        let mut style = CellStyle::default();
        let mut error = false;
        let text = match self.type_rows().get(r) {
            Some(TypeRow::User { name, def, .. }) => match c {
                0 => {
                    style.bold = true;
                    name.clone()
                }
                1 => def.clone(),
                5 => self.type_usage(name),
                _ => match self.wb.types.resolve(def) {
                    Ok(t) => match c {
                        // Short form; the full enum/record definition is in column Definition.
                        2 => match &t.base {
                            Base::Enum(v) => format!("enum ({} values)", v.len()),
                            Base::Record(f) => format!("record ({} fields)", f.len()),
                            b => b.to_string(),
                        },
                        3 if t.unit.is_none() => String::new(),
                        3 => t.unit.to_string(),
                        _ => t.check.unwrap_or_default(),
                    },
                    Err(_) => {
                        error = true;
                        if c == 2 { "#ERR".into() } else { String::new() }
                    }
                },
            },
            Some(TypeRow::Builtin { name, desc }) => {
                style.fg = Some(Color::Named("darkgray".into()));
                match c {
                    0 => name.to_string(),
                    1 => desc.to_string(),
                    _ => String::new(),
                }
            }
            _ => String::new(),
        };
        CellView { text, align: Align::Left, style, error, violation: false }
    }

    pub fn toggle_types(&mut self) {
        let kind = if self.types_view() { ViewKind::Table } else { ViewKind::Types };
        self.set_view(kind, self.view.sheet);
    }

    /// Checks a new type name; `current` is the name being renamed, if any.
    fn check_type_name(&self, name: &str, current: Option<&str>) -> Result<(), String> {
        let valid = name.chars().next().is_some_and(char::is_alphabetic)
            && name.chars().all(|c| c.is_alphanumeric() || c == '_');
        if !valid {
            return Err(format!("'{name}' is not a valid type name (letters, digits, _)"));
        }
        if is_builtin(name) {
            return Err(format!("'{name}' is a built-in type"));
        }
        let taken = self.type_rows().iter().any(|r| matches!(r, TypeRow::User { name: n, .. } if n == name));
        if taken && current != Some(name) {
            return Err(format!("type '{name}' already exists"));
        }
        Ok(())
    }

    fn commit_type(&mut self, text: String) {
        let (r, c) = self.view.cursor;
        let text = text.trim().to_string();
        match self.type_rows().get(r).cloned() {
            Some(TypeRow::User { block, line, name, def }) => {
                let new_line = match c {
                    0 if text.is_empty() => return self.error("a type needs a name (F8 deletes the type)"),
                    0 => {
                        if let Err(e) = self.check_type_name(&text, Some(&name)) {
                            return self.error(e);
                        }
                        format!("{text} = {def}")
                    }
                    _ => format!("{name} = {text}"),
                };
                self.with_doc(|doc| {
                    if let Block::Types(lines) = &mut doc.blocks[block] {
                        lines[line] = new_line;
                    }
                });
                if c == 0 && text != name {
                    self.info(format!("renamed type {name} → {text} (columns using {name} are not updated)"));
                }
            }
            Some(TypeRow::New) if !text.is_empty() => {
                if let Err(e) = self.check_type_name(&text, None) {
                    return self.error(e);
                }
                let line = format!("{text} = text");
                self.with_doc(|doc| add_type_line(doc, line));
                // The new type takes the placeholder's row; continue with its definition.
                self.view.cursor = (r, 1);
                self.skip_move = true;
                self.info(format!("type {text} added – now enter its definition"));
            }
            _ => {}
        }
    }

    fn delete_type(&mut self) {
        match self.type_rows().get(self.view.cursor.0).cloned() {
            Some(TypeRow::User { block, line, name, .. }) => {
                self.with_doc(|doc| {
                    if let Block::Types(lines) = &mut doc.blocks[block] {
                        lines.remove(line);
                    }
                });
                self.info(format!("deleted type {name}"));
            }
            _ => self.error("only your own types can be deleted"),
        }
    }

    // ------------------------------------------------------------ printing

    /// The function-key bar: what F1 … F10 do right now, in the order in
    /// which the keys are handled (the topmost dialog first).
    pub fn fkeys(&self) -> [(u8, &'static str); 10] {
        let labels = self.fkey_labels();
        std::array::from_fn(|i| (i as u8 + 1, labels[i]))
    }

    fn fkey_labels(&self) -> FKeys {
        if self.key_test.is_some() {
            return [""; 10];
        }
        if self.context.is_some() {
            return ["Help", "", "", "", "", "", "", "", "Close", "Close"];
        }
        if self.show_help {
            return ["Close", "", "Prev", "Next", "", "", "", "", "KeyTest", "Close"];
        }
        if self.confirm.is_some() {
            return ["", "Save", "", "", "", "", "", "Discard", "", "Cancel"];
        }
        if self.menu.is_some() {
            return ["Help", "", "", "", "", "", "", "", "Close", "Close"];
        }
        if let Some(d) = &self.file_dialog {
            let mut labels = d.fkeys();
            labels[0] = "Help";
            return labels;
        }
        if self.paper_menu.is_some() {
            return ["", "", "", "", "", "", "", "", "", "Close"];
        }
        if self.preview.is_some() {
            return PREVIEW_FKEYS;
        }
        match self.mode {
            Mode::Edit { .. } => return ["Help", "Accept", "", "", "", "", "", "", "", "Cancel"],
            // In the go-to prompt a second F4 jumps.
            Mode::Command { ref buf } if buf.starts_with("goto ") => return ["Help", "Run", "", "Jump↗", "", "", "", "", "", "Cancel"],
            Mode::Command { .. } => return ["Help", "Run", "", "", "", "", "", "", "", "Cancel"],
            Mode::Normal => {}
        }
        let main = self.main_view();
        let focused = self.view.is_data();
        let (goto, ins_row, ins_col, sort, delete) = match main.kind {
            ViewKind::Layout(_) => return LAYOUT_FKEYS,
            ViewKind::Types | ViewKind::Functions => ("", "New", "", "", "Delete"),
            ViewKind::Calc => ("", "InsLine", "", "", "DelLine"),
            _ if main.pivot.is_some() => ("", "", "", "", ""),
            _ => {
                // The cursor decides what F8 deletes and where a second F7 leads.
                let header = main.header;
                let footer = focused && self.cursor_footer().is_some();
                let again = self.last_sort == Some((main.sheet, main.cursor.1, false, false));
                let delete = match header {
                    Some(Header::Column) => "DelCol",
                    Some(Header::Corner) => "",
                    _ if footer => "DelFoot",
                    _ => "DelRow",
                };
                let goto = if self.cursor_on_link() { "Link↗" } else { "Goto" };
                (goto, "InsRow", "InsCol", if again { "Sort↓" } else { "Sort" }, delete)
            }
        };
        ["Help", "Save", "Open", goto, ins_row, ins_col, sort, delete, "Menu", "Quit"]
    }

    /// The left (main) window's view.
    pub fn main_view(&self) -> &View {
        match &self.other {
            Some(other) if !self.left_focused => other,
            _ => &self.view,
        }
    }

    /// First `# ` heading of the document, else the file name.
    pub fn doc_title(&self) -> String {
        for block in &self.doc.blocks {
            if let Block::Text(lines) = block
                && let Some(h) = lines.iter().find_map(|l| l.strip_prefix("# "))
            {
                return h.trim().to_string();
            }
        }
        self.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "untitled".into())
    }

    /// Comments on cell `id` (empty while the comments layer is hidden).
    pub fn comments_for(&mut self, id: CellId) -> Vec<String> {
        if !self.layer_visible_in(COMMENTS, id.0) {
            return Vec::new();
        }
        let mut out = Vec::new();
        for (sheet, target, text) in &self.comment_rules {
            if *sheet == id.0 && target.matches(&mut self.wb, id) {
                out.push(text.clone());
            }
        }
        out
    }

    /// What to print, in document order. `all`: the whole document (text,
    /// tables, charts, code) as flow layout; otherwise the current table with
    /// its charts. Tables look as on screen; charts with errors are left out.
    pub fn print_items(&mut self, all: bool) -> Vec<PrintItem> {
        let mut items = Vec::new();
        let (mut sheet, mut chart, mut footnote) = (0, 0, 0);
        // Calculation sheets print with the way to each result.
        let mut calc = crate::sheet::render_blocks(&self.doc.clone(), &mut self.wb, crate::sheet::Render::default());
        for (i, block) in self.doc.blocks.clone().into_iter().enumerate() {
            match block {
                Block::Table(_) => {
                    if all || sheet == self.view.sheet {
                        // Each table as last shown: its slice or scenario.
                        let z = if sheet == self.view.sheet { self.shown_layer() } else { self.levels.get(&sheet).copied().unwrap_or(0) };
                        let mut table = self.print_table(sheet, z, &mut footnote);
                        self.drop_hidden_columns(&mut table, sheet);
                        items.push(PrintItem::Table(table));
                    }
                    sheet += 1;
                }
                Block::Chart { .. } => {
                    if let Ok(data) = self.chart_data(chart)
                        && (all || data.sheet == self.view.sheet)
                    {
                        items.push(PrintItem::Chart(data));
                    }
                    chart += 1;
                }
                // Live values in text refer to the table above.
                Block::Text(lines) if all => {
                    let context = sheet.saturating_sub(1);
                    let lines = lines.iter().map(|l| crate::text::live_values(l, &mut self.wb, context)).collect();
                    items.push(PrintItem::Text(lines));
                }
                // The standard PDF fonts have no Greek letters, ≤ or ✓.
                Block::Code { .. } if all && calc.contains_key(&i) => {
                    let lines = calc.remove(&i).unwrap_or_default();
                    items.push(PrintItem::Code(lines.iter().map(|l| crate::sheet::expr::latin1(l)).collect()));
                }
                Block::Code { body, .. } if all => items.push(PrintItem::Code(body)),
                _ => {}
            }
        }
        items
    }

    /// One table for printing; comments become numbered footnotes.
    /// Table `s` for printing, its slice or scenario `z` (named in the title).
    fn print_table(&mut self, s: usize, z: usize, footnote: &mut usize) -> PrintTable {
        // Printing always shows the data, also from the exclusive layer view.
        let solo = self.solo.take();
        let z = z.min(self.wb.sheets[s].layers.len());
        let table = self.print_table_inner(s, z, footnote);
        self.solo = solo;
        table
    }

    /// Takes the columns the view of table `s` hides out of a printed table
    /// (while the view layer is switched on), with their footnotes.
    fn drop_hidden_columns(&self, t: &mut PrintTable, s: usize) {
        let Some(view) = self.column_views.get(&s).filter(|v| v.hidden_count() > 0 && self.layer_visible_in(VIEW, s)) else { return };
        let keep = |c: &usize| !view.is_hidden(*c);
        let strip = |cells: &mut Vec<PrintCell>| {
            let mut c = 0;
            cells.retain(|_| {
                c += 1;
                keep(&(c - 1))
            });
        };
        let mut c = 0;
        t.headers.retain(|_| {
            c += 1;
            keep(&(c - 1))
        });
        t.rows.iter_mut().for_each(strip);
        let (headers, rows) = (&t.headers, &t.rows);
        t.footnotes.retain(|note| {
            let marker = note.split_whitespace().next().unwrap_or_default();
            headers.iter().any(|h| h.ends_with(marker)) || rows.iter().flatten().any(|c| c.text.ends_with(marker))
        });
    }

    /// The data rows of table `s` that are printed on level `z`: all, or
    /// those its filter shows (while the filter layer is switched on).
    fn print_rows(&self, s: usize, z: usize) -> Vec<usize> {
        let rows = self.wb.sheets[s].cells.len();
        let filtered = self.hidden_rows.get(&(s, z)).filter(|_| self.layer_visible_in(FILTER, s));
        let element = self.print_where.as_ref().filter(|(sheet, _)| *sheet == s).map(|(_, out)| out);
        (0..rows).filter(|&r| filtered.is_none_or(|h| h.get(r) != Some(&true)) && element.is_none_or(|out| out.get(r) != Some(&true))).collect()
    }

    fn print_table_inner(&mut self, s: usize, z: usize, footnote: &mut usize) -> PrintTable {
        let sheet = &self.wb.sheets[s];
        let mut title = sheet.name.clone().unwrap_or_else(|| format!("Table {}", s + 1));
        if !sheet.layers.is_empty() {
            title = format!("{title} · {}", self.level_title(s, z));
        }
        let sheet = &self.wb.sheets[s];
        let mut headers: Vec<String> = sheet.columns.iter().map(|c| c.name.clone()).collect();
        let (rows, cols) = (sheet.cells.len(), sheet.columns.len());
        // Rows the filter hides are not printed; the bands alternate over the others.
        let kept = self.print_rows(s, z);
        let place: HashMap<usize, usize> = kept.iter().enumerate().map(|(i, &r)| (r, i)).collect();
        let mut rows: Vec<Vec<PrintCell>> = (0..rows)
            .map(|r| {
                let band = place.get(&r).copied().unwrap_or(r);
                (0..cols)
                    .map(|c| {
                        let v = self.table_cell_view_in((s, r, c), z);
                        let mut color = if v.error { Some((190, 0, 0)) } else { v.style.fg.as_ref().and_then(print::print_color) };
                        let mut bg = v.style.bg.as_ref().and_then(print::print_bg);
                        // As on screen: banded rows, and a rule violation in red.
                        if bg.is_none() && self.bands && let Some(ratatui::style::Color::Rgb(r, g, b)) = tui_band(band) {
                            bg = Some((r, g, b));
                        }
                        if v.violation {
                            bg = Some((250, 205, 205));
                            color = Some((150, 0, 0));
                        }
                        let style = SpanStyle { bold: v.style.bold, italic: v.style.italic, color, shade: false, bg, wrap: v.style.wrap };
                        PrintCell { text: v.text, align: v.align, style }
                    })
                    .collect()
            })
            .collect();
        let mut footnotes = Vec::new();
        if self.layer_visible_in(COMMENTS, s) {
            let rules: Vec<(layers::Target, String)> =
                self.comment_rules.iter().filter(|(sh, _, _)| *sh == s).map(|(_, t, x)| (t.clone(), x.clone())).collect();
            for (target, text) in rules {
                *footnote += 1;
                let marker = crate::text::superscript(*footnote);
                // A column comment marks the header, others their first cell.
                let column = match &target {
                    layers::Target::Column(name) => self.wb.sheets[s].columns.iter().position(|c| &c.name == name).or_else(|| {
                        (name.len() == 1 && name.chars().all(|c| c.is_ascii_uppercase()))
                            .then(|| (name.as_bytes()[0] - b'A') as usize)
                            .filter(|&c| c < cols)
                    }),
                    _ => None,
                };
                if let Some(c) = column {
                    headers[c].push_str(&marker);
                } else if let Some((r, c)) =
                    kept.iter().flat_map(|&r| (0..cols).map(move |c| (r, c))).find(|&(r, c)| target.matches(&mut self.wb, (s, r, c)))
                {
                    rows[r][c].text.push_str(&marker);
                } else {
                    // Nothing printed carries this comment: its number is free again.
                    *footnote -= 1;
                    continue;
                }
                footnotes.push(format!("{marker} {text}"));
            }
        }
        if kept.len() < rows.len() {
            let mut r = 0;
            rows.retain(|_| {
                r += 1;
                place.contains_key(&(r - 1))
            });
        }
        // Footer rows in bold below the data; the label in the first column.
        for k in 0..self.wb.sheets[s].footer.len() {
            let mut line: Vec<PrintCell> = (0..cols)
                .map(|c| {
                    let id = (s, FOOTER_ROW + k, in_layer(c, z));
                    let v = self.wb.value(id);
                    let text = if v.is_error() { "#ERR".to_string() } else { self.wb.display(id) };
                    let align = if matches!(v, Value::Num(..) | Value::Date(_)) { Align::Right } else { Align::Left };
                    let color = v.is_error().then_some((190, 0, 0));
                    PrintCell { text, align, style: SpanStyle { bold: true, italic: false, color, shade: false, bg: None, wrap: None } }
                })
                .collect();
            if let Some(first) = line.first_mut() {
                let label = &self.wb.sheets[s].footer[k].label;
                first.text = if first.text.is_empty() { label.clone() } else { format!("{label}: {}", first.text) };
            }
            rows.push(line);
        }
        let footer_rows = self.wb.sheets[s].footer.len();
        PrintTable { title, headers, rows, footnotes, footer_rows }
    }

    pub fn layout(&mut self, setup: &PageSetup) -> Layout {
        match setup.scope {
            Scope::Layout(i) => self.layout_parts(i).2,
            _ => self.flow_layout(setup),
        }
    }

    /// A raster layout as shown and printed: its elements where they end up
    /// (see [`App::resolved_layout`]), their content, and the pages.
    pub fn layout_parts(&mut self, i: usize) -> (RasterSpec, Vec<PlacedElement>, Layout) {
        let (spec, placed) = self.resolved_layout(i);
        let pages = print::compose(&spec.setup(i), &placed, &self.page_footer());
        (spec, placed, pages)
    }

    fn page_footer(&self) -> String {
        let days = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| (d.as_secs() / 86_400) as i64)
            .unwrap_or(0);
        let file = self.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        format!("{file} · {}", Value::Date(days).display(None, false))
    }

    fn flow_layout(&mut self, setup: &PageSetup) -> Layout {
        let footer = self.page_footer();
        match setup.scope {
            // The whole document brings its own headings.
            Scope::Document | Scope::Layout(_) => {
                let items = self.print_items(true);
                print::paginate("", &items, setup, &footer)
            }
            Scope::Table => {
                let items = self.print_items(false);
                print::paginate(&self.doc_title(), &items, setup, &footer)
            }
        }
    }

    /// Human-readable name of a print scope.
    pub fn scope_name(&self, scope: Scope) -> String {
        match scope {
            Scope::Table => "current table".into(),
            Scope::Document => "whole document".into(),
            Scope::Layout(i) => format!("layout {}", self.layout_blocks().get(i).map(|(n, _)| n.as_str()).unwrap_or("?")),
        }
    }

    /// `budget.hcmd` → `budget.pdf` next to it.
    pub fn pdf_path(&self) -> PathBuf {
        if self.is_untitled() {
            return files::untitled_pdf().to_path_buf();
        }
        let name = self.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let stem = crate::storage::document_stem(&name);
        self.path.with_file_name(format!("{stem}.pdf"))
    }

    /// Writes the PDF for `setup`; returns its path and page count.
    pub fn write_pdf(&mut self, setup: &PageSetup, out: Option<&Path>) -> Result<(PathBuf, usize), String> {
        let layout = self.layout(setup);
        let path = out.map(Path::to_path_buf).unwrap_or_else(|| self.pdf_path());
        let bytes = print::to_pdf(&layout, &self.doc_title());
        std::fs::write(&path, bytes).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
        Ok((path, layout.pages.len()))
    }

    fn export_pdf(&mut self, out: Option<&Path>) -> Option<PathBuf> {
        let setup = self.print_setup.clone();
        match self.write_pdf(&setup, out) {
            Ok((path, pages)) => {
                self.info(format!("PDF written: {} ({pages} page{})", path.display(), if pages == 1 { "" } else { "s" }));
                Some(path)
            }
            Err(e) => {
                self.error(e);
                None
            }
        }
    }

    pub fn open_preview(&mut self) {
        let setup = self.print_setup.clone();
        let layout = self.layout(&setup);
        // A preview opens in the normal view, whatever the last one showed.
        if let Some(before) = self.image_before_zoom.take() {
            self.image_mode = before;
        }
        self.preview = Some(Preview { layout, page: 0, scroll: (0, 0), zoom: 0 });
        self.message = None;
    }

    fn relayout(&mut self) {
        let setup = self.print_setup.clone();
        let layout = self.layout(&setup);
        if let Some(p) = &mut self.preview {
            p.page = p.page.min(layout.pages.len() - 1);
            p.layout = layout;
        }
    }

    /// The layout whose page the paper menu changes: the one previewed or
    /// edited; `None` means the page setup of tables and documents.
    fn paper_target(&self) -> Option<usize> {
        match (&self.preview, self.print_setup.scope, self.view.kind) {
            (Some(_), Scope::Layout(i), _) => Some(i),
            (Some(_), _, _) => None,
            (None, _, ViewKind::Layout(i)) => Some(i),
            _ => None,
        }
    }

    /// Opens the help on the page that fits the current view.
    pub fn open_help(&mut self) {
        self.help_page = match (self.preview.is_some(), self.main_view().kind) {
            (true, _) | (false, ViewKind::Layout(_)) => 3,
            (false, ViewKind::Types | ViewKind::Functions | ViewKind::Calc) => 1,
            _ if self.solo.is_some() => 2,
            _ => 0,
        };
        self.show_help = true;
    }

    /// Keys while the help is shown: page through it or close it.
    fn key_help(&mut self, key: KeyEvent) {
        let pages = super::help::PAGES.len() + 1;
        // On the Keys page: the key test.
        if key.code == KeyCode::Char('t') && self.help_page == pages - 1 {
            return self.start_key_test();
        }
        match key.code {
            KeyCode::F(9) => self.start_key_test(),
            KeyCode::Right | KeyCode::PageDown | KeyCode::Down | KeyCode::Tab | KeyCode::Char(' ') | KeyCode::F(4) => {
                self.help_page = (self.help_page + 1) % pages;
            }
            KeyCode::Left | KeyCode::PageUp | KeyCode::Up | KeyCode::BackTab | KeyCode::F(3) => {
                self.help_page = (self.help_page + pages - 1) % pages;
            }
            KeyCode::Home => self.help_page = 0,
            KeyCode::End => self.help_page = pages - 1,
            KeyCode::Char(c @ '1'..='9') if (c as usize - '1' as usize) < pages => self.help_page = c as usize - '1' as usize,
            KeyCode::Esc | KeyCode::F(1) | KeyCode::F(10) | KeyCode::Enter | KeyCode::Char('q') => self.show_help = false,
            _ => {}
        }
    }

    /// Paper and orientation currently in effect for the paper menu.
    pub fn current_paper(&self) -> (print::Paper, bool) {
        match self.paper_target() {
            Some(i) => {
                let spec = self.raster_spec(i);
                (spec.paper, spec.landscape)
            }
            None => (self.print_setup.paper, self.print_setup.landscape),
        }
    }

    pub fn open_paper_menu(&mut self) {
        let current = self.current_paper();
        self.paper_menu = Some(print::paper_choices().iter().position(|c| *c == current).unwrap_or(0));
    }

    fn apply_paper(&mut self, n: usize) {
        self.paper_menu = None;
        let Some(&(paper, landscape)) = print::paper_choices().get(n) else { return };
        match self.paper_target() {
            Some(i) => {
                let page = format!("{} {}", paper.name(), if landscape { "landscape" } else { "portrait" });
                self.set_layout_setting(i, "page", page);
            }
            None => {
                self.print_setup.paper = paper;
                self.print_setup.landscape = landscape;
            }
        }
        if self.preview.is_some() {
            self.relayout();
        }
        self.info(format!("paper: {}", paper.label(landscape)));
    }

    fn key_paper_menu(&mut self, key: KeyEvent) {
        let n = print::paper_choices().len();
        let Some(sel) = self.paper_menu else { return };
        match key.code {
            KeyCode::Up => self.paper_menu = Some((sel + n - 1) % n),
            KeyCode::Down => self.paper_menu = Some((sel + 1) % n),
            KeyCode::Enter | KeyCode::Char(' ') => self.apply_paper(sel),
            KeyCode::Esc | KeyCode::F(10) => self.paper_menu = None,
            _ => {}
        }
    }

    fn preview_fkey(&mut self, n: u8) {
        // A raster layout keeps its own page settings in its block.
        if let Scope::Layout(i) = self.print_setup.scope
            && matches!(n, 5 | 7 | 8)
        {
            self.layout_page_key(i, n);
            return self.relayout();
        }
        match n {
            1 => self.open_help(),
            2 => {
                self.export_pdf(None);
            }
            3 => {
                if let Some(path) = self.export_pdf(None) {
                    match open_external(&path) {
                        Ok(()) => self.info(format!("opened {}", path.display())),
                        Err(e) => self.error(format!("cannot open viewer: {e}")),
                    }
                }
            }
            4 => {
                if let Some(path) = self.export_pdf(None) {
                    match print_file(&path) {
                        Ok(out) => self.info(format!("sent to printer: {out}")),
                        Err(e) => self.error(e),
                    }
                }
            }
            5 => {
                self.print_setup.landscape = !self.print_setup.landscape;
                self.relayout();
            }
            6 => self.open_paper_menu(),
            7 | 8 => {
                let current = self.preview.as_ref().map(|p| p.layout.font_size).unwrap_or(9.0);
                let step = if n == 7 { -1.0 } else { 1.0 };
                self.print_setup.font_size = Some((current.round() + step).clamp(print::MIN_FONT, 24.0));
                self.relayout();
            }
            9 => {
                let layouts = self.layout_blocks().len();
                self.print_setup.scope = match self.print_setup.scope {
                    Scope::Table => Scope::Document,
                    Scope::Document if layouts > 0 => Scope::Layout(0),
                    Scope::Layout(i) if i + 1 < layouts => Scope::Layout(i + 1),
                    _ => Scope::Table,
                };
                self.relayout();
            }
            10 => self.preview = None,
            _ => {}
        }
    }

    /// `i` in the print preview: image (or, without an image protocol, a
    /// block-graphics overview) or text grid.
    pub fn toggle_image_mode(&mut self) {
        self.image_mode = !self.image_mode;
    }

    /// The preview page scaled to `w` × `h` pixels for block graphics
    /// (each cell shows two pixels, one above the other).
    pub fn preview_thumbnail(&mut self, w: usize, h: usize) -> Result<super::graphics::RgbImage, String> {
        use std::hash::{Hash, Hasher};
        let pdf = self.preview_page_pdf().ok_or("no preview")?;
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        (&pdf, w, h).hash(&mut hasher);
        let key = hasher.finish();
        if let Some((k, img)) = &self.thumbnail
            && *k == key
        {
            return Ok(img.clone());
        }
        // Render larger than needed; averaging down keeps thin lines visible.
        let img = super::graphics::rasterize_rgb(&pdf, (w as u32 * 4).clamp(300, 1600))?.resize(w, h);
        self.thumbnail = Some((key, img.clone()));
        Ok(img)
    }

    /// The current preview page as a one-page PDF (input for the image preview).
    pub fn preview_page_pdf(&self) -> Option<Vec<u8>> {
        let p = self.preview.as_ref()?;
        if p.zoom > 0 {
            let (across, down) = p.grid();
            return Some(print::to_pdf_sheet(&p.layout, p.page, across, down, &self.doc_title()));
        }
        let page = Layout {
            pages: vec![p.layout.pages[p.page].clone()],
            graphics: vec![p.layout.graphics[p.page].clone()],
            ..p.layout.clone()
        };
        Some(print::to_pdf(&page, &self.doc_title()))
    }

    /// `-` / `+` in the print preview: zooms out (`by` > 0) to show more pages
    /// side by side, or in; the overview is drawn as an image, since the text
    /// grid cannot shrink.
    pub fn zoom_preview(&mut self, by: isize) {
        let Some(p) = &mut self.preview else { return };
        p.zoom = (p.zoom as isize + by).clamp(0, PREVIEW_ZOOMS.len() as isize - 1) as usize;
        if p.zoom > 0 {
            // The view before the overview comes back with zoom 0.
            if self.image_before_zoom.is_none() {
                self.image_before_zoom = Some(self.image_mode);
            }
            self.image_mode = true;
        } else if let Some(before) = self.image_before_zoom.take() {
            self.image_mode = before;
        }
    }

    /// `i` in the print preview: back from the overview to the normal text
    /// view, else switches between image and text.
    fn preview_view_key(&mut self) {
        if self.preview.as_ref().is_some_and(|p| p.zoom > 0) {
            if let Some(p) = &mut self.preview {
                p.zoom = 0;
            }
            self.image_before_zoom = None;
            self.image_mode = false;
        } else {
            self.toggle_image_mode();
        }
    }

    fn key_preview(&mut self, key: KeyEvent) {
        self.message = None;
        if key.code == KeyCode::Char('i') {
            return self.preview_view_key();
        }
        let Some(p) = &mut self.preview else { return };
        let pages = p.layout.pages.len();
        let group = if p.zoom > 0 { p.grid().0 * p.grid().1 } else { 1 };
        match key.code {
            KeyCode::Char('+' | '=') => self.zoom_preview(-1),
            KeyCode::Char('-' | '_') => self.zoom_preview(1),
            KeyCode::Char('0') => self.zoom_preview(isize::MIN / 2),
            KeyCode::Esc => self.preview = None,
            KeyCode::F(n) => self.preview_fkey(n),
            KeyCode::Char('q') if key.modifiers.contains(KeyModifiers::CONTROL) => self.quit_key(),
            KeyCode::PageDown | KeyCode::Char(' ') => {
                p.page = (p.page + group).min(pages - 1);
                p.scroll.1 = 0;
            }
            KeyCode::PageUp => {
                p.page = p.page.saturating_sub(group);
                p.scroll.1 = 0;
            }
            KeyCode::Home => p.page = 0,
            KeyCode::End => p.page = pages - 1,
            KeyCode::Down => p.scroll.1 += 1,
            KeyCode::Up => p.scroll.1 = p.scroll.1.saturating_sub(1),
            KeyCode::Right => p.scroll.0 += 4,
            KeyCode::Left => p.scroll.0 = p.scroll.0.saturating_sub(4),
            _ => {}
        }
    }

    // ------------------------------------------------------------ raster layouts

    /// (name, lines) of all ```layout blocks in document order.
    pub fn layout_blocks(&self) -> Vec<(String, Vec<String>)> {
        self.doc
            .blocks
            .iter()
            .filter_map(|b| if let Block::Layout { name, lines } = b { Some((name.clone(), lines.clone())) } else { None })
            .collect()
    }

    pub fn raster_spec(&self, i: usize) -> RasterSpec {
        RasterSpec::parse(&self.layout_blocks().get(i).map(|(_, l)| l.clone()).unwrap_or_default())
    }

    /// The layout as it is shown and printed, with the content of its elements.
    ///
    /// Elements with `height: auto` take the rows their content needs, from
    /// top to bottom; what lies below keeps its distance (see [`reflow`]). An
    /// element pushed over a page end this way starts on the next page. Without
    /// `height: auto` this is the layout as written.
    pub fn resolved_layout(&mut self, i: usize) -> (RasterSpec, Vec<PlacedElement>) {
        let mut spec = self.raster_spec(i);
        let setup = spec.setup(i);
        let (cols, _, _, _) = print::page_grid(&setup, spec.font);
        let body = print::layout_rows_per_page(&setup);
        let mut autos: Vec<usize> = (0..spec.elements.len()).filter(|&n| spec.elements[n].flows()).collect();
        if !autos.is_empty() {
            let written = spec.elements.clone();
            autos.sort_by_key(|&n| written[n].rows.0);
            for n in autos {
                let e = spec.elements[n].clone();
                let height = self.place_element(&spec, &e, cols, body, &mut 0).height;
                let grown = Element { rows: (e.rows.0, e.rows.0 + height - 1), ..e };
                spec.elements = reflow(&spec.elements, n, &grown);
            }
            // Pushed over a page end: on to the next page (if it fits on one).
            for _ in 0..spec.elements.len() {
                let cut = |m: &usize| {
                    let e = &spec.elements[*m];
                    !e.flows() && e.rows != written[*m].rows && e.height() <= body && (e.rows.0 - 1) / body != (e.rows.1 - 1) / body
                };
                let Some(m) = (0..spec.elements.len()).filter(cut).min_by_key(|&m| spec.elements[m].rows.0) else { break };
                let e = spec.elements[m].clone();
                let top = ((e.rows.0 - 1) / body + 1) * body + 1;
                let moved = Element { rows: (top, top + e.height() - 1), ..e };
                spec.elements = reflow(&spec.elements, m, &moved);
            }
        }
        let mut footnote = 0;
        let placed: Vec<PlacedElement> = spec.elements.clone().iter().map(|e| self.place_element(&spec, e, cols, body, &mut footnote)).collect();
        for (e, p) in spec.elements.iter_mut().zip(&placed) {
            e.rows.1 = e.rows.0 + p.height - 1;
        }
        (spec, placed)
    }

    /// The elements of layout `i` where they are shown (see [`App::resolved_layout`]).
    pub fn shown_spec(&mut self, i: usize) -> RasterSpec {
        let spec = self.raster_spec(i);
        if spec.elements.iter().any(Element::flows) { self.resolved_layout(i).0 } else { spec }
    }

    /// Reduces a table to the rows its element asks for (`rows:`).
    fn pick_rows(&mut self, t: &mut PrintTable, s: usize, z: usize, pick: &RowPick) -> Result<(), String> {
        let data = t.rows.len() - t.footer_rows;
        let keep: Vec<usize> = match pick {
            RowPick::All => return Ok(()),
            RowPick::First(n) => (0..data.min(*n)).collect(),
            RowPick::Last(n) => (data.saturating_sub(*n)..data).collect(),
            RowPick::Total => Vec::new(),
            RowPick::Ranked { n, by, bottom } => {
                let c = self.wb.sheets[s].columns.iter().position(|c| c.name == *by).ok_or_else(|| format!("rows: no column '{by}' to rank by"))?;
                // Position in the printed rows, ranked by the value of its table row.
                let source = self.print_rows(s, z);
                let mut ranked: Vec<(usize, f64)> = (0..data.min(source.len()))
                    .filter_map(|r| match self.wb.value((s, source[r], in_layer(c, z))) {
                        Value::Num(x, _) => Some((r, x)),
                        Value::Date(d) => Some((r, d as f64)),
                        _ => None,
                    })
                    .collect();
                ranked.sort_by(|a, b| if *bottom { a.1.total_cmp(&b.1) } else { b.1.total_cmp(&a.1) });
                ranked.into_iter().take(*n).map(|(r, _)| r).collect()
            }
        };
        // The title says that this is an excerpt.
        match pick {
            RowPick::First(n) if *n < data => t.title.push_str(&format!(" · first {n} of {data}")),
            RowPick::Last(n) if *n < data => t.title.push_str(&format!(" · last {n} of {data}")),
            RowPick::Ranked { .. } => t.title.push_str(&format!(" · {}", pick.text().unwrap_or_default())),
            _ => {}
        }
        let footer = t.rows.split_off(data);
        let rows = std::mem::take(&mut t.rows);
        t.rows = keep.iter().map(|&r| rows[r].clone()).chain(footer).collect();
        // Footnotes of rows that are not shown go with them.
        let (headers, rows) = (&t.headers, &t.rows);
        t.footnotes.retain(|note| {
            let marker = note.split_whitespace().next().unwrap_or_default();
            headers.iter().any(|h| h.ends_with(marker)) || rows.iter().flatten().any(|c| c.text.ends_with(marker))
        });
        Ok(())
    }

    /// The content of an element in its area. An element with `height: auto`
    /// gets the height its content needs from its first row on.
    fn place_element(&mut self, spec: &RasterSpec, e: &Element, cols: usize, body: usize, footnote: &mut usize) -> PlacedElement {
        use crate::text::{inline, live_values, wrap};
        let (x, w) = spec.span(cols, e);
        let flows = e.flows();
        // Unlimited for now; the real height follows from the content.
        let h = if flows { usize::MAX } else { e.height() };
        let line = |kind, pieces: &[crate::text::Styled], base: Option<&SpanStyle>| print::styled_line(kind, pieces, base);
        let error = |msg: String| {
            let red = SpanStyle { color: Some((190, 0, 0)), ..SpanStyle::default() };
            Placed::Lines(vec![print::Line { kind: print::LineKind::Row, spans: vec![print::Span { col: 0, text: format!("⚠ {msg}"), style: red }], width: 0 }])
        };
        let mut hidden = 0;
        // Lines repeated on every page a flowing element runs on to.
        let mut head = 0;
        let content = match e.kind {
            ElementKind::Heading | ElementKind::Text | ElementKind::Note => {
                let text = live_values(&e.content, &mut self.wb, 0);
                let kind = if e.kind == ElementKind::Heading { print::LineKind::Title } else { print::LineKind::Row };
                let note = SpanStyle { shade: true, color: Some((60, 60, 60)), ..SpanStyle::default() };
                let base = (e.kind == ElementKind::Note).then_some(&note);
                let mut lines: Vec<print::Line> = wrap(&inline(&text), w, 0).iter().map(|l| line(kind, l, base)).collect();
                if e.kind == ElementKind::Note {
                    // A note is a shaded box over its whole area.
                    let h = if flows { lines.len().max(1) } else { h };
                    lines.resize_with(h, || print::Line { kind: print::LineKind::Row, spans: Vec::new(), width: 0 });
                    for l in &mut lines {
                        let used: usize = l.spans.iter().map(|s| s.text.width()).sum();
                        l.spans.push(print::Span { col: used, text: " ".repeat(w.saturating_sub(used)), style: note.clone() });
                        for s in &mut l.spans {
                            s.style.italic = true;
                        }
                    }
                }
                Placed::Lines(lines)
            }
            ElementKind::Value => {
                let value = live_values(&format!("{{={}}}", e.content.trim_start_matches('=')), &mut self.wb, 0);
                let gray = SpanStyle { color: Some((100, 100, 100)), ..SpanStyle::default() };
                let bold = SpanStyle { bold: true, ..SpanStyle::default() };
                let mut lines = Vec::new();
                if let Some(label) = &e.label {
                    lines.push(print::Line { kind: print::LineKind::Row, spans: vec![print::Span { col: 0, text: label.clone(), style: gray }], width: 0 });
                }
                lines.push(print::Line { kind: print::LineKind::Row, spans: vec![print::Span { col: 0, text: value, style: bold }], width: 0 });
                Placed::Lines(lines)
            }
            ElementKind::Table => match self.wb.sheet_index(&e.content) {
                // One slice or scenario per element: `z: February` (or a letter); a without.
                Some(sheet) => {
                    let z = match &e.z {
                        None => Some(0),
                        Some(z) => self.wb.sheets[sheet].layer_named_or_lettered(z),
                    };
                    match z {
                        Some(z) => {
                            // `where: Status = open`: only the rows that meet it.
                            let condition = e.filter.as_ref().map(|text| layers::row_condition(&self.wb, sheet, text));
                            if let Some(Ok((col, checks))) = &condition {
                                self.print_where = Some((sheet, layers::failing_rows(&mut self.wb, sheet, z, *col, checks)));
                            }
                            // Bands follow the rows shown, not the rows of the table.
                            let bands = std::mem::take(&mut self.bands);
                            let mut t = self.print_table(sheet, z, footnote);
                            self.bands = bands;
                            if let Some(text) = &e.filter {
                                t.title.push_str(&format!(" · {text}"));
                            }
                            let where_error = condition.and_then(|c| c.err()).map(|msg| format!("where: {msg}"));
                            // Without `columns:` the element shows what the view shows.
                            if e.columns.is_empty() {
                                self.drop_hidden_columns(&mut t, sheet);
                            }
                            let picked = match where_error {
                                Some(msg) => Err(msg),
                                None => self.pick_rows(&mut t, sheet, z, &e.pick),
                            };
                            self.print_where = None;
                            match picked {
                                Ok(()) => {
                                    let data = t.rows.len() - t.footer_rows;
                                    for (r, row) in t.rows.iter_mut().take(data).enumerate() {
                                        if let (true, Some(ratatui::style::Color::Rgb(r, g, b))) = (bands, tui_band(r)) {
                                            row.iter_mut().filter(|c| c.style.bg.is_none()).for_each(|c| c.style.bg = Some((r, g, b)));
                                        }
                                    }
                                    let (lines, cut) = print::table_lines(&t, &e.columns, w, h);
                                    hidden = cut;
                                    head = 2;
                                    Placed::Lines(lines)
                                }
                                Err(msg) => error(msg),
                            }
                        }
                        None => error(format!("'{}' has no slice or scenario '{}'", e.content, e.z.clone().unwrap_or_default())),
                    }
                }
                None => error(format!("unknown table '{}'", e.content)),
            },
            ElementKind::Chart => match self.chart_blocks().iter().position(|(n, _)| *n == e.content) {
                Some(c) => match self.chart_data(c) {
                    Ok(data) => Placed::Chart(data),
                    Err(err) => error(format!("chart {}: {err}", e.content)),
                },
                None => error(format!("unknown chart '{}'", e.content)),
            },
        };
        let row = e.rows.0 - 1;
        let (height, flow) = match (&content, flows) {
            (Placed::Lines(lines), true) => (print::flow_rows(row, lines.len(), head, body).len().max(1), Some(head)),
            _ => (e.height(), None),
        };
        PlacedElement { row, col: x, width: w, height, content, flow, hidden }
    }

    /// Replaces the lines of layout block `i` (one undoable step).
    fn edit_layout(&mut self, i: usize, f: impl FnOnce(&mut Vec<String>)) {
        self.layout_rows.clear();
        self.with_doc(|doc| {
            if let Some(Block::Layout { lines, .. }) = doc.blocks.iter_mut().filter(|b| matches!(b, Block::Layout { .. })).nth(i) {
                f(lines);
            }
        });
    }

    /// Writes the elements of layout `i` back (in the order of its block),
    /// with `extra` as a new last element: one undoable step.
    fn write_elements(&mut self, i: usize, elements: &[Element], extra: Option<&Element>) {
        let spec = self.raster_spec(i);
        let changes: Vec<(usize, String)> = elements
            .iter()
            .enumerate()
            .filter(|(m, e)| spec.elements.get(*m) != Some(*e))
            .filter_map(|(m, e)| Some((*spec.element_lines.get(m)?, e.to_line())))
            .collect();
        if changes.is_empty() && extra.is_none() {
            return;
        }
        self.edit_layout(i, |lines| {
            for (line, text) in changes {
                lines[line] = text;
            }
            lines.extend(extra.map(Element::to_line));
        });
    }

    /// Changes element `n` of layout `i`. The change starts from the layout
    /// as shown: where `height: auto` moved elements, their rows are written
    /// down first, so that the editor changes what is on screen.
    fn update_element(&mut self, i: usize, n: usize, f: impl FnOnce(&mut Element)) {
        let mut elements = self.shown_spec(i).elements;
        let Some(e) = elements.get_mut(n) else { return };
        f(e);
        self.write_elements(i, &elements, None);
    }

    /// Sets `key: value` among the layout's settings (inserted at the top if missing).
    fn set_layout_setting(&mut self, i: usize, key: &str, value: String) {
        self.edit_layout(i, |lines| {
            let line = format!("{key}: {value}");
            match lines.iter_mut().find(|l| l.trim_start().starts_with(&format!("{key}:"))) {
                Some(l) => *l = line,
                None => lines.insert(0, line),
            }
        });
    }

    /// F5, F7, F8 in the preview of a layout: orientation and font size.
    fn layout_page_key(&mut self, i: usize, n: u8) {
        let mut spec = self.raster_spec(i);
        match n {
            5 => spec.landscape = !spec.landscape,
            7 => spec.font = (spec.font - 1.0).max(4.0),
            _ => spec.font = (spec.font + 1.0).min(24.0),
        }
        if n == 5 {
            self.set_layout_setting(i, "page", spec.page_line()["page: ".len()..].to_string());
        } else {
            self.set_layout_setting(i, "font", format!("{}", spec.font));
        }
    }

    /// The selected element of the layout in the focused window.
    fn selected_element(&self) -> Option<(usize, usize, Element)> {
        let ViewKind::Layout(i) = self.view.kind else { return None };
        let n = self.view.cursor.0;
        self.raster_spec(i).elements.get(n).cloned().map(|e| (i, n, e))
    }

    /// The selected element where it is shown (see [`App::resolved_layout`]).
    fn shown_element(&mut self) -> Option<(usize, usize, Element)> {
        let ViewKind::Layout(i) = self.view.kind else { return None };
        let n = self.view.cursor.0;
        self.shown_spec(i).elements.get(n).cloned().map(|e| (i, n, e))
    }

    /// `:import file`: adds the sheets of a spreadsheet as tables (with their
    /// comments layers) at the end of the document.
    fn import_sheets(&mut self, path: &Path) {
        let taken: Vec<String> = self.wb.sheets.iter().filter_map(|s| s.name.clone()).collect();
        match crate::import::import_file_avoiding(path, &taken) {
            Ok((imported, report)) => {
                let first_new = self.wb.sheets.len();
                // The imported title block is left out; tables and their layers are added.
                self.with_doc(|doc| doc.blocks.extend(imported.blocks.into_iter().filter(|b| !matches!(b, Block::Text(_)))));
                self.switch_sheet(first_new);
                self.info(format!("imported {report}"));
            }
            Err(e) => self.error(e),
        }
    }

    /// `:layout [Name]`: a new layout with all tables (charts beside them).
    fn create_layout(&mut self, name: Option<&str>) {
        let existing: Vec<String> = self.layout_blocks().into_iter().map(|(n, _)| n).collect();
        let name = match name {
            Some(n) => n.to_string(),
            None => (1..).map(|k| if k == 1 { "Report".to_string() } else { format!("Report {k}") }).find(|n| !existing.contains(n)).unwrap(),
        };
        if existing.contains(&name) {
            return self.error(format!("layout {name} already exists"));
        }
        let mut lines = vec!["page: A4 portrait".to_string(), "font: 9".to_string(), "grid: 12".to_string()];
        // Tables are as tall as their data: what follows moves along when they grow.
        let el = |cols, rows, kind, content: String| Element { cols, rows, kind, content, auto: kind == ElementKind::Table, ..Element::default() }.to_line();
        lines.push(el((1, 12), (1, 2), ElementKind::Heading, self.doc_title()));
        let mut row = 4;
        let charts: Vec<(String, Option<usize>)> =
            (0..self.chart_blocks().len()).map(|c| (self.chart_blocks()[c].0.clone(), self.chart_data(c).ok().map(|d| d.sheet))).collect();
        for (s, sheet) in self.wb.sheets.iter().enumerate() {
            let Some(table) = sheet.name.clone() else { continue };
            let h = (sheet.cells.len() + sheet.footer.len() + 2).clamp(4, 20);
            let chart = charts.iter().find(|(_, sheet)| *sheet == Some(s)).map(|(n, _)| n.clone());
            match chart {
                Some(chart) => {
                    let h = h.max(12);
                    lines.push(el((1, 6), (row, row + h - 1), ElementKind::Table, table));
                    lines.push(el((7, 12), (row, row + h - 1), ElementKind::Chart, chart));
                    row += h + 1;
                }
                None => {
                    lines.push(el((1, 12), (row, row + h - 1), ElementKind::Table, table));
                    row += h + 1;
                }
            }
        }
        let index = existing.len();
        self.with_doc(|doc| doc.blocks.push(Block::Layout { name: name.clone(), lines }));
        self.show_helper(ViewKind::Layout(index));
        self.info(format!("layout '{name}' created – drag elements with the mouse, Space shows the overview"));
    }

    /// Adds an element below all others (or in `rect`) and selects it.
    fn add_element(&mut self, i: usize, kind: ElementKind, content: String, rect: Option<((usize, usize), (usize, usize))>) {
        let spec = self.shown_spec(i);
        let ((c1, c2), (r1, r2)) = rect.unwrap_or_else(|| {
            let row = spec.last_row() + 2;
            ((1, spec.grid), (row, row + 1))
        });
        self.push_element(i, Element { cols: (c1, c2), rows: (r1, r2), kind, content, ..Element::default() });
    }

    /// Adds `e` as the last element of layout `i` and selects it.
    fn push_element(&mut self, i: usize, e: Element) {
        let spec = self.shown_spec(i);
        let n = spec.elements.len();
        self.write_elements(i, &spec.elements, Some(&e));
        self.view.cursor = (n, 0);
        self.view.follow = true;
    }

    fn key_layout(&mut self, i: usize, key: KeyEvent) -> bool {
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        // macOS Terminal.app sends ⌥← / ⌥→ as ESC b / ESC f ("word back / forward").
        let code = match key.code {
            KeyCode::Char('b') if alt => KeyCode::Left,
            KeyCode::Char('f') if alt => KeyCode::Right,
            c => c,
        };
        let count = self.raster_spec(i).elements.len();
        let step = |key: KeyCode| match key {
            KeyCode::Left => (-1, 0),
            KeyCode::Right => (1, 0),
            KeyCode::Up => (0, -1),
            _ => (0, 1),
        };
        if self.layout_grab {
            // Picked up: arrows move, Shift+arrows or + − > < resize.
            let resize = match code {
                KeyCode::Char('+') => Some((0, 1)),
                KeyCode::Char('-') => Some((0, -1)),
                KeyCode::Char('>') => Some((1, 0)),
                KeyCode::Char('<') => Some((-1, 0)),
                KeyCode::Left | KeyCode::Right | KeyCode::Up | KeyCode::Down if shift => Some(step(code)),
                _ => None,
            };
            match code {
                _ if resize.is_some() => {
                    let (dc, dr) = resize.unwrap();
                    self.resize_element(i, self.view.cursor.0, dc, dr);
                    self.view.follow = true;
                    return true;
                }
                KeyCode::Enter | KeyCode::Esc => {
                    self.drop_element();
                    return true;
                }
                KeyCode::Char('=') => {
                    self.fit_element(i, self.view.cursor.0);
                    self.view.follow = true;
                    return true;
                }
                KeyCode::Char('a') => {
                    self.toggle_auto(i, self.view.cursor.0);
                    self.view.follow = true;
                    return true;
                }
                KeyCode::Char(' ') => {
                    self.toggle_overview();
                    return true;
                }
                KeyCode::Left | KeyCode::Right | KeyCode::Up | KeyCode::Down => {
                    self.move_element(i, self.view.cursor.0, step(code));
                    return true;
                }
                // Anything else puts the element down first.
                _ => self.layout_grab = false,
            }
        }
        if code == KeyCode::Esc {
            return false;
        }
        match code {
            // Alt+arrows move directly (where the terminal sends them).
            KeyCode::Left | KeyCode::Right | KeyCode::Up | KeyCode::Down if alt => {
                self.move_element(i, self.view.cursor.0, step(code));
            }
            // Arrows (also with Shift) jump to the nearest element in that direction.
            KeyCode::Left | KeyCode::Right | KeyCode::Up | KeyCode::Down => {
                let spec = self.shown_spec(i);
                if let Some(n) = neighbour(&spec, self.view.cursor.0, step(code)) {
                    self.view.cursor.0 = n;
                }
            }
            // Tab/BackTab are taken by the split; PageUp/PageDown select elements.
            KeyCode::PageDown if count > 0 => self.view.cursor.0 = (self.view.cursor.0 + 1) % count,
            KeyCode::PageUp if count > 0 => self.view.cursor.0 = (self.view.cursor.0 + count - 1) % count,
            KeyCode::Enter => self.start_edit(None),
            KeyCode::Delete | KeyCode::Backspace => self.delete_element(),
            KeyCode::Insert => {
                self.add_element(i, ElementKind::Text, String::new(), None);
                self.start_edit(None);
            }
            KeyCode::Char(' ') => self.toggle_overview(),
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => self.start_edit(Some(c)),
            _ => return false,
        }
        self.view.follow = true;
        true
    }

    /// Moves element `n` by one grid column / row.
    fn move_element(&mut self, i: usize, n: usize, (dc, dr): (i32, i32)) {
        let grid = self.raster_spec(i).grid;
        self.update_element(i, n, |e| {
            let (w, h) = (e.cols.1 - e.cols.0, e.rows.1 - e.rows.0);
            e.cols.0 = ((e.cols.0 as i32 + dc).max(1) as usize).min(grid - w);
            e.cols.1 = e.cols.0 + w;
            e.rows.0 = (e.rows.0 as i32 + dr).max(1) as usize;
            e.rows.1 = e.rows.0 + h;
        });
        self.view.follow = true;
    }

    /// Resizes element `n` at its bottom-right corner. When it gets taller or
    /// shorter, the elements below it in the grid move along (one undoable step).
    fn resize_element(&mut self, i: usize, n: usize, dc: i32, dr: i32) {
        let spec = self.shown_spec(i);
        let Some(old) = spec.elements.get(n).cloned() else { return };
        let mut resized = old.clone();
        resized.cols.1 = ((resized.cols.1 as i32 + dc).max(resized.cols.0 as i32) as usize).min(spec.grid);
        resized.rows.1 = (resized.rows.1 as i32 + dr).max(resized.rows.0 as i32) as usize;
        // A height set by hand is a fixed height again.
        if dr != 0 && std::mem::take(&mut resized.auto) {
            self.info("height is fixed now – a makes it automatic again");
        }
        self.write_elements(i, &reflow(&spec.elements, n, &resized), None);
    }

    /// `=` while moving: the element becomes as tall as its content needs
    /// (what lies below moves along).
    fn fit_element(&mut self, i: usize, n: usize) {
        let spec = self.shown_spec(i);
        let Some(old) = spec.elements.get(n).cloned() else { return };
        if old.kind == ElementKind::Chart {
            return self.error("a chart has no height of its own – resize it with + and −");
        }
        let setup = spec.setup(i);
        let (cols, _, _, _) = print::page_grid(&setup, spec.font);
        let body = print::layout_rows_per_page(&setup);
        let probe = Element { auto: true, ..old.clone() };
        let height = self.place_element(&spec, &probe, cols, body, &mut 0).height;
        let fitted = Element { rows: (old.rows.0, old.rows.0 + height - 1), ..old };
        self.write_elements(i, &reflow(&spec.elements, n, &fitted), None);
        self.info(format!("{height} rows – as tall as the content"));
    }

    /// `a` while moving: `height: auto` on / off.
    fn toggle_auto(&mut self, i: usize, n: usize) {
        let Some(e) = self.shown_spec(i).elements.get(n).cloned() else { return };
        if e.kind == ElementKind::Chart {
            return self.error("a chart has no height of its own");
        }
        self.update_element(i, n, |e| e.auto = !e.auto);
        self.info(if e.auto {
            "height is fixed now".to_string()
        } else {
            format!("height: auto – the {} grows and shrinks with its content, what lies below moves along", e.kind.name())
        });
    }

    /// Space or `:overview`: the layout editor's overview on / off.
    pub fn toggle_overview(&mut self) {
        self.layout_overview = !self.layout_overview;
        self.view.follow = true;
        self.info(if self.layout_overview {
            "overview: every element as a box, a page per screen – Space returns to the page as printed"
        } else {
            "the page as printed – Space shows the overview"
        });
    }

    fn drop_element(&mut self) {
        self.layout_grab = false;
        self.info("element put down");
    }

    fn delete_element(&mut self) {
        let Some((i, n, e)) = self.selected_element() else { return self.error("no element selected") };
        let line = self.raster_spec(i).element_lines[n];
        self.edit_layout(i, |lines| {
            lines.remove(line);
        });
        self.view.cursor.0 = n.saturating_sub(1);
        self.info(format!("deleted {} element", e.kind.name()));
    }

    fn layout_fkey(&mut self, i: usize, n: u8) {
        match n {
            3 if self.layout_grab => self.drop_element(),
            3 => match self.selected_element() {
                Some((_, _, e)) => {
                    self.layout_grab = true;
                    self.info(format!("moving the {} – arrows move, + − > < (or Shift+arrows) resize, = fits the height, a automatic height, Enter/Esc/F3 put it down", e.kind.name()));
                }
                None => self.error("no element selected"),
            },
            4 => self.start_edit(None),
            5 => {
                let Some((_, _, e)) = self.shown_element() else { return self.error("no element selected") };
                let row = self.shown_spec(i).last_row() + 2;
                let h = e.height();
                // The copy shows the same columns, rows and slice.
                self.push_element(i, Element { rows: (row, row + h - 1), ..e });
            }
            6 => {
                let Some((_, n, e)) = self.selected_element() else { return self.error("no element selected") };
                let k = ElementKind::ALL.iter().position(|k| *k == e.kind).unwrap();
                let kind = ElementKind::ALL[(k + 1) % ElementKind::ALL.len()];
                let tables: Vec<String> = self.wb.sheets.iter().filter_map(|s| s.name.clone()).collect();
                let charts: Vec<String> = self.chart_blocks().into_iter().map(|(n, _)| n).collect();
                let content = match kind {
                    ElementKind::Table if !tables.contains(&e.content) => tables.first().cloned().unwrap_or_default(),
                    ElementKind::Chart if !charts.contains(&e.content) => charts.first().cloned().unwrap_or_default(),
                    _ => e.content.clone(),
                };
                self.update_element(i, n, |e| {
                    e.kind = kind;
                    e.content = content;
                });
                self.info(format!("element is now: {}", kind.name()));
            }
            7 => self.open_paper_menu(),
            8 => self.delete_element(),
            _ => {}
        }
    }

    fn layout_mouse(&mut self, i: usize, m: MouseEvent, geom: LayoutGeom) {
        let (col, row) = geom.cell_at(m.column, m.row);
        let spec = self.shown_spec(i);
        match m.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                self.layout_grab = false;
                // By canvas line: in the overview a line stands for several rows.
                let line = geom.line_of(row);
                let hit = spec
                    .elements
                    .iter()
                    .rposition(|e| (e.cols.0..=e.cols.1).contains(&col) && (geom.line_of(e.rows.0)..=geom.line_of(e.rows.1)).contains(&line));
                let now = Instant::now();
                let double = self.last_click.is_some_and(|(t, x, y)| now.duration_since(t) < DOUBLE_CLICK && (x, y) == (m.column, m.row));
                self.last_click = Some((now, m.column, m.row));
                self.layout_drag = Some(match hit {
                    Some(n) => {
                        self.view.cursor.0 = n;
                        if double {
                            self.start_edit(None);
                        }
                        let e = &spec.elements[n];
                        let mode = if (col, line) == (e.cols.1, geom.line_of(e.rows.1)) {
                            DragMode::Resize(n)
                        } else {
                            DragMode::Move(n, (col - e.cols.0, row as i64 - e.rows.0 as i64))
                        };
                        LayoutDrag { mode, cols: e.cols, rows: e.rows }
                    }
                    None => LayoutDrag { mode: DragMode::Create((col, row)), cols: (col, col), rows: (row, row) },
                });
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                let Some(d) = &mut self.layout_drag else { return };
                match d.mode {
                    DragMode::Move(_, (dc, dr)) => {
                        let (w, h) = (d.cols.1 - d.cols.0, d.rows.1 - d.rows.0);
                        let c0 = col.saturating_sub(dc).max(1).min(spec.grid - w);
                        let r0 = (row as i64 - dr).max(1) as usize;
                        d.cols = (c0, c0 + w);
                        d.rows = (r0, r0 + h);
                    }
                    DragMode::Resize(_) => {
                        d.cols.1 = col.max(d.cols.0);
                        d.rows.1 = geom.line_end(row).max(d.rows.0);
                    }
                    DragMode::Create((c, r)) => {
                        d.cols = (c.min(col), c.max(col));
                        d.rows = (r.min(row), geom.line_end(r.max(row)));
                    }
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                let Some(d) = self.layout_drag.take() else { return };
                match d.mode {
                    DragMode::Move(n, _) | DragMode::Resize(n) => {
                        if (d.cols, d.rows) != (spec.elements[n].cols, spec.elements[n].rows) {
                            // A height dragged by hand is a fixed height again.
                            let fixed = d.rows.1 - d.rows.0 != spec.elements[n].rows.1 - spec.elements[n].rows.0;
                            self.update_element(i, n, |e| {
                                e.cols = d.cols;
                                e.rows = d.rows;
                                e.auto &= !fixed;
                            });
                        }
                    }
                    DragMode::Create(_) => {
                        // A drawn rectangle becomes a text element; type its content right away.
                        self.add_element(i, ElementKind::Text, String::new(), Some((d.cols, d.rows)));
                        self.start_edit(None);
                        self.info("new text element – F6 changes its kind (heading, value, note, table, chart)");
                    }
                }
            }
            MouseEventKind::ScrollDown => self.scroll_lines(3),
            MouseEventKind::ScrollUp => self.scroll_lines(-3),
            _ => {}
        }
    }

    // ------------------------------------------------------------ exclusive layer view

    /// Whether the UI is drawn for a light terminal background.
    pub fn light(&self) -> bool {
        match self.theme {
            super::theme::ThemeSetting::Light => true,
            super::theme::ThemeSetting::Dark => false,
            super::theme::ThemeSetting::Auto => self.detected_light.unwrap_or(false),
        }
    }

    /// `:layer new <name>`: a layer block for the current table (unless it
    /// has one), then the layer view to fill in its rules.
    fn new_layer(&mut self, name: &str) {
        if self.view.kind != ViewKind::Table || self.view.pivot.is_some() {
            return self.error(":layer new works in tables");
        }
        if name.starts_with('@') || name.contains(['`', '@']) {
            return self.error(format!("'{name}' is not a valid layer name"));
        }
        let sheet = self.view.sheet;
        let (found, insert_at) = self.layer_block(name, sheet);
        if found.is_none() {
            let layer = crate::model::Layer { line: Default::default(), name: name.to_string(), target: None, rules: Vec::new() };
            self.with_doc(|doc| {
                let at = insert_at.unwrap_or(doc.blocks.len()).min(doc.blocks.len());
                doc.blocks.insert(at, Block::Layer(layer));
            });
        }
        self.hidden_layers.remove(name);
        self.layer_overrides.retain(|(_, l), _| l != name);
        self.set_solo(Some(name.to_string()));
        if found.is_some() {
            self.info(format!("layer {name} exists for this table – layer view: typing edits its rules · Esc back"));
        } else if name == FORMULAS {
            // set_solo explained the view already.
        } else if name == VIEW {
            self.info("layer view added – type into a column: hide, width 20, freeze (the columns up to it stay while scrolling) · Esc back");
        } else if name == FILTER {
            self.info("layer filter added – type a condition into a column: = open, >= 100, in(a, b), contains(text), empty · Esc back");
        } else if !matches!(name, FORMAT | VALIDATE | COMMENTS | LINKS | VIEW) {
            self.info(format!("layer {name} added – its rules are kept and shown, but only format, validate, filter, view, comments and links act · Esc back"));
        } else {
            self.info(format!("layer {name} added – type a rule into a cell or selection · Esc back"));
        }
    }

    pub fn set_solo(&mut self, layer: Option<String>) {
        self.info(match &layer {
            Some(l) if l == FORMULAS => format!("layer view: {l} – formulas, other cells show their type · Esc back"),
            Some(l) => format!("layer view: {l} – typing edits the {l} rule of the cell · Del removes it · Esc back"),
            None => "normal view".into(),
        });
        self.solo = layer;
    }

    /// The layer whose rules are edited in the exclusive view (`formulas`
    /// edits the cells themselves).
    fn solo_rule_layer(&self) -> Option<String> {
        self.solo.clone().filter(|l| l != FORMULAS && self.view.kind == ViewKind::Table && self.view.pivot.is_none())
    }

    /// Rule target for the cursor: the cell (`B2`) or the selected range (`B2:D4`).
    fn solo_target(&self) -> String {
        let a1 = |(r, c): (usize, usize)| format!("{}{}", col_letters(c), r + 1);
        // A filter or view rule belongs to a column.
        if let Some(layer @ (FILTER | VIEW)) = self.solo.as_deref() {
            return self.column_target(layer, self.view.cursor.1);
        }
        let (from, to) = self.selection();
        if from == to { a1(from) } else { format!("{}:{}", a1(from), a1(to)) }
    }

    /// Bodies of all rules of `layer` that apply to cell `id`.
    fn rules_for(&mut self, layer: &str, id: CellId) -> Vec<String> {
        let rules: Vec<(layers::Target, String)> = self
            .attached
            .iter()
            .filter(|a| a.layer.name == layer && a.sheet == id.0)
            .flat_map(|a| {
                a.layer.rules.iter().filter_map(|r| match r {
                    crate::model::Rule::Rule { target, body } => Some((layers::Target::parse(target), body.clone())),
                    crate::model::Rule::Raw(_) => None,
                })
            })
            .collect();
        rules.into_iter().filter(|(t, _)| t.matches(&mut self.wb, id)).map(|(_, b)| b).collect()
    }

    /// Bodies of the cell and range rules of `layer` that cover cell `id`;
    /// these also reach cells beyond the data.
    fn literal_rules_for(&self, layer: &str, id: CellId) -> Vec<String> {
        self.attached
            .iter()
            .filter(|a| a.layer.name == layer && a.sheet == id.0)
            .flat_map(|a| a.layer.rules.iter())
            .filter_map(|r| match r {
                crate::model::Rule::Rule { target, body } => match layers::Target::parse(target) {
                    layers::Target::Cell(r, c) if (r, c) == (id.1, id.2) => Some(body.clone()),
                    layers::Target::Range((c1, r1), (c2, r2))
                        if (r1.min(r2)..=r1.max(r2)).contains(&id.1) && (c1.min(c2)..=c1.max(c2)).contains(&id.2) =>
                    {
                        Some(body.clone())
                    }
                    _ => None,
                },
                crate::model::Rule::Raw(_) => None,
            })
            .collect()
    }

    /// The rule of `layer` for exactly `target` in table `sheet`, if any.
    fn layer_rule(&self, layer: &str, sheet: usize, target: &str) -> Option<String> {
        self.attached.iter().filter(|a| a.layer.name == layer && a.sheet == sheet).find_map(|a| {
            a.layer.rules.iter().find_map(|r| match r {
                crate::model::Rule::Rule { target: t, body } if t == target => Some(body.clone()),
                _ => None,
            })
        })
    }

    /// Sets (or with an empty `body` removes) the rule `target: body` in the
    /// `layer` block of table `sheet`, creating the block after the table if needed.
    /// The block of layer `layer` attached to table `sheet` (as
    /// `layers::attach` finds it), and where a new one would go: after the
    /// table and the layer blocks that follow it.
    fn layer_block(&self, layer: &str, sheet: usize) -> (Option<usize>, Option<usize>) {
        let mut table = None;
        let mut found = None;
        let mut insert_at = None;
        for (i, b) in self.doc.blocks.iter().enumerate() {
            match b {
                Block::Table(_) => {
                    table = Some(table.map_or(0, |t| t + 1));
                    if table == Some(sheet) {
                        insert_at = Some(i + 1);
                    }
                }
                Block::Layer(l) => {
                    let owner = match &l.target {
                        Some(name) => self.wb.sheet_index(name),
                        None => table,
                    };
                    if owner == Some(sheet) && l.name == layer && found.is_none() {
                        found = Some(i);
                    }
                    if insert_at == Some(i) {
                        insert_at = Some(i + 1);
                    }
                }
                _ => {}
            }
        }
        (found, insert_at)
    }

    fn set_layer_rule(&mut self, layer: &str, sheet: usize, target: &str, body: &str) {
        use crate::model::{Layer, Rule};
        let (found, insert_at) = self.layer_block(layer, sheet);
        if found.is_none() && body.is_empty() {
            return;
        }
        let (layer, target, body) = (layer.to_string(), target.to_string(), body.to_string());
        self.with_doc(|doc| {
            let index = match found {
                Some(i) => i,
                None => {
                    let at = insert_at.unwrap_or(doc.blocks.len());
                    doc.blocks.insert(at, Block::Layer(Layer { line: Default::default(), name: layer, target: None, rules: Vec::new() }));
                    at
                }
            };
            let Block::Layer(l) = &mut doc.blocks[index] else { return };
            let existing = l.rules.iter().position(|r| matches!(r, Rule::Rule { target: t, .. } if *t == target));
            match (existing, body.is_empty()) {
                (Some(k), true) => {
                    l.rules.remove(k);
                }
                (Some(k), false) => l.rules[k] = Rule::Rule { target, body },
                (None, false) => l.rules.push(Rule::Rule { target, body }),
                (None, true) => {}
            }
        });
    }

    /// A cell in the exclusive view of `layer`.
    fn solo_cell_view(&mut self, layer: &str, id: CellId) -> CellView {
        let dim = |text: String| CellView {
            text,
            align: Align::Left,
            style: CellStyle { fg: Some(Color::Named("darkgray".into())), ..CellStyle::default() },
            error: false,
            violation: false,
        };
        let shown = |text: String, style: CellStyle| CellView { text, align: Align::Left, style, error: false, violation: false };
        let data = |app: &mut App| app.wb.display(id);
        match layer {
            FORMULAS => {
                let raw = self.wb.sheets[id.0].raw[id.1][id.2].clone();
                if raw.starts_with('=') {
                    return shown(raw, CellStyle { fg: Some(Color::Named("cyan".into())), ..CellStyle::default() });
                }
                let declared = tables(&self.doc).nth(id.0).and_then(|t| t.columns.get(id.2)).and_then(|c| c.ty.clone());
                let kind = match self.wb.value(id) {
                    Value::Null => return dim(String::new()),
                    Value::Num(_, u) if u.is_none() => "number".to_string(),
                    Value::Num(_, u) => format!("number {u}"),
                    Value::Text(_) => "text".into(),
                    Value::Date(_) => "date".into(),
                    Value::Bool(_) => "bool".into(),
                    Value::Error(_) => "error".into(),
                    Value::Vector(_) => "vector".into(),
                };
                shown(declared.unwrap_or(format!("auto → {kind}")), CellStyle { fg: Some(Color::Named("gray".into())), italic: true, ..CellStyle::default() })
            }
            FORMAT => {
                let style = self.formats.style(&mut self.wb, id);
                let summary = style_summary(&style);
                if summary.is_empty() { dim(data(self)) } else { shown(summary, CellStyle { percent: None, decimals: None, align: None, ..style }) }
            }
            COMMENTS => {
                let notes = self.rules_for(COMMENTS, id);
                if notes.is_empty() { dim(data(self)) } else { shown(notes.join(" · "), CellStyle { fg: Some(Color::Named("magenta".into())), ..CellStyle::default() }) }
            }
            other => {
                let rules = self.rules_for(other, id);
                if rules.is_empty() {
                    return dim(data(self));
                }
                let mut view = shown(rules.join(" · "), CellStyle { fg: Some(Color::Named("yellow".into())), ..CellStyle::default() });
                if other == VALIDATE {
                    view.violation = self.violations.contains_key(&id);
                }
                view
            }
        }
    }

    // ------------------------------------------------------------ pivot

    /// Whether the current table can be shown as a pivot.
    pub fn can_pivot(&self) -> bool {
        self.view.kind == ViewKind::Table && self.tensor_axes().is_some()
    }

    fn tensor_axes(&self) -> Option<usize> {
        let s = &self.wb.sheets[self.view.sheet];
        match &s.shape {
            Shape::Tensor(axes) if axes.len() >= 2 && s.columns.len() == axes.len() + 1 => Some(axes.len()),
            _ => None,
        }
    }

    pub fn toggle_pivot(&mut self, axes: Option<(usize, usize)>) {
        if self.view.pivot.is_some() && axes.is_none() {
            self.view.pivot = None;
            self.view.cursor = (0, 0);
            self.view.anchor = None;
            self.view.header = None;
            return;
        }
        if self.types_view() {
            return self.error("pivot is not available in the types view (Esc to leave)");
        }
        let Some(n) = self.tensor_axes() else {
            self.error("pivot needs a tensor table (## Name :: tensor[A, B, …] with a value column)");
            return;
        };
        let (row_axis, col_axis) = axes.unwrap_or((0, 1));
        self.view.pivot = Some(Pivot { row_axis, col_axis, values: vec![Vec::new(); n], fixed: vec![0; n] });
        self.refresh_pivot();
        self.view.cursor = (0, 0);
        self.view.anchor = None;
        self.view.scroll = (0, 0);
    }

    fn refresh_pivot(&mut self) {
        let Some(n) = self.tensor_axes() else {
            self.view.pivot = None;
            return;
        };
        let raw = &self.wb.sheets[self.view.sheet].raw;
        let values: Vec<Vec<String>> = (0..n)
            .map(|a| {
                let mut v: Vec<String> = Vec::new();
                for row in raw {
                    let key = row[a].trim().to_string();
                    if !v.contains(&key) {
                        v.push(key);
                    }
                }
                v
            })
            .collect();
        if let Some(p) = &mut self.view.pivot {
            for (a, vals) in values.iter().enumerate() {
                p.fixed[a] = p.fixed[a].min(vals.len().saturating_sub(1));
            }
            p.values = values;
        }
    }

    fn pivot_matches(&self, r: usize, c: usize) -> Vec<usize> {
        let Some(p) = &self.view.pivot else { return Vec::new() };
        let raw = &self.wb.sheets[self.view.sheet].raw;
        let want: Vec<Option<&str>> = (0..p.values.len())
            .map(|a| {
                let idx = if a == p.row_axis {
                    r
                } else if a == p.col_axis {
                    c
                } else {
                    p.fixed[a]
                };
                p.values[a].get(idx).map(String::as_str)
            })
            .collect();
        if want.iter().any(Option::is_none) {
            return Vec::new();
        }
        (0..raw.len()).filter(|&i| want.iter().enumerate().all(|(a, w)| raw[i][a].trim() == w.unwrap())).collect()
    }

    /// Tab without split: rotate the pivot's dimensions, otherwise go to the
    /// next (Shift+Tab: previous) tab of the main window.
    fn tab_key(&mut self, back: bool) {
        if self.view.pivot.is_some() {
            return self.rotate_pivot(back);
        }
        let tabs = self.window_tabs(true);
        let Some(current) = tabs.iter().position(|(_, _, active)| *active) else { return };
        let n = tabs.len();
        let next = if back { (current + n - 1) % n } else { (current + 1) % n };
        match tabs[next].1 {
            Hit::Tab(i) => self.switch_sheet(i),
            Hit::TypesTab => self.set_view(ViewKind::Types, self.view.sheet),
            Hit::ViewTab(kind) => self.set_view(kind, self.view.sheet),
            _ => {}
        }
    }

    /// Tab in the pivot: the column axis becomes the row axis and the next
    /// filtered axis moves into the columns (with two axes: rows ↔ columns).
    /// Shift+Tab turns the other way.
    pub fn rotate_pivot(&mut self, back: bool) {
        let Some(p) = &mut self.view.pivot else { return };
        let n = p.values.len();
        let (r, c) = (p.row_axis, p.col_axis);
        let free = |from: usize, step: usize| (1..n).map(|k| (from + k * step) % n).find(|&a| a != r && a != c);
        (p.row_axis, p.col_axis) = if back { (free(r, n - 1).unwrap_or(c), r) } else { (c, free(c, 1).unwrap_or(r)) };
        self.view.cursor = (0, 0);
        self.view.anchor = None;
        self.view.scroll = (0, 0);
        let (rows, cols) = (self.axis_name(self.view.pivot.as_ref().unwrap().row_axis), self.axis_name(self.view.pivot.as_ref().unwrap().col_axis));
        self.info(format!("pivot: rows {rows} × columns {cols}"));
    }

    pub fn cycle_filter(&mut self, axis: usize) {
        if let Some(p) = &mut self.view.pivot {
            let n = p.values[axis].len().max(1);
            p.fixed[axis] = (p.fixed[axis] + 1) % n;
        }
    }

    // ------------------------------------------------------------ changes

    fn snapshot(&mut self) {
        self.record(Step::Doc(self.doc.clone()));
    }

    /// Keeps `doc` in sync with the workbook's raw cells after an edit.
    fn after_change(&mut self) {
        self.dirty = true;
        for (i, s) in self.wb.sheets.iter().enumerate() {
            if let Some(t) = table_mut(&mut self.doc, i) {
                t.rows = s.raw.clone();
            }
        }
        if self.view.pivot.is_some() {
            self.refresh_pivot();
        }
        self.revalidate();
    }

    /// Sets several raw cells of the current sheet as one undoable step.
    /// Writing beyond the data (into the placeholders) grows the table.
    pub fn set_cells(&mut self, cells: Vec<(usize, usize, String)>) {
        if self.footer_at(self.selection().0 .0).is_some() {
            return self.error("footer cells are typed one by one");
        }
        if self.shown_layer() > 0 {
            return self.set_layer_cells(self.shown_layer(), cells);
        }
        let (rows, cols) = self.dims();
        // Empty values outside the data would only create empty rows/columns.
        let mut cells: Vec<_> = cells.into_iter().filter(|(r, c, t)| (*r < rows && *c < cols) || !t.trim().is_empty()).collect();
        if cells.is_empty() {
            return;
        }
        self.snapshot();
        let need_rows = cells.iter().map(|(r, _, _)| r + 1).max().unwrap_or(0);
        // New rows get the formulas of the calculated columns.
        if need_rows > rows {
            let inherited = self.inherited_cells(rows, need_rows, &cells);
            cells.extend(inherited);
        }
        let need_cols = cells.iter().map(|(_, c, _)| c + 1).max().unwrap_or(0);
        if need_rows > rows || need_cols > cols {
            let calculates = self.calculates();
            if let Some(t) = table_mut(&mut self.doc, self.view.sheet) {
                grow(t, need_rows, need_cols);
                if calculates && need_rows > rows {
                    let names: Vec<String> = t.columns.iter().map(|c| c.name.clone()).collect();
                    calc_column::extend_layer_formulas(t, rows..need_rows, &names);
                }
            }
            self.rebuild();
        }
        for (r, c, text) in cells {
            self.wb.set_cell((self.view.sheet, r, c), &text);
        }
        self.after_change();
    }

    /// Change to the document structure, as one undoable step.
    fn with_doc(&mut self, f: impl FnOnce(&mut Document)) {
        self.snapshot();
        f(&mut self.doc);
        self.dirty = true;
        self.rebuild();
    }

    /// Structural change on the current table, as one undoable step.
    fn with_table(&mut self, f: impl FnOnce(&mut Table)) {
        let sheet = self.view.sheet;
        self.with_doc(|doc| {
            if let Some(t) = table_mut(doc, sheet) {
                f(t);
            }
        });
    }

    /// Ctrl+Z: takes back the last change, or goes back from the last jump.
    pub fn undo(&mut self) {
        match self.undo.pop() {
            Some(Step::Doc(doc)) => {
                self.redo.push(Step::Doc(std::mem::replace(&mut self.doc, doc)));
                self.dirty = true;
                self.rebuild();
                self.info("undone");
            }
            Some(Step::Jump { left, view }) => {
                let place = self.place(&view);
                let view = self.restore_view(left, view);
                self.redo.push(Step::Jump { left, view });
                self.info(format!("back to {place}"));
            }
            None => self.error("nothing to undo"),
        }
    }

    /// Ctrl+Y: does the change again, or the jump.
    pub fn redo(&mut self) {
        match self.redo.pop() {
            Some(Step::Doc(doc)) => {
                self.undo.push(Step::Doc(std::mem::replace(&mut self.doc, doc)));
                self.dirty = true;
                self.rebuild();
                self.info("redone");
            }
            Some(Step::Jump { left, view }) => {
                let place = self.place(&view);
                let view = self.restore_view(left, view);
                self.undo.push(Step::Jump { left, view });
                self.info(format!("on to {place}"));
            }
            None => self.error("nothing to redo"),
        }
    }

    /// Writes the cell at the cursor; in a pivot, a missing combination
    /// becomes a new row of the tensor.
    fn commit_value(&mut self, text: String) {
        // `/` at the start of an input opens the context menu; `'/` types it.
        let text = match text.strip_prefix("'/") {
            Some(rest) => format!("/{rest}"),
            None => text,
        };
        if self.on_header() == Some(Header::Corner) {
            return self.rename_table(text.trim());
        }
        if self.on_header() == Some(Header::Column) {
            return self.rename_column(text.trim());
        }
        if self.types_view() {
            return self.commit_type(text);
        }
        if self.functions_view() {
            return self.commit_function(text);
        }
        if self.calc_view() {
            return self.commit_calc(text);
        }
        if let Some(k) = self.cursor_footer() {
            return match self.on_header() {
                Some(Header::Row) => self.rename_footer(k, text.trim()),
                _ => self.set_footer_cell(k, self.view.cursor.1, &text),
            };
        }
        // The layer view never writes data, also not beyond the table's cells.
        if let Some(layer) = self.solo_rule_layer() {
            let target = self.solo_target();
            return self.set_layer_rule(&layer, self.view.sheet, &target, text.trim());
        }
        if let ViewKind::Layout(_) = self.view.kind {
            if let Some((i, n, mut e)) = self.selected_element() {
                // Content and options as in the file: `Account z: February rows: last 12`.
                match e.set_text(&text) {
                    Ok(()) => {
                        e.z = e.z.filter(|z| z != "a");
                        self.update_element(i, n, |old| {
                            (old.content, old.columns, old.label, old.z, old.pick, old.auto, old.filter) = (e.content, e.columns, e.label, e.z, e.pick, e.auto, e.filter)
                        });
                    }
                    Err(msg) => self.error(msg),
                }
            }
            return;
        }
        let (r, c) = self.view.cursor;
        if let Some(p) = self.view.pivot.clone() {
            match self.pivot_matches(r, c).as_slice() {
                [row] => {
                    let last = self.wb.sheets[self.view.sheet].columns.len() - 1;
                    self.set_cells(vec![(*row, last, text)]);
                }
                [] if text.trim().is_empty() => {}
                [] => {
                    let mut row: Vec<String> = (0..p.values.len())
                        .map(|a| {
                            let idx = if a == p.row_axis {
                                r
                            } else if a == p.col_axis {
                                c
                            } else {
                                p.fixed[a]
                            };
                            p.values[a].get(idx).cloned().unwrap_or_default()
                        })
                        .collect();
                    row.push(text);
                    self.with_table(|t| t.rows.push(row));
                }
                _ => self.error("aggregated cell (several rows share these keys) is read-only"),
            }
            return;
        }
        let cells = self.typed_cells(r, c, text);
        self.set_cells(cells);
    }

    fn clear_selection(&mut self) {
        if self.types_view() {
            return self.error("F8 deletes a type");
        }
        if self.functions_view() {
            return self.error("F8 deletes a function");
        }
        if self.calc_view() {
            return self.delete_calc_lines();
        }
        if self.solo_rule_layer().is_some() {
            return self.commit_value(String::new());
        }
        if self.view.pivot.is_some() {
            return self.commit_value(String::new());
        }
        let ((r0, c0), (r1, c1)) = self.selection();
        // Rows the filter hides keep their content.
        let shown = self.shown_in(r0, r1);
        let footer: Vec<(usize, usize)> = (r0..=r1)
            .filter_map(|r| self.footer_at(r))
            .flat_map(|k| (c0..=c1).map(move |c| (k, c)))
            .filter(|&(k, c)| self.table().footer[k].cells.get(c).is_some_and(|e| !e.is_empty()))
            .collect();
        if !footer.is_empty() {
            self.with_table(|t| {
                for (k, c) in footer {
                    t.footer[k].cells[c].clear();
                }
            });
        }
        if self.footer_at(r0).is_some() {
            return;
        }
        // In a data layer: its own cells (they take the table's again).
        if let z @ 1.. = self.shown_layer() {
            let cells = shown
                .iter()
                .flat_map(|&r| (c0..=c1).map(move |c| (r, c)))
                .filter(|&(r, c)| !self.own_raw(z, r, c).is_empty())
                .map(|(r, c)| (r, c, String::new()))
                .collect();
            return self.set_cells(cells);
        }
        // Only cells with data (an empty table or cells beyond it: nothing to clear).
        let raw = &self.wb.sheets[self.view.sheet].raw;
        let cells = shown
            .iter()
            .flat_map(|&r| (c0..=c1).map(move |c| (r, c)))
            .filter(|&(r, c)| raw.get(r).and_then(|row| row.get(c)).is_some_and(|v| !v.is_empty()))
            .map(|(r, c)| (r, c, String::new()))
            .collect();
        self.set_cells(cells);
    }

    /// Ctrl+X: copies the selection and clears it (one undo step).
    fn cut(&mut self) {
        if self.view.pivot.is_some() || self.view.kind != ViewKind::Table {
            return self.error("cut is only available in tables");
        }
        self.copy();
        self.clear_selection();
        let n: usize = self.clipboard.iter().map(Vec::len).sum();
        self.info(format!("cut {n} cell(s)"));
    }

    fn copy(&mut self) {
        if self.view.pivot.is_some() || self.view.kind != ViewKind::Table {
            return self.error("copy is only available in tables");
        }
        let ((r0, c0), (r1, c1)) = self.selection();
        let (rows, cols) = self.dims();
        self.clipboard = (r0..=r1.min(rows.saturating_sub(1)))
            .filter(|&r| r < rows && self.row_shown(r))
            .map(|r| (c0..=c1.min(cols.saturating_sub(1))).filter(|&c| c < cols).map(|c| self.effective_raw(r, c)).collect())
            .collect();
        let n: usize = self.clipboard.iter().map(Vec::len).sum();
        self.info(format!("copied {n} cell(s)"));
    }

    fn paste_grid(&mut self, grid: Vec<Vec<String>>) {
        if self.view.kind != ViewKind::Table {
            return self.error("paste is only available in tables");
        }
        if self.view.pivot.is_some() {
            if let Some(v) = grid.first().and_then(|r| r.first()) {
                self.commit_value(v.clone());
            }
            return;
        }
        let (r0, c0) = self.selection().0;
        if self.footer_at(r0).is_some() {
            return match grid.as_slice() {
                [row] if row.len() == 1 => self.commit_value(row[0].clone()),
                _ => self.error("footer cells are pasted one by one"),
            };
        }
        // Row by row into the shown rows: those the filter hides are skipped;
        // rows beyond the data grow the table.
        let rows = self.dims().0;
        let targets: Vec<usize> = (r0..rows).filter(|&r| self.row_shown(r)).chain(rows.max(r0)..).take(grid.len()).collect();
        let cells: Vec<(usize, usize, String)> = grid
            .into_iter()
            .zip(targets)
            .flat_map(|(row, r)| row.into_iter().enumerate().map(move |(j, v)| (r, c0 + j, v)))
            .collect();
        let n = cells.len();
        self.set_cells(cells);
        self.info(format!("pasted {n} cell(s)"));
    }

    pub fn save(&mut self, path: Option<&str>) {
        match path {
            Some(p) => {
                self.path = files::document_path(p);
                // Save as: the new name decides the compression.
                self.compression = crate::storage::Compression::from_path(&self.path);
            }
            // A new document asks for its name first.
            None if self.is_untitled() => {
                self.mode = Mode::Command { buf: "w ".into() };
                return self.info("save as: type a file name (.hcmd is added), Tab completes, Enter saves");
            }
            None => {}
        }
        if let Some(kind) = crate::bundle::Kind::from_path(&self.path) {
            let bundle = crate::bundle::build(&self.doc, self.bundle.as_ref(), kind, false);
            return match bundle.write(&self.path, kind) {
                Ok(()) => {
                    self.bundle = Some(bundle);
                    self.dirty = false;
                    crate::recent::add(&self.path);
                    self.info(format!("written {}", self.path.display()));
                    if let Some(action) = self.after_save.take() {
                        self.run_pending(action);
                    }
                }
                Err(e) => self.error(format!("cannot write {}: {e}", self.path.display())),
            };
        }
        // What hardcalc writes names its format version.
        self.doc.stamp();
        let text = crate::write(&self.doc);
        match crate::storage::write_text(&self.path, &text, self.compression) {
            Ok(bytes) => {
                self.dirty = false;
                crate::recent::add(&self.path);
                let how = match self.compression {
                    crate::storage::Compression::None => String::new(),
                    c => format!(", {}", c.name()),
                };
                self.info(format!("written {} ({bytes} bytes{how})", self.path.display()));
                if let Some(action) = self.after_save.take() {
                    self.run_pending(action);
                }
            }
            Err(e) => self.error(format!("cannot write {}: {e}", self.path.display())),
        }
    }

    // ------------------------------------------------------------ navigation

    fn clamp_cursor(&mut self) {
        let (rows, cols) = self.view_dims();
        self.view.cursor.0 = self.view.cursor.0.min(rows.saturating_sub(1));
        self.view.cursor.1 = self.view.cursor.1.min(cols.saturating_sub(1));
        if let Some(a) = &mut self.view.anchor {
            a.0 = a.0.min(rows.saturating_sub(1));
            a.1 = a.1.min(cols.saturating_sub(1));
        }
    }

    fn move_by(&mut self, dr: isize, dc: isize, extend: bool) {
        if self.header_move(dr, dc, extend) {
            return;
        }
        if extend {
            self.view.anchor.get_or_insert(self.view.cursor);
        } else {
            self.view.anchor = None;
        }
        // Rows hidden by the filter and hidden columns are stepped over.
        let r = self.step_row(self.view.cursor.0, dr);
        let c = self.step_col(self.view.cursor.1, dc);
        self.view.cursor = (r, c);
        self.view.follow = true;
    }

    /// Arrow keys on a column / row header, or from the first row / column
    /// onto it. Returns whether the move was handled here.
    fn header_move(&mut self, dr: isize, dc: isize, extend: bool) -> bool {
        if self.view.kind != ViewKind::Table || self.view.pivot.is_some() {
            return false;
        }
        // Headers reach as far as the view: the + row / column, or the whole
        // window of a filling table.
        let (rows, cols) = self.view_dims();
        let (r, c) = self.view.cursor;
        match self.view.header {
            Some(Header::Corner) if dc > 0 && cols > 0 => self.enter_header(Header::Column, 0, 0),
            // With data layers the origin is their head: ↑/↓ go through the stack.
            Some(Header::Corner) if dr != 0 && self.layer_count() > 0 && !extend => self.step_layer(dr),
            Some(Header::Corner) if dr > 0 && rows > 0 => self.enter_header(Header::Row, 0, 0),
            Some(Header::Column) if dc < 0 && c == 0 && !extend => self.enter_header(Header::Corner, 0, 0),
            Some(Header::Row) if dr < 0 && self.step_row(r, -1) == r && !extend => self.enter_header(Header::Corner, 0, 0),
            Some(Header::Column) if dr > 0 => {
                self.leave_header();
                self.snap_to_shown();
            }
            Some(Header::Row) if dc > 0 => self.leave_header(),
            Some(Header::Column) if dc != 0 => {
                self.extend_header(extend);
                self.view.cursor.1 = self.step_col(c, dc);
            }
            Some(Header::Row) if dr != 0 => {
                self.extend_header(extend);
                self.view.cursor.0 = self.step_row(r, dr);
            }
            // Already at the edge.
            Some(_) => {}
            // From the first row (the first one the filter shows) up onto the header.
            None if dr == -1 && dc == 0 && self.step_row(r, -1) == r && c < cols && !extend => self.enter_header(Header::Column, 0, c),
            None if dc == -1 && dr == 0 && c == 0 && r < rows && !extend => self.enter_header(Header::Row, r, 0),
            None => return false,
        }
        self.view.follow = true;
        true
    }

    /// `Name` or `Name:Type` for the column at the cursor (on its header);
    /// references to the old name follow. A column beyond the data is added.
    fn rename_column(&mut self, text: &str) {
        let c = self.view.cursor.1;
        // Read as the header of a table in a file is.
        let (name, ty) = crate::parse::split_header(text.trim().trim_end_matches(':'));
        let name = table_name(&name);
        let starts = name.chars().next().is_some_and(|ch| ch.is_alphabetic() || ch == '_');
        if !starts || !valid_table_name(&name) {
            return self.error(format!("'{name}' is not a valid column name (letters, digits, spaces, _ and -, starting with a letter)"));
        }
        // A name that reads like a cell or another column's letter would be ambiguous.
        let letter = crate::engine::letters_to_col(&name).is_some_and(|l| l != c);
        if letter || crate::engine::parse_a1(&name).is_some() {
            return self.error(format!("'{name}' looks like a cell or column reference"));
        }
        let columns = &self.wb.sheets[self.view.sheet].columns;
        if columns.iter().enumerate().any(|(i, col)| i != c && col.name == name) {
            return self.error(format!("column {name} already exists"));
        }
        let old = columns.get(c).map(|col| col.name.clone());
        let sheet = self.view.sheet;
        let rows = self.dims().0;
        let new = name.clone();
        self.with_doc(|doc| {
            if let Some(old) = &old
                && *old != new
            {
                crate::refs::rename_column(doc, sheet, old, &new);
            }
            if let Some(t) = table_mut(doc, sheet) {
                grow(t, rows, c + 1);
                t.columns[c].name = new;
                t.columns[c].ty = ty;
            }
        });
        match old {
            Some(old) if old != name => self.info(format!("renamed column {old} → {name}; references updated")),
            Some(_) => self.info(format!("column {name} updated")),
            None => self.info(format!("column {name} added")),
        }
    }

    /// Adds a footer row (below the others) and moves the cursor into it.
    fn add_footer(&mut self, label: Option<String>) {
        let taken = |l: &str| self.table().footer.iter().any(|f| f.label == l);
        let label = match label {
            Some(l) => l.trim().to_string(),
            None => (1..).map(|n| if n == 1 { "Total".to_string() } else { format!("Total {n}") }).find(|l| !taken(l)).unwrap(),
        };
        if let Err(e) = self.check_footer_label(&label, None) {
            return self.error(e);
        }
        let cols = self.dims().1;
        // Footers show below the + row, not in a table filling the window.
        self.with_table(|t| {
            t.footer.push(FooterRow { label: label.clone(), cells: vec![String::new(); cols] });
            if t.fill == Some(true) {
                t.fill = None;
            }
        });
        let k = self.table().footer.len() - 1;
        let start = self.footer_start().unwrap_or(0);
        self.move_to(start + k, self.view.cursor.1.min(cols.saturating_sub(1)));
        self.info(format!("footer {label} added – type =sum, =avg, =count …, a formula, a value or a text; ← on its label renames it"));
    }

    /// A footer label must be non-empty, without `:` and unique in the table.
    fn check_footer_label(&self, label: &str, current: Option<usize>) -> Result<(), String> {
        if label.is_empty() || label.contains([':', '.', '"']) {
            return Err(format!("'{label}' is not a valid footer label (no : . or \")"));
        }
        if self.table().footer.iter().enumerate().any(|(k, f)| Some(k) != current && f.label == label) {
            return Err(format!("footer {label} already exists"));
        }
        Ok(())
    }

    /// New label for footer row `k`; references to it follow.
    fn rename_footer(&mut self, k: usize, label: &str) {
        if let Err(e) = self.check_footer_label(label, Some(k)) {
            return self.error(e);
        }
        let old = self.table().footer[k].label.clone();
        if old == label {
            return;
        }
        let sheet = self.view.sheet;
        self.with_doc(|doc| {
            crate::refs::rename_footer(doc, sheet, &old, label);
            if let Some(t) = table_mut(doc, sheet) {
                t.footer[k].label = label.to_string();
            }
        });
        self.info(format!("renamed footer {old} → {label}; references updated"));
    }

    /// Sets footer cell (`k`, `c`), written as in the data (`=sum`, a formula, a value or a text).
    fn set_footer_cell(&mut self, k: usize, c: usize, text: &str) {
        let cols = self.dims().1;
        if c >= cols {
            return self.error("footer cells belong to a column – add the column first");
        }
        let text = text.trim().to_string();
        let text = crate::types::point_cell(&text, &self.wb.sheets[self.view.sheet].columns[c].ty).unwrap_or(text);
        self.with_table(|t| {
            t.footer[k].cells.resize(cols, String::new());
            t.footer[k].cells[c] = text;
        });
    }

    /// New name for the current table; references to it follow.
    fn rename_table(&mut self, name: &str) {
        let name = &table_name(name);
        if !valid_table_name(name) {
            return self.error(format!("'{name}' is not a valid table name (letters, digits, spaces, _ and -)"));
        }
        if self.wb.sheet_index(name).is_some_and(|i| i != self.view.sheet) {
            return self.error(format!("table {name} already exists"));
        }
        let old = self.wb.sheets[self.view.sheet].name.clone();
        let sheet = self.view.sheet;
        self.with_doc(|doc| crate::refs::rename_table(doc, sheet, name));
        match old {
            Some(old) => self.info(format!("renamed table {old} → {name}; references updated")),
            None => self.info(format!("table named {name}")),
        }
    }

    /// Click on a column / row header: selects it (Shift: up to it); dragging extends.
    fn click_header(&mut self, header: Header, at: (usize, usize), extend: bool) {
        if matches!(self.mode, Mode::Edit { .. }) {
            self.commit_edit(0, 0);
        }
        if extend && self.on_header() == Some(header) {
            self.view.anchor.get_or_insert(self.view.cursor);
        } else {
            self.view.anchor = None;
        }
        self.view.header = Some(header);
        self.view.cursor = at;
        self.view.follow = false;
        self.dragging = true;
    }

    fn enter_header(&mut self, header: Header, r: usize, c: usize) {
        self.view.header = Some(header);
        self.view.anchor = None;
        self.view.cursor = (r, c);
        let what = match header {
            Header::Row if self.footer_at(r).is_some() => "footer row selected – type to rename it, F8 deletes it, → back to the cells",
            Header::Column => "column selected – type to name it, - / + moves it, Shift+←/→ extends, ↓ back to the cells",
            Header::Row => "row selected – - / + moves it, Shift+↑/↓ extends, → back to the cells",
            Header::Corner if self.layer_count() > 0 => "whole table selected – type to rename it · ↑/↓ go through the slices and scenarios · - / + move the shown one",
            Header::Corner => "whole table selected – type or Enter to rename it",
        };
        self.info(what);
    }

    fn extend_header(&mut self, extend: bool) {
        if extend {
            self.view.anchor.get_or_insert(self.view.cursor);
        } else {
            self.view.anchor = None;
        }
    }

    /// Back from a header into the cells (the cursor cell of the column / row).
    fn leave_header(&mut self) {
        self.view.header = None;
        self.view.anchor = None;
    }

    /// Moves the selected columns (dc) or rows (dr) one place (keys - and + on
    /// a header); references in formulas and layer rules follow.
    fn move_lines(&mut self, dr: isize, dc: isize) {
        let header = self.on_header();
        let ((r0, c0), (r1, c1)) = self.selection();
        let (mut rows, mut cols) = self.dims();
        // In a filling table, lines move into the empty area too: the table grows.
        if self.fills() {
            let need = |hi: usize| if dr + dc > 0 { hi + 2 } else { hi + 1 };
            match header {
                Some(Header::Column) => cols = cols.max(need(c1)),
                Some(Header::Row) => rows = rows.max(need(r1)),
                _ => {}
            }
        }
        let (lo, hi, n, d) = match header {
            Some(Header::Column) if dc != 0 => (c0, c1, cols, dc),
            Some(Header::Row) if dr != 0 => (r0, r1, rows, dr),
            _ => return,
        };
        if header == Some(Header::Row) && self.hidden_count().is_some() {
            return self.error("rows are moved with all rows shown – switch the filter layer off first");
        }
        if n == 0 || hi >= n || (d < 0 && lo == 0) || (d > 0 && hi + 1 >= n) {
            return;
        }
        // map[old] = new
        let mut map: Vec<usize> = (0..n).collect();
        if d < 0 {
            map[lo - 1] = hi;
            map[lo..=hi].iter_mut().for_each(|i| *i -= 1);
        } else {
            map[hi + 1] = lo;
            map[lo..=hi].iter_mut().for_each(|i| *i += 1);
        }
        let identity: Vec<usize> = (0..if header == Some(Header::Column) { rows } else { cols }).collect();
        let (row_map, col_map) = if header == Some(Header::Column) { (identity, map) } else { (map, identity) };
        let sheet = self.view.sheet;
        self.with_doc(|doc| {
            crate::refs::update_document(doc, sheet, &crate::refs::Moves { rows: &row_map, cols: &col_map });
            if let Some(t) = table_mut(doc, sheet) {
                grow(t, rows, cols);
                if header == Some(Header::Column) {
                    // Columns named by their letter keep matching their position.
                    let mut auto: Vec<bool> = t.columns.iter().enumerate().map(|(i, c)| c.name == col_letters(i)).collect();
                    shift_block(&mut t.columns, lo, hi, d);
                    shift_block(&mut auto, lo, hi, d);
                    for (i, col) in t.columns.iter_mut().enumerate() {
                        if auto[i] {
                            col.name = col_letters(i);
                        }
                    }
                    data_layers::pad_layers(t);
                    for row in t.grids_mut().flatten() {
                        if hi + usize::from(d > 0) < row.len() {
                            shift_block(row, lo, hi, d);
                        }
                    }
                    for f in &mut t.footer {
                        shift_block(&mut f.cells, lo, hi, d);
                    }
                } else {
                    data_layers::pad_layers(t);
                    for grid in t.grids_mut() {
                        shift_block(grid, lo, hi, d);
                    }
                }
            }
        });
        let step = |i: usize| (i as isize + d) as usize;
        match header {
            Some(Header::Column) => {
                self.view.cursor.1 = step(self.view.cursor.1);
                if let Some(a) = &mut self.view.anchor {
                    a.1 = step(a.1);
                }
            }
            _ => {
                self.view.cursor.0 = step(self.view.cursor.0);
                if let Some(a) = &mut self.view.anchor {
                    a.0 = step(a.0);
                }
            }
        }
        self.view.follow = true;
    }

    fn move_to(&mut self, r: usize, c: usize) {
        self.view.anchor = None;
        self.view.header = None;
        self.view.cursor = (r, c);
        self.clamp_cursor();
        self.view.follow = true;
    }

    pub fn switch_sheet(&mut self, i: usize) {
        if i < self.wb.sheets.len() && (i != self.view.sheet || self.view.kind != ViewKind::Table) {
            self.set_view(ViewKind::Table, i);
        }
    }

    /// Shows `kind` (for tables: sheet `sheet`) in the focused window.
    pub fn set_view(&mut self, kind: ViewKind, sheet: usize) {
        self.view = View::new(kind, sheet);
        if kind == ViewKind::Table {
            self.view.layer = self.levels.get(&sheet).copied().unwrap_or(0);
            self.clamp_view();
            self.snap_to_shown();
        }
        self.layout_grab = false;
    }

    pub fn types_view(&self) -> bool {
        self.view.kind == ViewKind::Types
    }

    // ------------------------------------------------------------ editing

    fn start_edit(&mut self, initial: Option<char>) {
        if self.on_header() == Some(Header::Row) && self.cursor_footer().is_none() {
            return self.error("a whole row is selected – Enter or → goes back into the cells");
        }
        if self.on_header().is_none() && self.cursor_footer().is_some() && self.view.cursor.1 >= self.dims().1 {
            return self.error("footer cells belong to a column – add the column first");
        }
        if self.functions_view()
            && let Some(p) = self.function_edit_problem()
        {
            return self.error(p);
        }
        if self.calc_view()
            && let Some(p) = self.calc_edit_problem()
        {
            return self.error(p);
        }
        if self.types_view() {
            let (r, c) = self.view.cursor;
            let problem = match self.type_rows().get(r) {
                Some(TypeRow::User { .. }) if c <= 1 => None,
                Some(TypeRow::User { .. }) => Some("this column is computed – edit Type or Definition"),
                Some(TypeRow::New) if c == 0 => None,
                Some(TypeRow::New) => Some("enter the name of the new type in column Type first"),
                _ => Some("built-in types are read-only"),
            };
            if let Some(p) = problem {
                return self.error(p);
            }
        }
        let buf = match initial {
            Some(ch) => ch.to_string(),
            None => self.cursor_edit_text(),
        };
        let cursor = buf.chars().count();
        self.mode = Mode::Edit { buf, cursor };
        self.quick_entry = initial.is_some() && !matches!(self.view.kind, ViewKind::Layout(_));
        self.point = None;
        self.message = None;
    }

    /// Commits a running edit (if any) and moves by (dr, dc).
    fn commit_edit(&mut self, dr: isize, dc: isize) {
        if let Mode::Edit { buf, .. } = std::mem::replace(&mut self.mode, Mode::Normal) {
            let edited = self.view.cursor.0;
            if buf != self.cursor_edit_text() {
                self.commit_value(buf);
            }
            let was_hidden = self.row_hidden(edited);
            // Names of the table, its columns and footers: the cursor stays on the header.
            let corner = matches!(self.on_header(), Some(Header::Corner | Header::Column | Header::Row));
            if std::mem::take(&mut self.skip_move) || corner || matches!(self.view.kind, ViewKind::Layout(_)) {
                self.view.follow = true;
            } else {
                self.move_by(dr, dc, false);
            }
            // The row just typed no longer meets the filter: say where it went.
            if was_hidden && self.view.cursor.0 != edited && self.message.as_ref().is_none_or(|m| !m.error) {
                self.info(format!("row {} does not meet the filter and is hidden – the filter layer (or :filter off) shows all rows", edited + 1));
            }
        }
    }

    pub fn on_paste(&mut self, text: &str) {
        if self.mode == Mode::Normal {
            // Tab-separated lines, as copied from other spreadsheets.
            let grid: Vec<Vec<String>> = text
                .trim_end_matches(['\n', '\r'])
                .lines()
                .map(|l| l.split('\t').map(|s| s.trim().to_string()).collect())
                .collect();
            self.paste_grid(grid);
        } else {
            self.insert_text(text.lines().next().unwrap_or_default().to_string());
        }
    }

    fn insert_text(&mut self, text: String) {
        match &mut self.mode {
            Mode::Edit { buf, cursor } => {
                let byte = buf.char_indices().nth(*cursor).map(|(i, _)| i).unwrap_or(buf.len());
                buf.insert_str(byte, &text);
                *cursor += text.chars().count();
            }
            Mode::Command { buf } => buf.push_str(&text),
            Mode::Normal => {}
        }
    }

    // ------------------------------------------------------------ input

    pub fn on_key(&mut self, key: KeyEvent) {
        // Terminals in "Option as Meta" mode send Alt+arrow as ESC ESC [ A,
        // which arrives as Esc, '[', 'A' at the same instant – no one types
        // that fast, so the three are put back together.
        const SAME_INSTANT: Duration = Duration::from_millis(15);
        let now = Instant::now();
        let recent = self.esc_seq.is_some_and(|(t, _)| now.duration_since(t) < SAME_INSTANT);
        match (self.esc_seq.take(), key.code) {
            (Some((t, false)), KeyCode::Char('[')) if recent => {
                self.esc_seq = Some((t, true));
                return;
            }
            (Some((_, true)), KeyCode::Char(c @ ('A' | 'B' | 'C' | 'D'))) if recent => {
                let arrow = match c {
                    'A' => KeyCode::Up,
                    'B' => KeyCode::Down,
                    'C' => KeyCode::Right,
                    _ => KeyCode::Left,
                };
                return self.on_key(KeyEvent::new(arrow, KeyModifiers::ALT));
            }
            // Not a sequence after all: the swallowed '[' is a real key.
            (Some((_, true)), _) => self.handle_key(KeyEvent::new(KeyCode::Char('['), KeyModifiers::NONE)),
            _ => {}
        }
        if key.code == KeyCode::Esc && key.modifiers.is_empty() {
            self.esc_seq = Some((now, false));
        }
        // A conditional key that arrives works here.
        if self.key_test.is_none() && self.caps.observe(&key) {
            self.save_caps();
        }
        self.handle_key(key);
        if self.show_keys {
            self.info(format!("key received: {:?} {:?} (:keys turns this off)", key.code, key.modifiers));
        }
    }

    fn handle_key(&mut self, key: KeyEvent) {
        if self.key_test.is_some() {
            return self.key_test_key(key);
        }
        if self.context.is_some() {
            return self.key_context(key);
        }
        if self.values.is_some() {
            return self.key_values(key);
        }
        if self.show_help {
            return self.key_help(key);
        }
        if self.confirm.is_some() {
            return self.key_confirm(key);
        }
        if self.menu.is_some() {
            return self.key_menu(key);
        }
        if self.file_dialog.is_some() {
            return self.key_file_dialog(key);
        }
        if self.paper_menu.is_some() {
            return self.key_paper_menu(key);
        }
        if self.preview.is_some() {
            return self.key_preview(key);
        }
        match self.mode {
            Mode::Normal => self.key_normal(key),
            Mode::Edit { .. } => self.key_edit(key),
            Mode::Command { .. } => self.key_command(key),
        }
    }

    fn key_normal(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        if !matches!(key.code, KeyCode::Char(':') | KeyCode::F(7)) {
            self.last_sort = None;
        }
        let quit_key = key.code == KeyCode::F(10) || (ctrl && key.code == KeyCode::Char('q'));
        if !quit_key {
            self.quit_armed = false;
        }
        self.message = None;
        if matches!(key.code, KeyCode::Tab | KeyCode::BackTab) && self.is_split() {
            return self.swap_focus();
        }
        if let ViewKind::Layout(i) = self.view.kind
            && !ctrl
            && !matches!(key.code, KeyCode::F(_) | KeyCode::Char(':') | KeyCode::Tab | KeyCode::BackTab)
            && self.key_layout(i, key)
        {
            return;
        }
        if !self.view.is_data() {
            if self.key_helper(key) {
                return;
            }
            // Editing keys mean nothing in a helper window.
            if matches!(key.code, KeyCode::Char(_) | KeyCode::Enter | KeyCode::Delete | KeyCode::Backspace) && !ctrl {
                return;
            }
        }
        // The layout editor gives F3 … F8 its own meaning.
        if let (ViewKind::Layout(_), KeyCode::F(n)) = (self.view.kind, key.code)
            && key.modifiers.is_empty()
        {
            return self.fkey(n);
        }
        // Everything in the KEYMAP; what remains are cursor keys and typing.
        if let Some(action) = keymap::action_for(&key, &self.caps) {
            return self.handle(action);
        }
        match key.code {
            // On a column / row header, - and + move it backwards / forwards.
            // On the table origin, - and + move the shown slice / scenario in the stack.
            KeyCode::Char(ch @ ('-' | '+')) if !ctrl && self.on_header() == Some(Header::Corner) && self.layer_count() > 0 => {
                self.move_level(if ch == '-' { -1 } else { 1 });
            }
            KeyCode::Char(ch @ ('-' | '+')) if !ctrl && matches!(self.on_header(), Some(Header::Column | Header::Row)) => {
                let d = if ch == '-' { -1 } else { 1 };
                match self.on_header() {
                    Some(Header::Column) => self.move_lines(0, d),
                    _ => self.move_lines(d, 0),
                }
            }
            // Ctrl+F asks for a text to find, Ctrl+G goes to the next match.
            KeyCode::Char('f') if ctrl && self.view.is_data() => {
                self.mode = Mode::Command { buf: "find ".into() };
                self.cmd_choice = None;
            }
            KeyCode::Char('g') if ctrl => self.find(None, false),
            // On a column header, Space opens its value list (the filter).
            KeyCode::Char(' ') if !ctrl && !alt && self.on_header() == Some(Header::Column) && self.view.cursor.1 < self.dims().1 => self.open_values(None),
            KeyCode::Up if alt && self.view.kind == ViewKind::Table => self.step_layer(-1),
            KeyCode::Down if alt && self.view.kind == ViewKind::Table => self.step_layer(1),
            KeyCode::Up => self.move_by(-1, 0, shift),
            KeyCode::Down => self.move_by(1, 0, shift),
            KeyCode::Left => self.move_by(0, -1, shift),
            KeyCode::Right => self.move_by(0, 1, shift),
            KeyCode::Tab => self.tab_key(false),
            KeyCode::BackTab => self.tab_key(true),
            KeyCode::PageDown => self.move_by(self.view.page_rows as isize, 0, shift),
            KeyCode::PageUp => self.move_by(-(self.view.page_rows as isize), 0, shift),
            KeyCode::Home if ctrl => {
                self.move_to(0, 0);
                self.snap_to_shown();
            }
            KeyCode::End if ctrl => {
                let (rows, cols) = self.dims();
                self.move_to(rows.saturating_sub(1), cols.saturating_sub(1));
                // The last row the filter shows.
                if self.row_hidden(self.view.cursor.0) {
                    self.view.cursor.0 = self.step_row(self.view.cursor.0, -1);
                }
            }
            KeyCode::Home => {
                self.move_to(self.view.cursor.0, 0);
                self.snap_to_shown_col();
            }
            KeyCode::End => {
                self.move_to(self.view.cursor.0, self.dims().1.saturating_sub(1));
                if self.col_hidden(self.view.cursor.1) {
                    self.view.cursor.1 = self.step_col(self.view.cursor.1, -1);
                }
            }
            KeyCode::Esc if !self.has_range() && self.solo.is_some() => self.set_solo(None),
            // Out of a data layer back to the table itself.
            KeyCode::Esc if !self.has_range() && self.shown_layer() > 0 => self.set_layer(0),
            KeyCode::Esc if self.types_view() && !self.has_range() => self.toggle_types(),
            KeyCode::Esc => self.leave_header(),
            KeyCode::F(n) => self.fkey(n),
            KeyCode::Char(ch) if !ctrl && !alt => self.start_edit(Some(ch)),
            _ => {}
        }
    }

    fn key_edit(&mut self, key: KeyEvent) {
        let arrow = match key.code {
            KeyCode::Up => Some((-1, 0)),
            KeyCode::Down => Some((1, 0)),
            KeyCode::Left => Some((0, -1)),
            KeyCode::Right => Some((0, 1)),
            _ => None,
        };
        if let Some((dr, dc)) = arrow
            && self.can_point()
        {
            return self.point_move(dr, dc, key.modifiers.contains(KeyModifiers::SHIFT));
        }
        // Any other key ends pointing.
        self.point = None;
        // Typed straight into the cell: ← → move on like ↑ ↓ (formulas keep the caret).
        if self.quick_entry
            && let Some((0, dc)) = arrow
            && matches!(&self.mode, Mode::Edit { buf, .. } if !buf.trim_start().starts_with('='))
        {
            return self.commit_edit(0, dc);
        }
        if matches!(key.code, KeyCode::Home | KeyCode::End) {
            self.quick_entry = false;
        }
        let Mode::Edit { buf, cursor } = &mut self.mode else { return };
        let len = buf.chars().count();
        let byte_at = |s: &str, i: usize| s.char_indices().nth(i).map(|(b, _)| b).unwrap_or(s.len());
        match key.code {
            KeyCode::F(1) => self.open_help(),
            KeyCode::F(2) => self.commit_edit(0, 0),
            KeyCode::Esc | KeyCode::F(10) => self.mode = Mode::Normal,
            KeyCode::Enter => self.commit_edit(1, 0),
            KeyCode::Tab => self.commit_edit(0, 1),
            KeyCode::BackTab => self.commit_edit(0, -1),
            KeyCode::Up => self.commit_edit(-1, 0),
            KeyCode::Down => self.commit_edit(1, 0),
            KeyCode::Left => *cursor = cursor.saturating_sub(1),
            KeyCode::Right => *cursor = (*cursor + 1).min(len),
            KeyCode::Home => *cursor = 0,
            KeyCode::End => *cursor = len,
            KeyCode::Backspace if *cursor > 0 => {
                buf.remove(byte_at(buf, *cursor - 1));
                *cursor -= 1;
            }
            KeyCode::Delete if *cursor < len => {
                buf.remove(byte_at(buf, *cursor));
            }
            KeyCode::Char(ch) => self.insert_text(ch.to_string()),
            _ => {}
        }
    }

    fn key_command(&mut self, key: KeyEvent) {
        // F2 runs the line like Enter, F10 leaves it like Esc.
        let key = match key.code {
            KeyCode::F(1) => return self.open_help(),
            KeyCode::F(2) => KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            // F4 F4: the go-to prompt that F4 opened jumps at once.
            KeyCode::F(4) if matches!(&self.mode, Mode::Command { buf } if buf.starts_with("goto ")) => {
                KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)
            }
            KeyCode::F(10) => KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            _ => key,
        };
        match key.code {
            KeyCode::Up => return self.choose_suggestion(-1),
            KeyCode::Down => return self.choose_suggestion(1),
            // Tab: the command or value suggested, else a file name.
            KeyCode::Tab => {
                if !self.apply_suggestion()
                    && let Mode::Command { buf } = &self.mode
                    && let Some(done) = self.complete_command(&buf.clone())
                {
                    self.mode = Mode::Command { buf: done };
                }
                return;
            }
            // Enter on a suggestion chosen with ↑/↓ takes it; one that still
            // needs an argument stays in the command line.
            KeyCode::Enter => {
                if self.cmd_choice.is_some() && self.apply_suggestion() && matches!(&self.mode, Mode::Command { buf } if buf.ends_with(' ')) {
                    return;
                }
                let Mode::Command { buf } = &mut self.mode else { return };
                let mut cmd = std::mem::take(buf);
                self.mode = Mode::Normal;
                self.cmd_choice = None;
                // `:` alone runs the last command again.
                if cmd.trim().is_empty() {
                    match self.last_command.clone() {
                        Some(last) => cmd = last,
                        None => return,
                    }
                } else {
                    self.last_command = Some(cmd.clone());
                }
                // Commands act on the main window when a helper is focused.
                return self.run_command_line(&cmd);
            }
            _ => {}
        }
        let Mode::Command { buf } = &mut self.mode else { return };
        match key.code {
            // Leaving the command line also drops an action waiting for a save.
            KeyCode::Esc => {
                self.mode = Mode::Normal;
                self.after_save = None;
            }
            KeyCode::Backspace if buf.is_empty() => {
                self.mode = Mode::Normal;
                self.after_save = None;
            }
            KeyCode::Backspace => {
                buf.pop();
                self.cmd_choice = None;
            }
            KeyCode::Char(ch) => {
                buf.push(ch);
                self.cmd_choice = None;
            }
            _ => {}
        }
    }

    /// A menu or dialog is open above the windows.
    fn overlay_open(&self) -> bool {
        self.key_test.is_some()
            || self.context.is_some()
            || self.values.is_some()
            || self.show_help
            || self.confirm.is_some()
            || self.menu.is_some()
            || self.file_dialog.is_some()
            || self.paper_menu.is_some()
    }

    /// The function key whose label is at `pos` and not hidden by a dialog.
    fn fkey_at(&self, pos: Position) -> Option<u8> {
        match self.hits.iter().rev().find(|(r, _)| r.contains(pos)) {
            Some((_, Hit::FKey(n))) => Some(*n),
            _ => None,
        }
    }

    pub fn on_mouse(&mut self, m: MouseEvent) {
        let pos = Position::new(m.column, m.row);
        if self.caps.mouse != Cap::Confirmed && matches!(m.kind, MouseEventKind::Down(_)) {
            if self.key_test == Some(2) && m.kind == MouseEventKind::Down(MouseButton::Right) {
                return self.key_test_answer(Cap::Confirmed);
            }
            self.caps.set(Cond::Mouse, Cap::Confirmed);
            self.save_caps();
        }
        // A click on the bar is the key it shows, whatever is open.
        if m.kind == MouseEventKind::Down(MouseButton::Left)
            && !m.modifiers.contains(KeyModifiers::CONTROL)
            && (self.overlay_open() || self.mode != Mode::Normal)
            && let Some(n) = self.fkey_at(pos)
        {
            return self.handle_key(KeyEvent::new(KeyCode::F(n), KeyModifiers::NONE));
        }
        if let Some(_ctx) = self.context {
            if let MouseEventKind::Down(_) = m.kind {
                match self.hits.iter().rev().find(|(r, h)| r.contains(pos) && matches!(h, Hit::ContextItem(_))).map(|(_, h)| *h) {
                    Some(Hit::ContextItem(i)) => self.choose_context(i),
                    _ => self.context = None,
                }
            }
            return;
        }
        if let Some(list) = &self.values {
            let inside = list.rect.contains(pos);
            match m.kind {
                MouseEventKind::Down(_) => {
                    let hit = self.hits.iter().rev().find(|(r, h)| r.contains(pos) && matches!(h, Hit::ValueItem(_) | Hit::ValuesApply)).map(|(_, h)| *h);
                    self.click_values(hit, inside);
                }
                MouseEventKind::ScrollDown => self.key_values(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)),
                MouseEventKind::ScrollUp => self.key_values(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE)),
                _ => {}
            }
            return;
        }
        let mut hit = self.hits.iter().find(|(r, _)| r.contains(pos)).map(|(_, h)| *h);
        if self.show_help {
            // Page titles are clickable; a click elsewhere closes the help.
            match (m.kind, hit) {
                (MouseEventKind::Down(MouseButton::Left), Some(Hit::HelpPage(n))) => self.help_page = n,
                (MouseEventKind::Down(_), _) => self.show_help = false,
                (MouseEventKind::ScrollDown, _) => self.key_help(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE)),
                (MouseEventKind::ScrollUp, _) => self.key_help(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE)),
                _ => {}
            }
            return;
        }
        if let Some((open, _)) = self.menu {
            if let MouseEventKind::Down(_) = m.kind {
                let target = self.hits.iter().rev().find(|(r, h)| r.contains(pos) && matches!(h, Hit::MenuTop(_) | Hit::MenuItem(_) | Hit::Logo));
                match target.map(|(_, h)| *h) {
                    Some(Hit::MenuTop(i)) if i != open => self.menu = Some((i, 0)),
                    Some(Hit::MenuItem(i)) => self.run_menu_item(open, i),
                    _ => self.menu = None,
                }
            }
            return;
        }
        if let (Mode::Command { .. }, MouseEventKind::Down(MouseButton::Left)) = (&self.mode, m.kind)
            && let Some(i) = self.hits.iter().find_map(|(r, h)| match h {
                Hit::Suggestion(i) if r.contains(pos) => Some(*i),
                _ => None,
            })
        {
            self.cmd_choice = Some(i);
            self.apply_suggestion();
            return;
        }
        if self.confirm.is_some() {
            if let (MouseEventKind::Down(MouseButton::Left), Some(Hit::Confirm(n))) = (m.kind, hit) {
                self.answer_confirm(n);
            }
            return;
        }
        if self.file_dialog.is_some() {
            let spot = self.hits.iter().find_map(|(r, h)| match h {
                Hit::File(spot) if r.contains(pos) => Some(*spot),
                _ => None,
            });
            match (m.kind, spot) {
                (MouseEventKind::Down(MouseButton::Left), Some(spot)) => {
                    let now = Instant::now();
                    let double = self.last_click.is_some_and(|(t, x, y)| now.duration_since(t) < DOUBLE_CLICK && (x, y) == (m.column, m.row));
                    self.last_click = Some((now, m.column, m.row));
                    self.click_file_spot(spot, double);
                }
                (MouseEventKind::ScrollDown, _) => self.scroll_file_dialog(3),
                (MouseEventKind::ScrollUp, _) => self.scroll_file_dialog(-3),
                _ => {}
            }
            return;
        }
        if self.paper_menu.is_some() {
            if let MouseEventKind::Down(MouseButton::Left) = m.kind {
                match hit {
                    Some(Hit::PaperChoice(n)) => self.apply_paper(n),
                    _ => self.paper_menu = None,
                }
            }
            return;
        }
        if self.preview.is_none() && self.split_mouse(m) {
            return;
        }
        if self.preview.is_none() && self.is_split() {
            let window = self.window_areas.iter().find(|(_, r)| r.contains(pos)).map(|(left, _)| *left);
            if let Some(left) = window
                && left != self.left_focused
            {
                match m.kind {
                    MouseEventKind::Down(MouseButton::Left) => {
                        // Clicking into the other window focuses it, then acts there.
                        if matches!(self.mode, Mode::Edit { .. }) {
                            self.commit_edit(0, 0);
                        }
                        hit = self.other_hits.iter().find(|(r, _)| r.contains(pos)).map(|(_, h)| *h);
                        self.swap_focus();
                    }
                    MouseEventKind::ScrollDown
                    | MouseEventKind::ScrollUp
                    | MouseEventKind::ScrollLeft
                    | MouseEventKind::ScrollRight => {
                        return self.with_window(left, |app| app.wheel(m));
                    }
                    _ => return,
                }
            }
        }
        if self.preview.is_none()
            && let ViewKind::Layout(i) = self.view.kind
            && let Some(g) = self.layout_geom.clone()
            && (g.area.contains(pos) || self.layout_drag.is_some())
        {
            if matches!(m.kind, MouseEventKind::Down(_)) && matches!(self.mode, Mode::Edit { .. }) {
                self.commit_edit(0, 0);
            }
            return self.layout_mouse(i, m, g);
        }
        if let Some(p) = &mut self.preview {
            match m.kind {
                MouseEventKind::Down(MouseButton::Left) => {
                    if self.show_help {
                        self.show_help = false;
                    } else if let Some(Hit::FKey(n)) = hit {
                        self.message = None;
                        self.preview_fkey(n);
                    }
                }
                MouseEventKind::ScrollDown => p.scroll.1 += 3,
                MouseEventKind::ScrollUp => p.scroll.1 = p.scroll.1.saturating_sub(3),
                MouseEventKind::ScrollRight => p.scroll.0 += 4,
                MouseEventKind::ScrollLeft => p.scroll.0 = p.scroll.0.saturating_sub(4),
                _ => {}
            }
            return;
        }
        // The ↗ of a link jumps on a plain click; otherwise it is part of its cell.
        let follows = matches!(m.kind, MouseEventKind::Down(MouseButton::Left))
            && !m.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::SHIFT)
            && matches!(self.mode, Mode::Normal)
            && !self.can_point();
        if let Some(Hit::Link(r, c)) = hit
            && !follows
        {
            hit = Some(Hit::Cell(r, c));
        }
        match m.kind {
            // Right click on a layer: its exclusive view (again: back to normal).
            MouseEventKind::Down(MouseButton::Right) => {
                if let Some(Hit::Layer(i)) = hit
                    && let Some(name) = self.layer_names.get(i).cloned()
                {
                    let next = if self.solo.as_deref() == Some(name.as_str()) { None } else { Some(name) };
                    self.set_solo(next);
                } else {
                    self.context_click(hit, (m.column, m.row));
                }
            }
            // Ctrl+click (macOS habit): the context menu as well.
            MouseEventKind::Down(MouseButton::Left) if m.modifiers.contains(KeyModifiers::CONTROL) => {
                self.context_click(hit, (m.column, m.row));
            }
            MouseEventKind::Down(MouseButton::Left) => {
                if hit != Some(Hit::FKey(7)) {
                    self.last_sort = None;
                }
                if hit != Some(Hit::FKey(10)) {
                    self.quit_armed = false;
                }
                if self.show_help {
                    self.show_help = false;
                    return;
                }
                if matches!(self.mode, Mode::Command { .. }) {
                    self.mode = Mode::Normal;
                }
                match hit {
                    // Editing a formula: the click picks a reference (Shift: a range).
                    Some(Hit::Cell(r, c)) if self.can_point() => {
                        let anchor = self.point.filter(|_| m.modifiers.contains(KeyModifiers::SHIFT)).map(|p| p.anchor.unwrap_or(p.cell));
                        self.point_at((r, c), anchor);
                        self.point_drag = true;
                    }
                    Some(Hit::Cell(r, c)) => {
                        if matches!(self.mode, Mode::Edit { .. }) {
                            self.commit_edit(0, 0);
                        }
                        let now = Instant::now();
                        let double = self
                            .last_click
                            .is_some_and(|(t, x, y)| now.duration_since(t) < DOUBLE_CLICK && (x, y) == (m.column, m.row));
                        self.last_click = Some((now, m.column, m.row));
                        if m.modifiers.contains(KeyModifiers::SHIFT) && self.on_header().is_none() {
                            self.view.anchor.get_or_insert(self.view.cursor);
                        } else {
                            self.view.anchor = None;
                        }
                        self.view.header = None;
                        self.view.cursor = (r, c);
                        self.view.follow = true;
                        self.dragging = true;
                        self.message = None;
                        if double {
                            self.start_edit(None);
                        }
                    }
                    Some(Hit::Values(c)) => self.open_values(Some(c)),
                    Some(Hit::ColHeader(c)) if self.view.kind == ViewKind::Table && self.view.pivot.is_none() => {
                        if c < self.view_dims().1 {
                            self.click_header(Header::Column, (0, c), m.modifiers.contains(KeyModifiers::SHIFT));
                        }
                    }
                    Some(Hit::ColHeader(c)) => {
                        let rows = self.dims().0;
                        self.view.anchor = Some((0, c));
                        self.view.cursor = (rows.saturating_sub(1), c);
                        self.view.follow = false;
                    }
                    Some(Hit::RowHeader(r)) if self.view.kind == ViewKind::Table && self.view.pivot.is_none() => {
                        if r < self.view_dims().0 {
                            self.click_header(Header::Row, (r, 0), m.modifiers.contains(KeyModifiers::SHIFT));
                        }
                    }
                    Some(Hit::Logo) => self.open_menu(0),
                    Some(Hit::FillHandle) => {
                        if matches!(self.mode, Mode::Edit { .. }) {
                            self.commit_edit(0, 0);
                        }
                        self.fill_drag = Some(self.selection());
                        self.fill_to = None;
                    }
                    Some(Hit::RowHeader(_) | Hit::File(_) | Hit::Confirm(_) | Hit::MenuTop(_) | Hit::MenuItem(_) | Hit::ContextItem(_) | Hit::Suggestion(_) | Hit::ValueItem(_) | Hit::ValuesApply) => {}
                    Some(Hit::Corner) => {
                        if matches!(self.mode, Mode::Edit { .. }) {
                            self.commit_edit(0, 0);
                        }
                        self.view.header = Some(Header::Corner);
                        self.view.anchor = None;
                        self.view.cursor = (0, 0);
                    }
                    Some(Hit::Tab(i)) => self.switch_sheet(i),
                    Some(Hit::Link(r, c)) => self.follow_link(r, c),
                    Some(Hit::Back) => self.back(),
                    Some(Hit::Layer(i)) => {
                        if let Some(name) = self.layer_names.get(i).cloned() {
                            if self.layer_empty(&name) {
                                self.info(format!("layer {name} has no rules for this table yet – right-click it: layer view, type a rule into a cell"));
                            } else {
                                self.toggle_layer(&name);
                            }
                        }
                    }
                    Some(Hit::DataLayer(z)) => self.set_layer(z),
                    Some(Hit::Filter(axis)) => self.cycle_filter(axis),
                    Some(Hit::TypesTab) => {
                        if !self.types_view() {
                            self.toggle_types();
                        }
                    }
                    Some(Hit::PivotButton) => self.toggle_pivot(None),
                    Some(Hit::ViewTab(kind)) => self.show_helper(kind),
                    Some(Hit::PaperChoice(n)) => self.apply_paper(n),
                    Some(Hit::LayerScope) => self.toggle_layer_scope(),
                    Some(Hit::HelpPage(n)) => self.help_page = n,
                    Some(Hit::ChartItem(i)) => {
                        if let ViewKind::Chart(chart) = self.view.kind {
                            self.select_chart_item(chart, i);
                        }
                    }
                    Some(Hit::Choice(n)) => {
                        self.view.cursor.0 = n;
                        self.apply_choice(n);
                    }
                    Some(Hit::FKey(n)) => {
                        if matches!(self.mode, Mode::Edit { .. }) {
                            self.commit_edit(0, 0);
                        }
                        self.message = None;
                        self.fkey(n);
                    }
                    None => {}
                }
            }
            MouseEventKind::Drag(MouseButton::Left) if self.fill_drag.is_some() => {
                if let Some(Hit::Cell(r, c)) = hit {
                    self.fill_to = Some((r, c));
                }
            }
            MouseEventKind::Drag(MouseButton::Left) if self.point_drag => {
                if let Some(Hit::Cell(r, c)) = hit
                    && let Some(p) = self.point
                    && (r, c) != p.cell
                {
                    self.point_at((r, c), Some(p.anchor.unwrap_or(p.cell)));
                }
            }
            MouseEventKind::Drag(MouseButton::Left) if self.dragging => {
                let (rows, cols) = self.dims();
                let to = match (self.on_header(), hit) {
                    (Some(Header::Column), Some(Hit::ColHeader(c) | Hit::Cell(_, c))) => Some((0, c.min(cols.saturating_sub(1)))),
                    (Some(Header::Row), Some(Hit::RowHeader(r) | Hit::Cell(r, _))) => Some((r.min(rows.saturating_sub(1)), 0)),
                    (None, Some(Hit::Cell(r, c))) => Some((r, c)),
                    _ => None,
                };
                if let Some(to) = to
                    && to != self.view.cursor
                {
                    self.view.anchor.get_or_insert(self.view.cursor);
                    self.view.cursor = to;
                    self.view.follow = true;
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                self.dragging = false;
                self.point_drag = false;
                if let (Some(src), Some(to)) = (self.fill_drag.take(), self.fill_to.take()) {
                    self.autofill(src, to, true);
                    return;
                }
                if self.view.anchor == Some(self.view.cursor) {
                    self.view.anchor = None;
                }
            }
            MouseEventKind::ScrollDown
            | MouseEventKind::ScrollUp
            | MouseEventKind::ScrollLeft
            | MouseEventKind::ScrollRight => self.wheel(m),
            _ => {}
        }
    }

    /// Dragging the split separator; returns whether the event was used.
    fn split_mouse(&mut self, m: MouseEvent) -> bool {
        let Some(body) = self.split_body else {
            self.split_drag = false;
            return false;
        };
        match m.kind {
            MouseEventKind::Down(MouseButton::Left) if self.split_sep.contains(Position::new(m.column, m.row)) => {
                self.split_drag = true;
            }
            MouseEventKind::Drag(MouseButton::Left) if self.split_drag => {
                self.split_percent = split_percent_at(body, m.column);
            }
            MouseEventKind::Up(MouseButton::Left) if self.split_drag => self.split_drag = false,
            _ => return false,
        }
        true
    }

    /// Action of function key `n` (also used for clicks on the F-key bar).
    pub fn fkey(&mut self, n: u8) {
        if self.preview.is_some() {
            return self.preview_fkey(n);
        }
        if !self.view.is_data() {
            return self.with_window(true, |app| app.fkey(n));
        }
        if let ViewKind::Layout(i) = self.view.kind
            && (3..=8).contains(&n)
        {
            return self.layout_fkey(i, n);
        }
        // The same functions as the keys in the KEYMAP.
        let key = KeyEvent::new(KeyCode::F(n), KeyModifiers::NONE);
        if let Some(action) = keymap::action_for(&key, &self.caps) {
            self.handle(action);
        }
    }

    /// Opens the command line with a command that needs an argument.
    pub(super) fn ask_argument(&mut self, command: &str) {
        self.mode = Mode::Command { buf: command.to_string() };
        self.cmd_choice = None;
    }

    /// F6: a new column right of the cursor; the command line asks for its name.
    pub(super) fn insert_column_key(&mut self) {
        if self.view.kind == ViewKind::Table && self.view.pivot.is_none() {
            self.ask_argument("addcol ");
        } else {
            // Says why not in this view.
            self.command("addcol");
        }
    }

    /// F10 / Ctrl+Q: with unsaved changes the first press warns, the second quits.
    fn quit_key(&mut self) {
        if !self.dirty || self.quit_armed {
            self.quit = true;
        } else {
            self.quit_armed = true;
            self.error("unsaved changes – F2 saves, F10 again quits without saving");
        }
    }

    /// Mouse wheel in the focused window.
    fn wheel(&mut self, m: MouseEvent) {
        if !self.view.is_data() || matches!(self.view.kind, ViewKind::Layout(_)) {
            match m.kind {
                MouseEventKind::ScrollDown => self.scroll_lines(3),
                MouseEventKind::ScrollUp => self.scroll_lines(-3),
                _ => {}
            }
            return;
        }
        match m.kind {
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
                let down = m.kind == MouseEventKind::ScrollDown;
                let (rows, cols) = self.dims();
                if m.modifiers.contains(KeyModifiers::SHIFT) {
                    self.view.scroll.1 = if down { (self.view.scroll.1 + 1).min(cols.saturating_sub(1)) } else { self.view.scroll.1.saturating_sub(1) };
                } else {
                    // Three shown rows: those the filter hides do not count.
                    let top = self.rows_from(self.view.scroll.0, 1).first().copied().unwrap_or(self.view.scroll.0);
                    self.view.scroll.0 = if down { self.step_row(top, 3).min(rows.saturating_sub(1)) } else { self.step_row(top, -3) };
                }
                self.view.follow = false;
            }
            MouseEventKind::ScrollRight => {
                self.view.scroll.1 = (self.view.scroll.1 + 1).min(self.dims().1.saturating_sub(1));
                self.view.follow = false;
            }
            MouseEventKind::ScrollLeft => {
                self.view.scroll.1 = self.view.scroll.1.saturating_sub(1);
                self.view.follow = false;
            }
            _ => {}
        }
    }

    fn request_quit(&mut self, force: bool) {
        if self.dirty && !force {
            self.error("unsaved changes – :w to save, :q! to discard");
        } else {
            self.quit = true;
        }
    }

    // ------------------------------------------------------------ commands

    fn find_column(&self, name: &str) -> Option<usize> {
        let cols = &self.wb.sheets[self.view.sheet].columns;
        cols.iter().position(|c| c.name == name).or_else(|| {
            let up = name.to_ascii_uppercase();
            if !up.is_empty() && up.chars().all(|c| c.is_ascii_uppercase()) {
                let i = up.chars().fold(0, |acc, c| acc * 26 + (c as usize - 'A' as usize + 1)) - 1;
                (i < cols.len()).then_some(i)
            } else {
                None
            }
        })
    }

    pub fn command(&mut self, line: &str) {
        let mut words = line.split_whitespace();
        let Some(cmd) = words.next() else { return };
        let args: Vec<&str> = words.collect();
        if self.functions_view() {
            match cmd {
                "delrow" => return self.delete_function(),
                "addrow" => {
                    let new = self.function_rows().iter().position(|r| *r == FnRow::New).unwrap_or(0);
                    self.move_to(new, 0);
                    return self.start_edit(None);
                }
                "addcol" | "delcol" | "sort" | "sortcol" | "pivot" => {
                    return self.error(format!(":{cmd} is not available in the functions view"));
                }
                _ => {}
            }
        }
        if self.calc_view() {
            match cmd {
                "delrow" => return self.delete_calc_lines(),
                "addrow" => return self.insert_calc_line(),
                "addcol" | "delcol" | "sort" | "sortcol" | "pivot" => {
                    return self.error(format!(":{cmd} is not available in the calculation view"));
                }
                _ => {}
            }
        }
        if self.types_view() {
            match cmd {
                "delrow" => return self.delete_type(),
                "addrow" => {
                    let new = self.type_rows().iter().position(|r| *r == TypeRow::New).unwrap_or(0);
                    self.move_to(new, 0);
                    return self.start_edit(None);
                }
                "addcol" | "delcol" | "sort" | "sortcol" | "pivot" => {
                    return self.error(format!(":{cmd} is not available in the types view (Esc to leave)"));
                }
                _ => {}
            }
        }
        let structural = matches!(cmd, "addrow" | "delrow" | "addcol" | "delcol" | "sort" | "sortcol");
        if structural && self.view.pivot.is_some() {
            return self.error(format!(":{cmd} is not available in the pivot view (Ctrl+P to leave)"));
        }
        match cmd {
            // File names may contain spaces.
            "w" => self.save((!args.is_empty()).then(|| args.join(" ")).as_deref()),
            "q" => self.request_quit(false),
            "q!" => self.request_quit(true),
            "wq" | "x" => {
                self.save((!args.is_empty()).then(|| args.join(" ")).as_deref());
                if !self.dirty {
                    self.quit = true;
                }
            }
            "help" | "h" => self.open_help(),
            "print" => {
                // In the layout editor: that layout.
                if let ViewKind::Layout(i) = self.view.kind {
                    self.print_setup.scope = Scope::Layout(i);
                }
                self.open_preview();
            }
            "chart" => self.create_chart(args.first().copied()),
            "export" if args.is_empty() => self.error("usage: :export <file.xlsx|.ods|.csv|.tsv>"),
            "export" => {
                let out = args.join(" ");
                let csv = Path::new(&out).extension().is_some_and(|e| e.eq_ignore_ascii_case("csv") || e.eq_ignore_ascii_case("tsv"));
                // CSV holds one table: the one in the main window.
                let result = if csv {
                    crate::delimited::export(&self.doc, Path::new(&out), Some(self.main_view().sheet))
                        .map(|files| crate::export::ExportReport { files, ..Default::default() })
                } else {
                    crate::export::export_file(&self.doc, Path::new(&out))
                };
                match result {
                    Ok(report) => self.info(format!("exported {out}: {report}")),
                    Err(e) => self.error(e),
                }
            }
            "import" if args.is_empty() => self.error("usage: :import <file.xlsx|.xls|.ods>"),
            // The file name may contain spaces.
            "import" => self.import_sheets(Path::new(&args.join(" "))),
            "overview" if matches!(self.view.kind, ViewKind::Layout(_)) => self.toggle_overview(),
            "overview" => self.error("the overview belongs to the layout editor – open a ▤ tab first"),
            "layout" => {
                let name = args.join(" ");
                self.create_layout((!name.is_empty()).then_some(name.as_str()));
            }
            "pdf" => {
                self.export_pdf(args.first().map(Path::new));
            }
            "report" => self.write_report(args.first().map(Path::new)),
            "bundle" => match args.first().and_then(|a| crate::bundle::Kind::parse(a)) {
                Some(kind) => {
                    let name = self.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                    let stem = crate::bundle::stem(&name);
                    let target = self.path.with_file_name(format!("{stem}.{}", kind.extension()));
                    let target = target.display().to_string();
                    self.save(Some(&target));
                }
                None => self.error("usage: :bundle textbundle|textpack|hcbundle|hcpack"),
            },
            "compress" => match args.first().and_then(|a| crate::storage::Compression::parse(a)) {
                Some(c) => {
                    // Save under the name with the matching suffix.
                    let path = crate::storage::with_compression(&self.path, c);
                    self.compression = c;
                    let name = path.display().to_string();
                    self.save(Some(&name));
                }
                None if args.is_empty() => self.info(format!("compression: {} (none | gzip | zstd | xz)", self.compression.name())),
                None => self.error("usage: :compress none|gzip|zstd|xz"),
            },
            "keytest" => self.start_key_test(),
            "undo" => self.undo(),
            "redo" => self.redo(),
            "goto" | "g" => self.goto(&args.join(" ")),
            "back" => self.back(),
            "slice" | "scenario" if args.is_empty() => self.error(format!("usage: :{cmd} Name – e.g. :slice February, :scenario Optimistisch")),
            "slice" => self.new_data_layer(&args.join(" "), crate::model::LayerKind::Slice),
            "scenario" => self.new_data_layer(&args.join(" "), crate::model::LayerKind::Scenario),
            "z" if args.first() == Some(&"rename") && args.len() > 1 => self.rename_data_layer(&args[1..].join(" ")),
            "z" if args == ["delete"] => self.delete_data_layer(),
            "z" => self.goto_layer((!args.is_empty()).then(|| args.join(" ")).as_deref()),
            "layer" if args.len() == 2 && args[0] == "new" => self.new_layer(args[1]),
            "layer" if args.len() > 2 && args[0] == "new" => self.error("a layer name is one word (months, variants: :slice Name, :scenario Name)"),
            "layer" if args == ["new"] && !self.layer_names.iter().any(|n| n == "new") => {
                self.error("usage: :layer new <name> – e.g. format, validate, filter, comments")
            }
            "layer" => match args.first() {
                Some(name) => {
                    self.toggle_layer(name);
                    let state = if self.layer_visible(name) { "shown" } else { "hidden" };
                    let place = if self.layers_global { "in all tables" } else { "in this table" };
                    self.info(format!("layer {name} {state} {place}"));
                }
                None => self.error("usage: :layer <name>"),
            },
            "bold" | "italic" | "underline" if args.is_empty() => {
                let family = match cmd { "bold" => "bold", "italic" => "italic", _ => "underline" };
                self.toggle_style(family);
            }
            "fg" | "bg" => {
                let family = if cmd == "fg" { "fg" } else { "bg" };
                match args.first().map(|a| a.to_lowercase()) {
                    None => self.apply_style(formatting::StyleChange::Color(family, None)),
                    Some(c) if c == "none" => self.apply_style(formatting::StyleChange::Color(family, None)),
                    Some(c) if formatting::valid_color(&c) => self.apply_style(formatting::StyleChange::Color(family, Some(c))),
                    Some(c) => self.error(format!("unknown color '{c}' – {} or #rrggbb", formatting::COLORS.join(", "))),
                }
            }
            "wrap" => match args.first().map(|a| a.to_lowercase()) {
                None => self.apply_style(formatting::StyleChange::Wrap(Some(0))),
                Some(a) if a == "on" => self.apply_style(formatting::StyleChange::Wrap(Some(0))),
                Some(a) if a == "off" || a == "none" => self.apply_style(formatting::StyleChange::Wrap(None)),
                Some(a) => match a.parse::<u16>() {
                    Ok(n) if n >= 1 => self.apply_style(formatting::StyleChange::Wrap(Some(n))),
                    _ => self.error("usage: :wrap [on|off|width]"),
                },
            },
            "plain" => self.apply_style(formatting::StyleChange::Plain),
            "solo" => match args.first() {
                Some(name) if self.layer_names.iter().any(|n| n == name) => self.set_solo(Some(name.to_string())),
                Some(name) => self.error(format!("no layer '{name}' (layers: {})", self.layer_names.join(", "))),
                None => self.set_solo(None),
            },
            "theme" => match args.first().and_then(|a| super::theme::ThemeSetting::parse(a)) {
                Some(t) => {
                    self.theme = t;
                    let detected = match self.detected_light {
                        Some(true) => " (terminal: light)",
                        Some(false) => " (terminal: dark)",
                        None => " (terminal did not answer – dark assumed)",
                    };
                    let note = if t == super::theme::ThemeSetting::Auto { detected } else { "" };
                    self.info(format!("theme {}{note}", args[0].to_lowercase()));
                }
                None => self.error("usage: :theme auto|light|dark"),
            },
            "functions" => self.show_helper(ViewKind::Functions),
            "calc" => self.show_helper(ViewKind::Calc),
            "preview" => self.show_helper(ViewKind::CalcPreview),
            "formulas" => match args.first().and_then(|a| FormulaStyle::parse(a)) {
                Some(st) => {
                    self.formula_style = st;
                    self.info(format!("formulas: {}", st.name()));
                }
                None if args.is_empty() => self.info(format!("formulas: {} (2d | text | ascii)", self.formula_style.name())),
                None => self.error("usage: :formulas 2d|text|ascii"),
            },
            "types" => self.toggle_types(),
            "inspector" => self.show_helper(ViewKind::Inspector),
            "source" => self.show_helper(ViewKind::Source),
            "split" => self.toggle_split(),
            "swap" => self.swap_windows(),
            "copy" => self.copy(),
            "cut" => self.cut(),
            "paste" => {
                let grid = self.clipboard.clone();
                self.paste_grid(grid);
            }
            "menu" => self.open_menu(0),
            "version" => self.info(format!("hardcalc {}", env!("CARGO_PKG_VERSION"))),
            "e" | "edit" | "open" | "e!" | "edit!" | "open!" if args.is_empty() => self.open_file_dialog(Purpose::Open),
            "e" | "edit" | "open" | "e!" | "edit!" | "open!" => self.open_file(&args.join(" "), cmd.ends_with('!')),
            "new" | "new!" => self.new_file(cmd == "new!"),
            "recent" => self.open_recent_dialog(),
            "append" if args.is_empty() => self.open_file_dialog(Purpose::Append),
            "append" => self.append_file(&args.join(" ")),
            "bands" => {
                self.bands = match args.first().copied() {
                    Some("on") => true,
                    Some("off") => false,
                    _ => !self.bands,
                };
                self.info(if self.bands { "banded rows and columns on" } else { "banded rows and columns off" });
            }
            "animations" | "anim" => {
                let on = match args.first().copied() {
                    Some("on") => true,
                    Some("off") => false,
                    _ => !self.anim.enabled,
                };
                self.anim.set_by_hand(on);
                self.info(if self.anim.enabled { "animations on" } else { "animations off" });
            }
            "keys" => {
                self.show_keys = !self.show_keys;
                if !self.show_keys {
                    self.info("key display off");
                }
            }
            "layers" => match args.first().copied() {
                Some("all") => self.layers_global = true,
                Some("table" | "this") => self.layers_global = false,
                _ => self.toggle_layer_scope(),
            },
            "pivot" => match args.as_slice() {
                [] => self.toggle_pivot(None),
                [r, c] => {
                    let axes = match &self.wb.sheets[self.view.sheet].shape {
                        Shape::Tensor(axes) => axes.clone(),
                        _ => Vec::new(),
                    };
                    match (axes.iter().position(|a| a == r), axes.iter().position(|a| a == c)) {
                        (Some(r), Some(c)) if r != c => self.toggle_pivot(Some((r, c))),
                        _ => self.error(format!("axes are: {}", axes.join(", "))),
                    }
                }
                _ => self.error("usage: :pivot [row-axis col-axis]"),
            },
            "addrow" if self.cursor_footer().is_some() => self.add_footer(None),
            "addrow" => {
                let (rows, cols) = self.dims();
                let at = if rows == 0 { 0 } else { (self.view.cursor.0 + 1).min(rows) };
                let sheet = self.view.sheet;
                let calculates = self.calculates();
                let names: Vec<String> = self.wb.sheets[sheet].columns.iter().map(|c| c.name.clone()).collect();
                // References to the rows below follow them.
                let moved: Vec<usize> = (0..rows).map(|r| if r >= at { r + 1 } else { r }).collect();
                let same: Vec<usize> = (0..cols).collect();
                self.with_doc(|doc| {
                    crate::refs::update_document(doc, sheet, &crate::refs::Moves { rows: &moved, cols: &same });
                    if let Some(t) = table_mut(doc, sheet) {
                        data_layers::pad_layers(t);
                        let cols = t.columns.len();
                        for grid in t.grids_mut() {
                            grid.insert(at, vec![String::new(); cols]);
                        }
                        // The new row gets the formulas of the calculated columns,
                        // also in the slices and scenarios.
                        for c in 0..t.columns.len() {
                            if calculates && let Some((base, src)) = calc_column::column_formula(&t.rows, c, &names, Some(at)) {
                                t.rows[at][c] = calc_column::formula_in_row(&src, base, at, &names);
                            }
                        }
                        if calculates {
                            calc_column::extend_layer_formulas(t, at..at + 1, &names);
                        }
                    }
                });
                self.move_to(at, self.view.cursor.1);
            }
            "delrow" if self.cursor_footer().is_some() => {
                let ((r0, _), (r1, _)) = self.selection();
                let ks: Vec<usize> = (r0..=r1).filter_map(|r| self.footer_at(r)).collect();
                let labels: Vec<String> = ks.iter().map(|&k| self.table().footer[k].label.clone()).collect();
                self.with_table(|t| {
                    for &k in ks.iter().rev() {
                        t.footer.remove(k);
                    }
                });
                self.view.anchor = None;
                self.view.header = None;
                self.clamp_cursor();
                self.info(format!("deleted footer {}", labels.join(", ")));
            }
            "footer" => {
                if self.view.kind != ViewKind::Table || self.view.pivot.is_some() {
                    return self.error(":footer works in tables");
                }
                self.add_footer((!args.is_empty()).then(|| args.join(" ")));
            }
            "delrow" => {
                let ((r0, _), (r1, _)) = self.selection();
                let rows = self.dims().0;
                if r0 >= rows {
                    return self.error("no data row selected");
                }
                let r1 = r1.min(rows - 1);
                // Rows the filter hides stay.
                let doomed = self.shown_in(r0, r1);
                self.delete_rows(&doomed);
                self.view.anchor = None;
                if self.on_header().is_some() {
                    self.view.cursor.0 = r0;
                    if r0 >= self.dims().0 {
                        self.view.header = None;
                    }
                }
                self.snap_to_shown();
                self.info(format!("deleted {} row(s)", doomed.len()));
            }
            "addcol" => {
                let Some(spec) = args.first() else { return self.error("usage: :addcol Name[:Type]") };
                let (name, ty) = crate::parse::split_header(spec);
                let (rows, cols) = self.dims();
                let at = (self.view.cursor.1 + 1).min(cols);
                let sheet = self.view.sheet;
                // References to the columns on the right follow them.
                let same: Vec<usize> = (0..rows).collect();
                let moved: Vec<usize> = (0..cols).map(|c| if c >= at { c + 1 } else { c }).collect();
                self.with_doc(|doc| {
                    crate::refs::update_document(doc, sheet, &crate::refs::Moves { rows: &same, cols: &moved });
                    let Some(t) = table_mut(doc, sheet) else { return };
                    t.columns.insert(at, Column { name, ty, align: Align::None });
                    for row in t.grids_mut().flatten() {
                        if at <= row.len() {
                            row.insert(at, String::new());
                        }
                    }
                    for f in &mut t.footer {
                        f.cells.insert(at.min(f.cells.len()), String::new());
                    }
                    sync_shape(t);
                });
                self.move_to(self.view.cursor.0, at);
            }
            "delcol" => {
                // On the column header: all selected columns.
                let (c0, c1) = match self.on_header() {
                    Some(Header::Column) => {
                        let ((_, c0), (_, c1)) = self.selection();
                        (c0, c1)
                    }
                    _ => (self.view.cursor.1, self.view.cursor.1),
                };
                let cols = self.dims().1;
                if c1 >= cols {
                    return self.error("no data column selected");
                }
                if c1 - c0 + 1 >= cols {
                    return self.error("cannot delete the last column");
                }
                // References and format rules follow the columns on the right; those of the deleted ones go.
                let gone = c1 - c0 + 1;
                let moved: Vec<usize> = (0..cols).map(|c| if c < c0 { c } else if c <= c1 { crate::refs::GONE } else { c - gone }).collect();
                let same: Vec<usize> = (0..self.dims().0).collect();
                let sheet = self.view.sheet;
                self.with_doc(|doc| {
                    crate::refs::update_document(doc, sheet, &crate::refs::Moves { rows: &same, cols: &moved });
                    let Some(t) = table_mut(doc, sheet) else { return };
                    t.columns.drain(c0..=c1);
                    let layer_rows = t.layers.iter_mut().flat_map(|l| &mut l.rows);
                    for row in t.rows.iter_mut().chain(t.footer.iter_mut().map(|f| &mut f.cells)).chain(layer_rows) {
                        if c0 < row.len() {
                            row.drain(c0..=c1.min(row.len() - 1));
                        }
                    }
                    sync_shape(t);
                });
                self.view.anchor = None;
                self.view.cursor.1 = c0.min(self.dims().1 - 1);
            }
            "sort" | "sortcol" => {
                // `:sort` sorts the rows by the cursor column (in a selected block: only the block's
                // rows); `:sortcol` sorts each column on its own. Repeating it flips to descending.
                let single = cmd == "sortcol";
                let (rows, cols) = self.dims();
                // Several keys, separated by commas: `:sort Status, Amount desc` (or `-Amount`).
                let text = args.join(" ");
                let mut more_keys: Vec<(usize, bool)> = Vec::new();
                if !single && text.contains(',') {
                    for part in text.split(',').map(str::trim).filter(|p| !p.is_empty()) {
                        // A column named as a whole wins over a direction (`Job description`).
                        let (name, down) = match (part.strip_prefix('-'), part.rsplit_once(' ')) {
                            _ if self.find_column(part).is_some() => (part, false),
                            (Some(name), _) => (name.trim(), true),
                            (None, Some((name, dir))) if dir.starts_with("desc") => (name.trim(), true),
                            (None, Some((name, dir))) if dir.starts_with("asc") => (name.trim(), false),
                            _ => (part, false),
                        };
                        match self.find_column(name) {
                            Some(c) => more_keys.push((c, down)),
                            None => return self.error(format!("no column '{name}' – :sort Status, Amount desc")),
                        }
                    }
                }
                let args: Vec<&str> = if more_keys.is_empty() { args } else { Vec::new() };
                let col = match (more_keys.first(), args.first()) {
                    (Some((c, _)), _) => Some(*c),
                    (None, Some(a)) => self.find_column(a),
                    (None, None) => (self.view.cursor.1 < cols).then_some(self.view.cursor.1),
                };
                let Some(col) = col else {
                    return self.error(format!("usage: :{cmd} [column] [desc] – or place the cursor in a data column"));
                };
                let ((mut r0, mut c0), (mut r1, mut c1)) = ((0, 0), (rows.saturating_sub(1), cols.saturating_sub(1)));
                if self.has_range() {
                    let ((sr0, sc0), (sr1, sc1)) = self.selection();
                    (r0, c0, r1, c1) = (sr0, sc0.min(cols - 1), sr1.min(rows.saturating_sub(1)), sc1.min(cols - 1));
                } else if single {
                    (c0, c1) = (col, col);
                }
                if rows == 0 || r0 >= r1 && !single {
                    return self.error("nothing to sort");
                }
                let first = if single { c0 } else { col };
                let desc = match args.get(1) {
                    Some(a) => a.starts_with("desc"),
                    None => more_keys.is_empty() && args.is_empty() && self.last_sort == Some((self.view.sheet, first, false, single)),
                };
                let s = self.view.sheet;
                let compare = |x: &Value, y: &Value, desc: bool| {
                    use std::cmp::Ordering;
                    let o = match (x, y) {
                        (Value::Null, Value::Null) => Ordering::Equal,
                        // Empty cells always go last.
                        (Value::Null, _) => return Ordering::Greater,
                        (_, Value::Null) => return Ordering::Less,
                        (Value::Num(x, _), Value::Num(y, _)) => x.partial_cmp(y).unwrap_or(Ordering::Equal),
                        (Value::Date(x), Value::Date(y)) => x.cmp(y),
                        (x, y) => x.display(None, false).cmp(&y.display(None, false)),
                    };
                    if desc { o.reverse() } else { o }
                };
                // The order of rows r0..=r1 by one column: order[new] = old (relative to r0).
                let order_by = |keys: Vec<Value>| -> Vec<usize> {
                    let mut order: Vec<usize> = (0..keys.len()).collect();
                    order.sort_by(|&a, &b| compare(&keys[a], &keys[b], desc));
                    order
                };
                // By several columns: the first decides, the next ones break its ties.
                let order_by_keys = |keys: Vec<(Vec<Value>, bool)>| -> Vec<usize> {
                    let mut order: Vec<usize> = (0..keys.first().map_or(0, |k| k.0.len())).collect();
                    order.sort_by(|&a, &b| keys.iter().map(|(k, down)| compare(&k[a], &k[b], *down)).find(|o| o.is_ne()).unwrap_or(std::cmp::Ordering::Equal));
                    order
                };
                // (column range, order) pairs to apply.
                let jobs: Vec<(usize, usize, Vec<usize>)> = if !more_keys.is_empty() {
                    let keys = more_keys.iter().map(|&(c, down)| ((r0..=r1).map(|r| self.wb.value((s, r, c))).collect(), down)).collect();
                    vec![(c0, c1, order_by_keys(keys))]
                } else if single {
                    (c0..=c1).map(|c| (c, c, order_by((r0..=r1).map(|r| self.wb.value((s, r, c))).collect()))).collect()
                } else {
                    vec![(c0, c1, order_by((r0..=r1).map(|r| self.wb.value((s, r, col))).collect()))]
                };
                let whole_rows = !single && c0 == 0 && c1 + 1 == cols;
                self.with_doc(|doc| {
                    if whole_rows {
                        // References follow their rows (only when whole rows move).
                        let mut moved: Vec<usize> = (0..rows).collect();
                        for (new, &old) in jobs[0].2.iter().enumerate() {
                            moved[r0 + old] = r0 + new;
                        }
                        let all: Vec<usize> = (0..cols).collect();
                        crate::refs::update_document(doc, s, &crate::refs::Moves { rows: &moved, cols: &all });
                    } else {
                        // Formatting (rules for single cells) follows its cell; formulas stay.
                        let places: Vec<(usize, usize, Vec<usize>)> = jobs
                            .iter()
                            .map(|(ca, cb, order)| {
                                let mut new_of = vec![0; order.len()];
                                for (new, &old) in order.iter().enumerate() {
                                    new_of[old] = new;
                                }
                                (*ca, *cb, new_of)
                            })
                            .collect();
                        crate::refs::move_cell_targets(doc, s, &|r, c| {
                            let (_, _, new_of) = places.iter().find(|(ca, cb, _)| (*ca..=*cb).contains(&c))?;
                            (r0..=r1).contains(&r).then(|| (r0 + new_of[r - r0], c))
                        });
                    }
                    if let Some(t) = table_mut(doc, s) {
                        data_layers::pad_layers(t);
                        for grid in t.grids_mut() {
                            for (ca, cb, order) in &jobs {
                                let old: Vec<Vec<String>> = (r0..=r1)
                                    .map(|r| (*ca..=*cb).map(|c| grid.get(r).and_then(|row| row.get(c)).cloned().unwrap_or_default()).collect())
                                    .collect();
                                for (new, &o) in order.iter().enumerate() {
                                    if let Some(row) = grid.get_mut(r0 + new) {
                                        row.resize(row.len().max(*cb + 1), String::new());
                                        row[*ca..=*cb].clone_from_slice(&old[o]);
                                    }
                                }
                            }
                        }
                    }
                });
                self.last_sort = Some((self.view.sheet, first, desc, single));
                if !more_keys.is_empty() {
                    self.last_sort = None;
                    let cols = &self.wb.sheets[self.view.sheet].columns;
                    let keys: Vec<String> = more_keys.iter().map(|&(c, down)| format!("{}{}", cols[c].name, if down { " ↓" } else { " ↑" })).collect();
                    return self.info(format!("sorted by {}", keys.join(", then ")));
                }
                let name = self.wb.sheets[self.view.sheet].columns[first].name.clone();
                let what = if single && c1 > c0 { "columns sorted individually".to_string() } else { format!("sorted by {name}") };
                let dir = if desc { "descending" } else { "ascending – again for descending" };
                self.info(format!("{what} {dir}"));
            }
            "table" | "newtable" | "playground" => {
                let name = if args.is_empty() { format!("Sheet{}", self.wb.sheets.len() + 1) } else { table_name(&args.join(" ")) };
                if !valid_table_name(&name) {
                    return self.error(format!("'{name}' is not a valid table name (letters, digits, spaces, _ and -)"));
                }
                if self.wb.sheet_index(&name).is_some() {
                    return self.error(format!("table {name} already exists"));
                }
                let mut table = new_table(&name);
                if cmd == "playground" {
                    table.shape = Shape::Playground;
                    table.columns.truncate(1);
                    table.rows.clear();
                }
                self.snapshot();
                self.doc.blocks.push(Block::Table(table));
                self.dirty = true;
                self.rebuild();
                self.switch_sheet(self.wb.sheets.len() - 1);
            }
            "hide" => self.hide_columns(&args),
            "unhide" => self.unhide_columns(&args),
            "width" => self.set_width(&args),
            "freeze" => self.freeze_columns(&args),
            "find" => self.find(Some(line.trim_start()[cmd.len()..].trim().to_string()), false),
            "next" => self.find(None, false),
            "prev" => self.find(None, true),
            "replace" => self.replace(&search::split_args(&line.trim_start()[cmd.len()..])),
            "filter" => self.filter_command(&args),
            "when" => self.when_command(&args),
            "view" => self.view_command(&args),
            "duplicates" => self.show_duplicates(&args),
            "group" => self.group_by(&args),
            "dedupe" => self.dedupe(&args),
            "values" if args.is_empty() => self.open_values(None),
            "values" => match self.find_column(&args.join(" ")) {
                Some(c) => self.open_values(Some(c)),
                None => self.error(format!("no column '{}'", args.join(" "))),
            },
            "filldown" => self.fill_down(),
            "fillright" => self.fill_right(),
            "shape" => self.set_shape(args.first().copied()),
            "autofill" => self.autofill_to(args.first().and_then(|a| crate::engine::parse_a1(&a.to_uppercase()))),
            "type" => {
                if self.view.kind != ViewKind::Table || self.view.pivot.is_some() {
                    return self.error(":type works in tables");
                }
                let c = self.view.cursor.1;
                let spec = args.join(" ");
                let column = tables(&self.doc).nth(self.view.sheet).and_then(|t| t.columns.get(c)).cloned();
                if spec.is_empty() {
                    let name = column.as_ref().map_or_else(|| col_letters(c), |col| col.name.clone());
                    let ty = column.and_then(|col| col.ty).unwrap_or_else(|| "none (auto)".into());
                    return self.info(format!("column {name}: type {ty} – :type <Type> sets it, :type none clears it"));
                }
                let ty = match spec.as_str() {
                    "none" | "-" | "auto" => None,
                    t => match self.wb.types.resolve(t) {
                        Ok(_) => Some(t.to_string()),
                        Err(e) => return self.error(format!("type {t}: {e}")),
                    },
                };
                let (rows, sheet) = (self.dims().0, self.view.sheet);
                let shown = ty.clone().unwrap_or_else(|| "none".into());
                self.with_table(|t| {
                    grow(t, rows, c + 1);
                    t.columns[c].ty = ty;
                });
                let name = self.wb.sheets[sheet].columns[c].name.clone();
                let bad = self.wb.sheets[sheet].cells.iter().filter(|row| matches!(row.get(c), Some(crate::engine::Cell::Value(Value::Error(_))))).count();
                if bad > 0 {
                    self.error(format!("column {name}: type {shown} – {bad} value(s) do not fit"));
                } else {
                    self.info(format!("column {name}: type {shown}"));
                }
            }
            "compact" => {
                if self.view.kind != ViewKind::Table || self.view.pivot.is_some() {
                    return self.error(":compact works in tables");
                }
                let on = match args.first().copied() {
                    Some("on") => true,
                    Some("off") => false,
                    None => !self.table().compact,
                    _ => return self.error("usage: :compact [on|off]"),
                };
                if !on && (self.table().name.is_none() || self.table().heading_level.is_none()) {
                    return self.error("a table without a heading is always compact – only a heading can say `aligned`");
                }
                if self.table().compact != on {
                    self.with_table(|t| t.compact = on);
                }
                self.info(if on {
                    "compact (the default): saved without padding – a changed value changes only its line in a diff"
                } else {
                    "aligned: the table is saved with padded columns (`aligned` in the heading)"
                });
            }
            "fill" => {
                if self.view.kind != ViewKind::Table || self.view.pivot.is_some() {
                    return self.error(":fill works in tables");
                }
                let on = match args.first().copied() {
                    Some("on") => true,
                    Some("off") => false,
                    _ => !self.fills(),
                };
                // Saved in the heading (`fill` / `nofill`) only where it differs from the shape's default.
                let fill = (on != self.fills_by_default()).then_some(on);
                if self.table().fill != fill {
                    self.with_table(|t| t.fill = fill);
                }
                self.clamp_cursor();
                let footers = !self.wb.sheets[self.view.sheet].footer.is_empty();
                self.info(match (on, footers) {
                    (true, true) => "table fills the window with empty cells – its footer rows are hidden meanwhile",
                    (true, false) => "table fills the window with empty cells",
                    (false, _) => "table shows one + row and column",
                });
            }
            other => {
                if parse_a1(other).is_some() {
                    self.goto(other);
                } else {
                    self.error(format!("unknown command :{other} (F1 for help)"));
                }
            }
        }
    }
}

/// Left window width (percent) that puts the separator at screen column `x`,
/// kept within 40–60.
pub fn split_percent_at(body: Rect, x: u16) -> u16 {
    if body.width == 0 {
        return 50;
    }
    let rel = u32::from(x.saturating_sub(body.x));
    let pct = (rel * 100 + u32::from(body.width) / 2) / u32::from(body.width);
    pct.clamp(40, 60) as u16
}
