//! The one list of commands: `:` commands (with their arguments, for
//! completion and the F1 help) and the menu (F9 or a click on `hardcalc`),
//! whose entries run those commands.

/// A `:` command.
pub struct Command {
    pub name: &'static str,
    /// Argument syntax as shown, e.g. `[file]`, `auto|light|dark`.
    pub args: &'static str,
    pub help: &'static str,
    /// Fixed argument values offered for completion.
    pub choices: &'static [&'static str],
}

const fn c(name: &'static str, args: &'static str, help: &'static str) -> Command {
    Command { name, args, help, choices: &[] }
}

const fn choice(name: &'static str, args: &'static str, help: &'static str, choices: &'static [&'static str]) -> Command {
    Command { name, args, help, choices }
}

pub const COMMANDS: &[Command] = &[
    c("w", "[file]", "write (save); a new document asks for its name"),
    c("q", "", "quit"),
    c("q!", "", "quit without saving"),
    c("wq", "[file]", "write and quit"),
    c("new", "", "new document with a playground"),
    c("e", "[file]", "open a file (without a name: the file dialog)"),
    c("e!", "<file>", "open a file, discarding unsaved changes"),
    c("recent", "", "the recently opened files"),
    c("append", "[file]", "add the tables, charts, layouts … of a file (xlsx/ods/csv: import)"),
    c("import", "<file>", "add the sheets of a spreadsheet as tables"),
    c("export", "<file>", "write as .xlsx, .ods, .csv or .tsv"),
    choice("bundle", "textbundle|textpack|hcbundle|hcpack", "save as TextBundle (folder) / TextPack (zip) with the tables as TSV", &["hcbundle", "hcpack", "textbundle", "textpack"]),
    choice("compress", "none|gzip|zstd|xz", "save compressed as .hcmd.gz / .hcmd.zst / .hcmd.xz (none: plain .hcmd)", &["zstd", "gzip", "xz", "none"]),
    c("pdf", "[file]", "write the document as PDF"),
    c("print", "", "print preview"),
    c("report", "[file]", "calculation report: PDF with typeset formulas (.typ: the Typst source)"),
    c("undo", "", "undo the last change"),
    c("redo", "", "redo"),
    c("copy", "", "copy the selection (Ctrl+C)"),
    c("cut", "", "copy the selection and clear it (Ctrl+X)"),
    c("paste", "", "paste at the cursor"),
    c("goto", "<cell>", "jump to a cell, e.g. :goto B5, :goto Expenses[Rent, Amount] (Ctrl+Z jumps back)"),
    c("back", "", "back from the last jump; changes made since stay"),
    c("find", "<text>", "go to the next cell containing the text, in values and formulas, through all tables (Ctrl+F; Ctrl+G: the next match)"),
    c("next", "", "the next match of the last :find (Ctrl+G)"),
    c("prev", "", "the match before"),
    c("replace", "<old> <new>", "replace a text in the selection, else in the whole table – values and formulas; words with spaces in \"quotes\""),
    c("sort", "[column] [desc] | col, col desc …", "sort the rows by a column, in a selection only its rows (again: descending); several columns with commas: :sort Status, Amount desc"),
    c("sortcol", "[column] [desc]", "sort each column (of the selection) on its own"),
    c("addrow", "", "insert a row below"),
    c("delrow", "", "delete the selected rows"),
    c("duplicates", "[Column]", "show the rows whose value in the column occurs more than once (as a filter)"),
    c("dedupe", "[Column …]", "delete the rows that repeat an earlier one – in the named columns, else in all; the first stays"),
    c("footer", "[Label]", "add a footer row (sum, avg … per column) below the table"),
    c("addcol", "<Name[:Type]>", "insert a column to the right"),
    c("delcol", "", "delete the column (on the header: the selected ones)"),
    c("hide", "[Column …]", "hide columns (without a name: those of the selection); they stay in the file – a `view` layer keeps the rule"),
    c("unhide", "[Column …]", "show hidden columns again (without a name: all)"),
    choice("width", "<n|auto>", "width of the selected columns in characters (auto: as the content needs)", &["auto"]),
    choice("freeze", "[Column|off]", "the columns up to this one stay in place while scrolling sideways (off: none)", &["off"]),
    c("filldown", "", "first row of the selection into the rest (Ctrl+D)"),
    c("fillright", "", "first column of the selection into the rest (Ctrl+R)"),
    c("autofill", "<cell>", "continue the selection up to a cell: numbers, dates, Item 1, months …"),
    c("values", "[Column]", "the values of a column as a list to tick: what is ticked stays shown (the filter; also ▾ in the header, Space on it)"),
    choice("filter", "[Column [condition]] | off [Column]", "show only the rows that meet a condition: :filter Status = open, :filter Amount >= 100, in(a, b), contains(text), empty; without a condition: the value at the cursor; off: all rows", &["off"]),
    choice("type", "[Type|none]", "data type of the cursor column (also Name:Type on its header)", &["int", "decimal(2)", "float", "text", "bool", "date", "none"]),
    c("table", "[Name]", "new table"),
    c("playground", "[Name]", "new free sheet that fills the window"),
    choice("chart", "[type]", "chart of the selected columns", &["bar", "line", "area", "scatter", "histogram"]),
    c("layout", "[Name]", "new raster layout"),
    c("overview", "", "layout editor: overview (elements as boxes) on / off"),
    c("group", "[Column]", "new table with one row per value of the column: how many rows have it and the sums of the number columns (live formulas)"),
    c("pivot", "[rows cols]", "pivot view of a tensor"),
    c("split", "", "open / close the right window"),
    c("swap", "", "swap the windows"),
    c("types", "", "the types tab"),
    c("functions", "", "the functions tab"),
    c("calc", "", "the calculation sheet (```hardcalc blocks)"),
    c("preview", "", "the calculation sheet typeset, in the right window"),
    c("plot", "", "calculation sheet: settings of the chart at the cursor, or a new one (F7)"),
    c("library", "", "calculation sheet: insert a formula from the library (F6)"),
    choice("formulas", "2d|text|ascii", "how the preview draws formulas (f in the preview)", &["2d", "text", "ascii"]),
    c("inspector", "", "details of the cell in the right window"),
    c("source", "", "the Markdown source in the right window"),
    c("layer", "<name> | new <name>", "show / hide a layer; new: add one to this table (format, validate, filter, comments)"),
    c("bold", "", "bold on / off for the selection (Ctrl+B)"),
    c("italic", "", "italic on / off for the selection"),
    c("underline", "", "underline on / off for the selection"),
    choice("fg", "<color|#rrggbb|none>", "text color of the selection", crate::tui::app::formatting::COLORS),
    choice("bg", "<color|#rrggbb|none>", "fill color of the selection", crate::tui::app::formatting::COLORS),
    choice("wrap", "[on|off|width]", "break long text of the selection into lines (on: default width of 40 characters)", &["on", "off"]),
    c("plain", "", "remove bold, italic, underline and colors from the selection"),
    c("when", "<Column> <condition> <format|off>", "format the cells of a column that meet a condition: :when Amount > 1000 fg red bold · :when Status = open bg yellow"),
    c("slice", "<Name>", "stack a slice on the table: own values, e.g. a month"),
    c("scenario", "<Name>", "stack a scenario on the table: only changes, the rest shows through"),
    c("z", "[b|Name] | rename Name | delete", "show b, a slice or scenario (without: list them); rename / delete the shown one; also Alt+↑/↓"),
    choice("view", "[Name] | save Name | delete Name | off", "named views: save the filter and the column settings of the table under a name and show them again (without a name: list them; off: all rows and columns)", &["save ", "delete ", "off"]),
    choice("layers", "all|table", "layer switches for all tables / this table", &["all", "table"]),
    c("solo", "[layer]", "layer view (without a name: back)"),
    choice("shape", "[matrix|playground]", "turn a playground into a table (one connected area) or back", &["matrix", "playground"]),
    choice("compact", "[on|off]", "compact is the default (small diffs in Git); off: aligned, padded columns", &["on", "off"]),
    choice("fill", "[on|off]", "fill the table to the window edges", &["on", "off"]),
    choice("bands", "[on|off]", "shade alternate rows and columns", &["on", "off"]),
    choice("animations", "[on|off]", "sliding menus, turning tables, flowing formula sources (also :anim)", &["on", "off"]),
    choice("theme", "auto|light|dark", "colour scheme", &["auto", "light", "dark"]),
    c("keys", "", "show the keys hardcalc receives"),
    c("keytest", "", "test Menu key, Shift+F10 and right-click, so the help shows what works"),
    c("help", "", "the help (F1)"),
    c("menu", "", "the menu (F9)"),
    c("version", "", "show the version"),
];

/// Commands whose argument is a file name (completed from the folder).
pub const FILE_COMMANDS: &[&str] = &["w", "wq", "e", "e!", "edit", "edit!", "open", "open!", "append", "import", "export", "pdf", "report"];

pub const MENUS: [&str; 5] = ["File", "Edit", "View", "Insert", "Help"];

/// A menu entry: the command line it runs (ending in a space: the command
/// line opens with it, for the argument), and the key shown beside it.
pub struct MenuItem {
    pub menu: usize,
    pub label: &'static str,
    pub run: &'static str,
    pub key: &'static str,
}

const fn m(menu: usize, label: &'static str, run: &'static str, key: &'static str) -> MenuItem {
    MenuItem { menu, label, run, key }
}

pub const MENU_ITEMS: &[MenuItem] = &[
    m(0, "New", "new", ":new"),
    m(0, "Open…", "e", "F3"),
    m(0, "Recent files…", "recent", ":recent"),
    m(0, "Append / import…", "append", ":append"),
    m(0, "Save", "w", "F2"),
    m(0, "Save as…", "w ", ":w file"),
    m(0, "Export…", "export ", ":export"),
    m(0, "Save compressed (zstd)", "compress zstd", ":compress"),
    m(0, "Save as bundle…", "bundle ", ":bundle"),
    m(0, "Print preview", "print", ":print"),
    m(0, "Write PDF", "pdf", ":pdf"),
    m(0, "Calculation report", "report", ":report"),
    m(0, "Quit", "q", "F10"),
    m(1, "Undo", "undo", "Ctrl+Z"),
    m(1, "Redo", "redo", "Ctrl+Y"),
    m(1, "Copy", "copy", "Ctrl+C"),
    m(1, "Cut", "cut", "Ctrl+X"),
    m(1, "Paste", "paste", "Ctrl+V"),
    m(1, "Sort by column", "sort", "F7"),
    m(1, "Sort columns individually", "sortcol", ":sortcol"),
    m(1, "Filter by this value", "filter", ":filter"),
    m(1, "Filter: choose values…", "values", "Space on ▾"),
    m(1, "Filter by condition…", "filter ", ":filter"),
    m(1, "Remove filter", "filter off", ":filter off"),
    m(1, "Insert row", "addrow", "F5"),
    m(1, "Delete rows", "delrow", "F8"),
    m(1, "Show duplicates in column", "duplicates", ":duplicates"),
    m(1, "Delete repeated rows", "dedupe", ":dedupe"),
    m(1, "Add footer row", "footer", ":footer"),
    m(1, "Insert column…", "addcol ", "F6"),
    m(1, "Delete column", "delcol", ":delcol"),
    m(1, "Column type…", "type ", ":type"),
    m(1, "Fill down", "filldown", "Ctrl+D"),
    m(1, "Fill right", "fillright", "Ctrl+R"),
    m(1, "Auto fill to…", "autofill ", "drag ▪"),
    m(1, "Go to cell…", "goto ", "F4"),
    m(1, "Find…", "find ", "Ctrl+F"),
    m(1, "Find next", "next", "Ctrl+G"),
    m(1, "Replace…", "replace ", ":replace"),
    m(1, "Bold", "bold", "Ctrl+B"),
    m(1, "Italic", "italic", ":italic"),
    m(1, "Underline", "underline", ":underline"),
    m(1, "Text color…", "fg ", ":fg"),
    m(1, "Fill color…", "bg ", ":bg"),
    m(1, "Plain (no formatting)", "plain", ":plain"),
    m(1, "Format by condition…", "when ", ":when"),
    m(1, "Table ↔ playground…", "shape ", ":shape"),
    m(2, "Split window", "split", "Ctrl+O"),
    m(2, "Swap windows", "swap", "Ctrl+U"),
    m(2, "Inspector", "inspector", ""),
    m(2, "Source", "source", ""),
    m(2, "Types", "types", ":types"),
    m(2, "Functions", "functions", ""),
    m(2, "Calculation sheet", "calc", ":calc"),
    m(2, "Calculation preview", "preview", ":preview"),
    m(2, "Formula library…", "library", ":library"),
    m(2, "Pivot", "pivot", "Ctrl+P"),
    m(2, "Show a saved view…", "view ", ":view"),
    m(2, "Save this view…", "view save ", ":view save"),
    m(2, "Hide column", "hide", ":hide"),
    m(2, "Show all columns", "unhide", ":unhide"),
    m(2, "Column width…", "width ", ":width"),
    m(2, "Freeze columns up to here", "freeze", ":freeze"),
    m(2, "Unfreeze columns", "freeze off", ":freeze off"),
    m(2, "Fill window on/off", "fill", ":fill"),
    m(2, "Bands on/off", "bands", ":bands"),
    m(2, "Animations on/off", "animations", ":anim"),
    m(2, "Layers: all tables / this table", "layers", ":layers"),
    m(2, "Theme: auto", "theme auto", ""),
    m(2, "Theme: light", "theme light", ""),
    m(2, "Theme: dark", "theme dark", ""),
    m(2, "Show received keys", "keys", ":keys"),
    m(3, "Table", "table", ":table"),
    m(3, "Playground", "playground", ":playground"),
    m(3, "Group table (counts and sums per value)", "group", ":group"),
    m(3, "Chart of the selection", "chart", ":chart"),
    m(3, "Layout", "layout", ":layout"),
    m(3, "Layer…", "layer new ", ":layer new"),
    m(3, "Slice…", "slice ", ":slice"),
    m(3, "Scenario…", "scenario ", ":scenario"),
    m(4, "Help", "help", "F1"),
    m(4, "Version", "version", ""),
];

/// Entries of menu `menu`.
pub fn items(menu: usize) -> Vec<&'static MenuItem> {
    MENU_ITEMS.iter().filter(|i| i.menu == menu).collect()
}

