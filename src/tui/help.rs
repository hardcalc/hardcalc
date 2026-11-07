//! Pages of the F1 help: (title, [(keys, effect)]).

pub type Page = (&'static str, &'static [(&'static str, &'static str)]);

/// All pages: the fixed ones and "Keys", made from the KEYMAP with only
/// the inputs that work in this terminal.
pub fn pages(caps: &super::keymap::Caps) -> Vec<(&'static str, Vec<(String, String)>)> {
    let mut v: Vec<(&'static str, Vec<(String, String)>)> =
        PAGES.iter().map(|(t, e)| (*t, e.iter().map(|(k, d)| (k.to_string(), d.to_string())).collect())).collect();
    let mut keys: Vec<(String, String)> = super::keymap::help_lines(caps).into_iter().map(|(k, l)| (k, l.to_string())).collect();
    keys.push((String::new(), String::new()));
    keys.push(("t".into(), "test keys: Menu key, Shift+F10, right-click – afterwards the help shows what works here".into()));
    v.push(("Keys", keys));
    v
}

pub const PAGES: &[Page] = &[
    (
        "Basics",
        &[
            ("F1 … F10", "the labelled functions in the bottom bar (also clickable)"),
            ("type, Enter, F4, double-click", "edit the cell – typing replaces its content"),
            ("Enter / Tab while editing", "keep the input and go down / right"),
            ("Esc while editing", "cancel the input"),
            ("arrows / click in a formula", "after = ( , or an operator: insert a reference (Shift, drag: range)"),
            ("  then : and arrows", "second corner of a range: B1 : ↓↓ → B1:B3 (no Shift needed)"),
            ("Arrows, Home/End, PgUp/PgDn", "move the cursor"),
            ("Ctrl+Home / Ctrl+End", "first / last data cell"),
            ("Shift+arrows, drag", "select cells"),
            ("↑ in row 1, ← in column A", "onto the header: the whole column / row is selected"),
            ("  click a column / row header", "the same; Shift+arrows or dragging selects more"),
            ("  - / +", "move the column(s) / row(s) – references follow"),
            ("  F8 · Del · ↓ / →", "delete them · clear them · back into the cells"),
            ("  type, Enter on a column header", "name the column (Name or Name:Type) – references follow"),
            ("← / ↑ from the headers, click top left", "table origin: the whole table; type to rename it"),
            ("+ row / + column", "type there to add a row / column"),
            ("Del / Backspace", "clear the selection"),
            ("Ctrl+C / Ctrl+V", "copy / paste (also tab-separated text from other apps)"),
            ("Ctrl+Z / Ctrl+Y", "undo / redo"),
            ("Ctrl+D / Ctrl+R, drag ▪", "fill down / right · continue a series (1, 2 → 3; Jan → Feb)"),
            ("F7, F7 again", "sort by the current column ascending, then descending"),
            ("F2 / Ctrl+S", "save"),
            ("F10 / Ctrl+Q", "quit – with unsaved changes press it twice"),
        ],
    ),
    (
        "Views",
        &[
            ("Tab / Shift+Tab", "next / previous tab: tables, ⚙ types, ƒ functions, layouts"),
            ("F3, ⚙ types", "own data types (+ row adds, F8 deletes) and the built-in ones"),
            ("ƒ functions", "own functions like gross(x) = x * 1.19, and the built-in ones"),
            (":calc, ∑ calc", "calculation sheet: input left, the way to the result right (f_d = f_tk / γ_M, n ≤ f)"),
            (":preview, ∑ preview", "the sheet typeset in the right window; f: 2d / text / ascii · :report writes a PDF"),
            ("Ctrl+P, ⊞ pivot", "pivot view of a tensor: two axes on screen, the others as filters"),
            ("Tab in the pivot", "rotate the dimensions"),
            ("click a filter", "next value of that axis"),
            ("", ""),
            ("Ctrl+O", "split: open / close the right window"),
            ("Tab in the split", "switch between the windows"),
            ("Ctrl+U", "swap the windows (when both show data)"),
            ("ⓘ inspector", "details of the cell on the left: value, type, links, choices"),
            ("‹› source", "the Markdown source, the current table highlighted"),
            ("▥ chart", "chart of the data; t switches the type, a click selects the row"),
        ],
    ),
    (
        "Layers",
        &[
            ("click a layer", "show / hide it"),
            ("⊕ all tables / ⊙ this table", "whether a layer click applies to all tables or this one"),
            ("layer name with *", "this table has its own setting for the layer"),
            ("right-click a layer", "layer view: the cells show the layer instead of the data"),
            ("typing in the layer view", "edits the layer rule of the cell or selection"),
            ("Del in the layer view", "removes the rule"),
            ("right-click again, Esc", "back to the normal view"),
            (":layer new <name>", "add a layer to this table and open its layer view"),
            ("", ""),
            ("slices & scenarios (z axis)", "the table is a; :slice February · :scenario Optimistisch stack b, c …"),
            ("  :z b, click ▤, Alt+↑/↓", "show another one; ↑/↓ on the table origin too"),
            ("  Esc", "back to a, the table itself"),
            ("  B2b  B2.February  B2[-1]", "cell in b / by name / the one before"),
            ("  sum(B2a:c)  B2:4c", "through the stack · rows 2–4 in c"),
            (":z rename Name · :z delete", "the shown slice or scenario"),
            ("  - / + on the table origin", "move it in the stack (a stays first)"),
            ("", ""),
            ("formulas", "built-in: formula cells in cyan italics"),
            ("format", "alignment, colours, bold, percent(n), decimals(n)"),
            ("validate", "rules like >= 0, required, unique – violations shown red"),
            ("comments", "notes on cells – underlined, printed as footnotes"),
        ],
    ),
    (
        "Layouts & printing",
        &[
            ("Layout editor (▤ tab)", ""),
            ("  arrows, PgUp/PgDn", "select an element"),
            ("  type, Enter, double-click", "edit its content"),
            ("  F3 Move", "pick it up – then:"),
            ("    arrows", "move it"),
            ("    + / −  ·  > / <", "taller / shorter (elements below follow) · wider / narrower"),
            ("    Enter, Esc, F3", "put it down"),
            ("  F5 F6 F7 F8", "duplicate · kind · paper · delete (print: menu File)"),
            ("  mouse", "drag, ◢ resizes, draw on empty paper for a new element"),
            ("", ""),
            ("Print preview (F9 menu → File)", ""),
            ("  F2 · F3 · F4", "write PDF · open in a viewer · print"),
            ("  F5 · F6 · F7/F8", "portrait/landscape · paper · font size"),
            ("  F9", "scope: table · whole document · layouts"),
            ("  i", "the real PDF page as image (or block graphics)"),
            ("  PgUp/PgDn, arrows", "pages, scroll"),
        ],
    ),
    (
        "Menu & commands",
        &[
            ("F9, click on ≡ hardcalc", "the menu: File · Edit · View · Insert · Help"),
            ("  ← → ↑ ↓ Enter, letter", "choose · a letter jumps to an entry (the only one runs) · Esc closes"),
            ("", ""),
            (":", "command line – a grid of all commands, and the documentation of the one typed"),
            ("  type, ↑ ↓, Tab", "narrow the list · choose · complete (also values like :theme dark)"),
            ("  Tab after a file command", "completes file names (:e :append :w :import :export :pdf)"),
            ("", ""),
            ("Ctrl+E, :e, :append", "file dialog: Enter opens / appends, ← up, type to search, Ctrl+A all files"),
            ("", "unsaved changes? first: [S]ave · [D]iscard · [Esc] cancel"),
            (":e file  :e! file  :new", "open another file (! discards changes) · new document"),
            (":w [file]  :q  :q!  :wq", "write · quit · quit without saving · write and quit"),
            (":B5", "jump to a cell"),
            (":addrow  :delrow  :addcol Name", "insert / delete rows and columns"),
            (":footer [Label]", "footer row below the table: =sum, =avg, =count …, a formula, value or text"),
            ("", "← on its label renames it, F8 deletes it; formulas: Table.Label.Column"),
            (":table  :playground  :layout", "new table · free sheet · raster layout"),
            (":theme auto|light|dark  :bands", "colour scheme · banded rows and columns"),
        ],
    ),
    (
        "Files & terminal",
        &[
            (":import mappe.xlsx", "adds its sheets as tables (also xls, ods, csv, tsv)"),
            ("", "formulas without an equivalent: value + original as comment"),
            (":export out.xlsx", "all tables with formulas, formats, notes, charts"),
            (":export out.ods / .csv", "LibreOffice / values of the current table"),
            ("hardcalc mappe.xlsx", "on the command line: import and open"),
            ("", ""),
            (":theme auto", "colours follow the terminal (light / dark, also when it switches)"),
            (":theme light | dark", "fixed scheme, the whole screen painted by hardcalc"),
            (":keys", "shows what hardcalc receives for each key"),
            ("", ""),
            ("macOS Terminal.app", "does not send Alt+↑/↓, Shift+↑/↓ and PgUp/PgDn by default:"),
            ("", "use F3 Move with + − > < in layouts, or the mouse"),
            ("", "Alt+arrows: Settings → Profiles → Keyboard, ⌥↑ = \\033[1;3A"),
            ("", "Shift+arrows: the same with ⇧↑ = \\033[1;2A, ⇧↓ = \\033[1;2B"),
        ],
    ),
];