/// Other names of commands.
pub const ALIASES: &[(&str, &str)] = &[
    ("edit", "e"),
    ("open", "e"),
    ("edit!", "e!"),
    ("open!", "e!"),
    ("g", "goto"),
    ("x", "wq"),
    ("h", "help"),
    ("newtable", "table"),
];

/// The command called `name` (aliases resolved).
pub fn find(name: &str) -> Option<&'static Command> {
    let name = ALIASES.iter().find(|(a, _)| *a == name).map_or(name, |(_, c)| c);
    COMMANDS.iter().find(|c| c.name == name)
}

/// Documentation of a command for the preview: syntax, description and
/// further lines (other names, where it is in the menu, its key).
pub fn doc(name: &str) -> Option<(String, String, Vec<String>)> {
    let c = find(name)?;
    let syntax = format!(":{} {}", c.name, c.args).trim_end().to_string();
    let mut more = Vec::new();
    let aliases: Vec<&str> = ALIASES.iter().filter(|(_, n)| *n == c.name).map(|(a, _)| *a).collect();
    if !aliases.is_empty() {
        more.push(format!("also: :{}", aliases.join("  :")));
    }
    let places: Vec<String> = MENU_ITEMS
        .iter()
        .filter(|i| i.run.split_whitespace().next() == Some(c.name))
        .map(|i| {
            let key = if i.key.is_empty() || i.key.starts_with(':') { String::new() } else { format!(" ({})", i.key) };
            format!("{} → {}{key}", MENUS[i.menu], i.label)
        })
        .collect();
    if !places.is_empty() {
        more.push(format!("menu: {}", places.join(", ")));
    }
    if !c.choices.is_empty() {
        more.push(format!("values: {}", c.choices.join(", ")));
    }
    Some((syntax, c.help.to_string(), more))
}

/// Command line suggestions: (completed text, shown text, description).
/// `extra` supplies dynamic argument values (layer and column names …).
pub fn suggestions(buf: &str, extra: impl Fn(&str) -> Vec<String>) -> Vec<(String, String, String)> {
    match buf.split_once(' ') {
        None => {
            let lower = buf.to_lowercase();
            let starts = COMMANDS.iter().filter(|c| c.name.starts_with(&lower));
            let contains = COMMANDS.iter().filter(|c| !c.name.starts_with(&lower) && c.name.contains(&lower) && !lower.is_empty());
            starts
                .chain(contains)
                .map(|c| {
                    let done = if c.args.is_empty() { c.name.to_string() } else { format!("{} ", c.name) };
                    (done, format!(":{} {}", c.name, c.args).trim_end().to_string(), c.help.to_string())
                })
                .collect()
        }
        Some((cmd, arg)) => {
            if FILE_COMMANDS.contains(&cmd) || arg.contains(' ') {
                return Vec::new();
            }
            let Some(command) = COMMANDS.iter().find(|c| c.name == cmd) else { return Vec::new() };
            let mut values: Vec<String> = command.choices.iter().map(|s| s.to_string()).collect();
            values.extend(extra(cmd));
            let lower = arg.to_lowercase();
            values
                .into_iter()
                .filter(|v| v.to_lowercase().starts_with(&lower) && *v != arg)
                .map(|v| (format!("{cmd} {v}"), format!(":{cmd} {v}"), command.help.to_string()))
                .collect()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_menu_entry_runs_a_known_command() {
        for item in MENU_ITEMS {
            let name = item.run.split_whitespace().next().unwrap();
            assert!(COMMANDS.iter().any(|c| c.name == name), "{}", item.label);
        }
    }

    #[test]
    fn suggestions_complete_names_and_values() {
        let none = |_: &str| Vec::new();
        let s = suggestions("th", none);
        assert_eq!(s[0].0, "theme ");
        assert!(suggestions("", none).len() == COMMANDS.len());
        let s = suggestions("theme l", none);
        assert_eq!(s.iter().map(|x| x.0.as_str()).collect::<Vec<_>>(), ["theme light"]);
        let s = suggestions("solo f", |cmd| if cmd == "solo" { vec!["format".into(), "formulas".into()] } else { vec![] });
        assert_eq!(s.len(), 2);
        assert!(suggestions("e ~/x", none).is_empty(), "file names: Tab");
    }
}
