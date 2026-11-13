# hardcalc

[hardcalc.org](https://hardcalc.org) · [releases](https://github.com/hardcalc/hardcalc/releases) · [file format](SPEC.md)

A universal spreadsheet for the terminal – with mouse support, for macOS,
Linux and Windows. Files are **readable Markdown**: every file can be read in
any editor or Markdown viewer and compared sensibly in Git.

```markdown
## Expenses :: matrix[*, 5]

| Item     | Amount:Euro | Status:Status | Share:decimal(3) | Gross:Euro     |
|----------|------------:|---------------|------------------|----------------|
| Rent     |     1200.00 | paid          | =B1/sum(B)       | =Amount        |
| Power    |       85.50 | open          | =B2/sum(B)       | =Amount * 1.19 |
```

## Principles

1. **Vectors, matrices, tensors** – every table has a shape; tensors have
   named axes and can be rotated as a pivot.
2. **Types with units** – base types, your own types (`Euro = decimal(2)
   unit "€"`), units are checked when calculating (`€ + m` is an error).
3. **Layers** – as in CAD, formatting, validation rules, comments and formula
   highlighting live on their own layers that can be shown and hidden, also
   one at a time (“layer view”).

Plus: your own functions (like `LAMBDA`), charts, running text with live
values, grid layouts for reports, printing and PDF, import and export of
Excel, LibreOffice and CSV, and calculation sheets with units and the full
way to the result.

Documents use the extension `.hcmd`; plain `.md` files are read as well. The
file format is described in [SPEC.md](SPEC.md). An example with everything:
[examples/budget.hcmd](examples/budget.hcmd).

## Installation

Prebuilt binaries for macOS, Linux and Windows are attached to each
[release](https://github.com/hardcalc/hardcalc/releases), with shell and
PowerShell installers and a Homebrew formula:

```bash
# macOS / Linux, Homebrew
brew install hardcalc/tap/hardcalc

# macOS / Linux, installer script
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/hardcalc/hardcalc/releases/latest/download/hardcalc-installer.sh | sh
```

```powershell
# Windows, PowerShell
powershell -ExecutionPolicy Bypass -c "irm https://github.com/hardcalc/hardcalc/releases/latest/download/hardcalc-installer.ps1 | iex"
```

On Windows you can also unpack `hardcalc-x86_64-pc-windows-msvc.zip` (or the
`aarch64` one) from the release and put `hardcalc.exe` in a folder on your
`PATH`.

From source (needs Rust, <https://rustup.rs>):

```bash
cargo install --git https://github.com/hardcalc/hardcalc
# or, in a clone of the repository:
cargo install --path .
```

Both install `hardcalc` and `hardconf` (see below). Without installing, run it
from the project folder:

```bash
cargo run --release -- examples/budget.hcmd
```

### Start menu and file association

```bash
hardcalc desktop install     # start menu entry, .hcmd files open in hardcalc
hardcalc desktop uninstall   # remove both again
```

The installers leave your desktop alone; this command adds hardcalc to the
start menu and associates `.hcmd` files with it, in your own profile and
without administrator rights. hardcalc opens in a terminal window:

- **Linux**: an application entry with `Terminal=true` and the MIME type
  `text/x-hardcalc` below `~/.local/share`.
- **Windows**: a shortcut in the start menu and the file type `hardcalc.hcmd`
  below `HKCU\Software\Classes`; it opens in your default terminal.
- **macOS**: a small app `~/Applications/hardcalc.app` that runs hardcalc in
  Terminal and closes the window when hardcalc ends. On first use macOS asks
  once whether it may control Terminal.

Run it again after moving hardcalc. If the LibreOffice extension is installed
as well, both appear under *Open with*.

Optional: for the image preview when printing, hardcalc renders the PDF page
with `pdftoppm` (poppler) or `mutool` (MuPDF); on macOS the built-in `sips`
is enough. Printing from hardcalc uses `lp` (macOS, Linux).

## Quick start

```bash
hardcalc                           # new, empty document with a playground
hardcalc examples/budget.hcmd      # open
hardcalc new.hcmd                  # new file (created when saving)
hardcalc book.xlsx                 # import an Excel, LibreOffice or CSV file and open it
```

A new document asks for a file name on the first **F2** (`.hcmd` is
added). On the command line, **Tab** completes file names, e.g. for `:e`,
`:append`, `:w` and `:export`.

Typing writes into the cell, **F2** saves, **F10** quits, **F1** shows the
help – pages for basics, views, layers, layouts & printing, commands, files &
terminal and keys, turned with ← → or the page number; it opens at the page
for the current view. As in Midnight Commander, the most important functions
are labelled in the bottom line and can be clicked.

## Usage

### Tables

| F1   | F2   | F3   | F4   | F5     | F6     | F7   | F8     | F9   | F10  |
|------|------|------|------|--------|--------|------|--------|------|------|
| Help | Save | Open | Goto | InsRow | InsCol | Sort | DelRow | Menu | Quit |

The bar always shows what the keys do right now: F8 reads *DelCol* on a
column header, F7 reads *Sort↓* when a second press sorts descending, and a
key without a function has no label. Menus and dialogs bring their own bar –
the file dialog *Open · Recent · All · Name · Date · Places*, the question
about unsaved changes *Save · Discard · Cancel*, a cell being edited
*Accept · Cancel*. Everywhere F1 is the help, F2 confirms and F10 leaves.

**F9** or a click on **≡ hardcalc** at the top left opens the menu
(*File · Edit · View · Insert · Help*) with all functions and their keys;
←→ ↑↓ and Enter choose, a letter jumps to an entry, Esc closes. As soon as
you type **`:`**, hardcalc shows all commands as a grid above the command
line – like the Helix editor; typing narrows them down, and above them the
description of the selected command appears with syntax, other names, menu
place and key. ↑↓ selects, Tab completes, also values like `:theme dark` or
layer and column names.

| Input                            | Effect                                                        |
|----------------------------------|---------------------------------------------------------------|
| typing, Enter, double-click      | edit the cell (typing replaces the content; the input appears in the cell) |
| arrows, click                    | move; Shift+arrows or dragging selects                        |
| while editing a formula: arrows, click | after `=`, `(`, `,` or an operator a reference is inserted (Shift+arrows, dragging: a range; without Shift: type `:` and point at the second corner), as in Excel |
| ↑ in the first row, ← in the first column, click on a column/row header | cursor on the header: the whole column / row is selected; Shift+arrows extend |
| on a header: `-` / `+`           | move the selected columns / rows; references follow           |
| on a header: F8, ↓ / →           | delete columns / rows; back into the cells                    |
| on a column header: typing, Enter | name the column (`Name` or `Name:Type`); references to the old name follow |
| ← / ↑ from the headers, click top left | table origin: the whole table is selected; typing renames the table |
| Tab / Shift+Tab                  | next / previous tab (tables, ⚙ types, ƒ functions, layouts)   |
| `+` row / `+` column             | input there appends a new row / column                        |
| Del / Backspace                  | clear the selection                                           |
| Ctrl+D / Ctrl+R                  | fill the first row / column of the selection into the rest (one cell: from above / the left); formula references follow, `$B$1` stays |
| drag the handle **▪** of the selection, `:autofill B10` | continue a series: numbers (1, 2 → 3, 4), dates, “Item 1”, month and weekday names; formulas as with Ctrl+D |
| Ctrl+C / Ctrl+X / Ctrl+V         | copy / cut / paste (also tab-separated from Excel and the like) |
| Ctrl+Z / Ctrl+Y                  | undo / redo                                                   |
| F7, F7 again                     | sort the rows by the current column ascending, then descending; with a selection only its rows |
| `:sort Status, Amount desc`      | sort by several columns: the first decides, the next break its ties (`-Amount` is descending too) |
| `:sortcol`, menu *Edit*          | sort the column on its own (a selection: each of its columns) |
| `:filter`, `:filter Status = open`, `:filter off` | column filter: show only the rows with the value at the cursor / that meet a condition; remove it (see *Column filters*) |
| Ctrl+F, `:find text`             | go to the next cell containing the text – in what cells show and in their formulas, on through all tables; **Ctrl+G** (`:next`) the next match, `:prev` the one before |
| `:replace old new`               | replace a text in the selection, else in the whole table (values and formulas, one undo step); words with spaces in quotes: `:replace "due soon" open` |
| `:hide`, `:unhide`, `:width 20`, `:freeze` | hide columns, set their width, keep the first columns in place while scrolling sideways (see *Column view*) |
| `:duplicates [Column]`           | show the rows whose value in the column occurs more than once (as a filter) |
| `:dedupe [Column …]`             | delete the rows that repeat an earlier one – in the named columns, else in all of them; the first stays |
| `:group [Column]`                | new table with one row per value of the column: the number of rows and the sums of all number columns, as formulas that follow the data |
| F3 or Ctrl+E                     | open a file                                                   |
| F4                               | go to a cell (`B5`)                                           |
| F5 or Ins / F6                   | insert a row below / a column to the right (asks for `Name` or `Name:Type`) |
| F8                               | delete the rows; on a column header the columns, on a footer row that row |
| Ctrl+P or click on ⊞ pivot       | pivot view of a tensor; there Tab rotates the dimensions      |
| F10 or Ctrl+Q                    | quit; with unsaved changes only on the second press          |

A **playground** (`:playground [Name]`, in the file `## Name :: playground`)
is a free sheet as in Excel: the window is filled with empty cells to the
edge, columns are called A, B, C … Input anywhere makes the table grow up to
there; only the used range is saved, empty rows and columns at the edge are
dropped. `:fill` fills other tables too (and turns it off for a playground);
this is saved in the heading as `fill` or `nofill` after the shape.
Tables are saved without padded columns: a longer value changes only its own
line in a Git diff, not the whole table. `:compact off` (`aligned` in the
heading) keeps a table padded instead. `:shape matrix` turns a playground into a normal table, provided that values,
commented cells and named column headers form one connected range (also
diagonally adjacent); empty edges are dropped, references follow.
`:shape playground` does the reverse. Column and row headers can be reached
up to the edge of the window; a column gets its name directly in its header.

In the **pivot view**, two axes show rows and columns, the others are filters
(clickable). Input into an empty combination adds a new row to the tensor.

When **moving** and **sorting**, references follow: A1 references in formulas
(`B2`, `B2:B4`, `B`, also `Expenses!B2` from other tables) and the targets of
layer rules (e.g. a comment on `B1`). References by column name need no
change; indexes like `Expenses[2, 3]` stay where they are. The same holds when
rows and columns are **inserted** or **deleted**: colors, comments and checks
stay with their cells. After a deletion a range shrinks, the rules of the
deleted cells go, and a formula that pointed at a deleted cell shows `#REF!`.

**Formula columns** stay uniform, as in Excel tables: if a column holds the
same formula everywhere (except for references that move along, e.g.
`=B1/sum(B)`, `=B2/sum(B)` …) or is empty, a formula typed there goes into
all rows. A value without `=` stays an exception in its cell. New rows (`+`
row, Ins, paste) get the formula of the column. Columns with deliberately
different formulas are left alone, as are playgrounds. When rows and columns
are inserted, references follow.

Table names may contain spaces; in formulas they are then quoted:
`"My costs"!Amount`, `"My costs"!B2`. Other tables are reached with `!`
(`Expenses!Amount`). When a table is **renamed** (at the table origin), the
name is updated everywhere: in formulas (`Expenses!B2`, `Expenses!Amount`,
`Expenses[…]`), in `{=…}` in the text, in charts, layouts and layers with
`@Expenses`.

### Footer rows

Below a table there can be any number of **footer rows**, e.g. a total and an
average (`:footer Total` or menu *Edit → Add footer row*). They appear in
bold below the `+` row, their name on the left in the row column. Footer
cells are written like data cells: value, text or formula with `=`. An
aggregate on its own applies to its column (`=sum`, `=avg`, `=min`, `=max`,
`=count`, `=counta`, `=median` …); anything else is a formula over the table
(`=sumif(Status; "open"; Amount)`). Del clears a cell. ← onto the name and
typing renames the footer row, F8 deletes it. Formulas and text reach the
values as `Amount[Total]` or `Expenses!Amount[Total]`
(`{=Expenses!Amount[Total]}`). They are saved as a `footer` block right after
the table, itself a small Markdown table; printing, PDF, xlsx and ods show
the footer rows below the data. A playground with footer rows no longer fills
the window (`:fill on` hides them).

### Slices and scenarios (3D)

Levels can be stacked on a table – the z axis. The table is **a**;
`:slice February` adds a **slice** (its own values, e.g. a month),
`:scenario Optimistic` a **scenario** (only differences, the rest shows
through from a, in gray). The formulas of the table apply on every level;
values typed there belong to that level only, Del gives the cell back to the
table. In a slice the row names of the first column show through. Formula
columns stay uniform in slices and scenarios too.

- **Switching:** click in the line above the layer bar (`▤ Account  a January
  b February  ◇ c Optimistic`), `:z b` / `:z February`, ↑/↓ on the table
  origin (it shows the level) or Alt+↑/↓ (in Terminal.app only with “Use
  Option as Meta key”). **Esc** leads back to a. Every window has its own
  level – in the split window two months can be compared.
- **Formulas:** `B2b`, `B2.February`, `Amount.February`, `B2[-1]` (the level
  before, e.g. a running total `=C2[-1] + B2`), stacks `=sum(B2a:c)`, ranges
  `B2:4c`, `offset(B2; 0; 0; 1)`. Other tables are read at their level a,
  unless the reference names one (`Plan!B2.Optimistic`). The inspector shows
  the stack of a cell.
- `:z rename Name` renames the level shown (also a), `:z delete` deletes it.
  **`-` / `+` on the table origin** move it in the stack, like `-` / `+` on a
  column header move a column (a stays first); references by letter follow.
  Sorting, inserting or deleting rows and columns apply to all levels.
- **Printing:** every table is printed at the level last shown, its name is
  in the title (`Account · b February`). hardcalc remembers the level per
  table, also when switching tabs.
- **Layouts:** a table element shows one level – type `Account z: February`
  in the layout editor (in the file `table Account   z: February`). Several
  months: embed the table several times.

Details of the format: SPEC.md, section 2a.

### Types and your own functions

The tab **⚙ types** (`:types`; Esc returns to the table) lists your data types with base, unit, check and
the columns that use them; below them the built-in types. While a definition
is being edited, the bottom of the screen shows a short syntax help (base
types, `unit`, `range`, `check` …).

The type of a column is set in its header (`Name:Type`) or with `:type Type`
(menu *Edit → Column type…*; `:type none` removes it); values that do not
fit are reported.

Formulas know the usual Excel and text functions (`sumif`, `xlookup`, `left`,
`textjoin` …) and `@Column` or `[@Column]` for the value of the same row – as
in Excel tables.

The tab **ƒ functions** lists your own functions like `gross(x) = x * 1.19`
(similar to `LAMBDA` in Excel), usable in any formula – `=gross(B2)`,
`=sum(gross(Amount))`; “Used in” shows where they are called. Below them are
the built-in functions.

Everything can be edited directly in both tabs: the `+` row adds a new
entry, **F8** deletes.

### Calculation sheets (checks with units and the way to the result)

Besides the grid, a document can hold traceable engineering calculations –
in ```` ```hardcalc ```` blocks, with units, named variables and the way to
the result. Values from the tables are read through cell references:

````markdown
```hardcalc
## Bending check
f_yk    = 235 N/mm²          ' yield strength
gamma_M = 1.1
M_Ed    = 30.9 kNm
W_y     = Profiles!B2        ' value from the table
σ_Ed    = M_Ed / W_y -> N/mm²
f_yd    = f_yk / γ_M
σ_Ed <= f_yd
```
````

gives (tab **∑ calc**, `hardcalc calc`, printing/PDF):

```
W_y = Profiles!B2 = 324 cm³
σ_Ed = M_Ed/W_y = 30.9/324 = 95.4 N/mm²
f_yd = f_yk/γ_M = 235/1.1 = 214 N/mm²
σ_Ed = 95.4 N/mm² ≤ f_yd = 214 N/mm²   η = 0.45 ✓
```

Unit errors (`kN + m`) are detected, `kN` and `N` are converted. The tab
**∑ calc** (`:calc`, menu *View*) shows four columns: **Name**, **Input**,
the way to the result (**Calculation**) and **Comment**. A name labels a
line – `[Bending] σ_Ed <= f_yd` in the file – and stands before it when
printed and in the report; the comment is the `' …` at the end of the line.
Del in Name or Comment clears it, elsewhere it deletes the line. ASCII input
is rewritten (`gamma_M` → γ_M, `*` → ·, `<=` → ≤).

More in a sheet:

````markdown
```hardcalc
x = {0, 0.5,,, 5} m                  ' a range: 11 values
[Eq. 1] M = q·x·(L - x)/2            ' calculated for each x
M ≤ M_Rd                             ' max η = 0.78 at x = 2.50 m ✓
V = diff(q·t·(L - t)/2; t; x)        ' derivative; int(f; t; a; b) integrates
M_ss(q; L) = q·L^2/8                 ' an own function
[Moments] plot(M)                    ' a chart: ▥ Moments, layouts, print, report
```
````

**F6** (`:library`) opens the formula library – beams, cross sections,
stresses, buckling, geometry, and your own formulas in `formulas.hcmd` in the
hardcalc configuration folder; Enter inserts the formula with a line for each
input still missing. **F7** (`:plot`) edits the chart on the cursor line
(name, curves, x, type, title) or starts one for the variable there. All of it in one example:
[examples/calc-features.hcmd](examples/calc-features.hcmd). Example: [examples/beam.hcmd](examples/beam.hcmd), syntax in
[SPEC.md](SPEC.md) section 10.

In the right window, **∑ preview** (`:preview`) shows the sheet typeset –
fractions, roots and indexes drawn over several lines (term-maths); `f` or
`:formulas 2d|text|ascii` chooses the display.

**Calculation report:** `hardcalc report <file>` or `:report` writes a PDF
with typeset formulas through Typst (`<name>.report.pdf`; with the extension
`.typ` the Typst source). hardcalc brings the Typst compiler along when built
with `cargo install hardcalc --features typst`; otherwise the program
`typst` is used if installed.

### Formatting

As in Excel: **Ctrl+B** toggles bold for the selection, the context menu
(`/`, right-click) has the group **Format** with *Bold, Italic, Underline,
Text color…, Fill color…, Plain*, as does the menu *Edit*. As commands:
`:bold`, `:italic`, `:underline`, `:fg <color>`, `:bg <color>` (names like
`red`, `lightblue` or `#rrggbb`; `none` removes the color), `:wrap [on|off|width]` (long text of the
selection breaks into several lines; width in characters, default 40) and `:plain`. If
the terminal intercepts Ctrl+B (e.g. tmux), `:bold` remains.

**By condition:** `:when Amount > 1000 fg red bold` formats the cells of a
column that meet a condition (`:when Status = open bg yellow`;
`:when Amount > 1000 off` removes the rule). In the file this is the rule
`Amount[> 1000]: fg red bold`, with the conditions of the filter
(`=`, `<>`, `<`, `>`, `in(a, b)`, `contains(text)`, `empty`, `required`). It
colours the cell alone; `row[Status=open]: fg yellow` colours the whole row.

This is saved as a rule in the table's `format` layer – readable and with a
clean diff: a whole column becomes `B: bold`, a row `3: italic`, the whole
table `*`, otherwise the cell or the range (`B2:D4: fg red`). An exception in
a formatted column becomes `B2: nobold`; when the column is formatted again
later, such exceptions disappear, as in Excel.

### Layers

The layer bar above the status line shows all layers of the file:

- **Click** shows or hides a layer. The switch **⊕ all tables / ⊙ this
  table** in front of it decides whether this applies to all tables or only
  to the current one (a table's own settings are marked with `*`).
- **Right-click** opens the **layer view**: the cells show the layer instead
  of the data – for `formulas` the formulas and the type of data cells, for
  `format` the effective settings, for `validate` the rules, for `comments`
  the comments; the rest of the data fades. **Typing there edits the rule**
  of the cell or the selected range, Del removes it. Another right-click or
  Esc returns.
- **New layer:** `:layer new format` (or menu *Insert → Layer…*) adds the
  layer for the current table and opens its layer view right away. `format`,
  `validate`, `filter`, `comments` and `links` have an effect; other names are
  saved and shown.
- The built-in layer **formulas** highlights formula cells (cyan, italic).
- Validation failures are shown in red and counted, commented cells are
  underlined (the comment is shown in the status line).

### Column filters

A filter shows only the rows that meet a condition per column. It is a layer
of its own, `filter`: the file keeps **every row and only the rule**, hiding
is done when the table is shown or printed – a filter changes one line in a
Git diff.

````markdown
```layer filter
Status: in(open, paid)
Amount: >= 100
```
````

| Input                              | Effect                                              |
|------------------------------------|-----------------------------------------------------|
| click on **▾** in the header of the cursor column (**▼** when filtered), Space on a column header, `:values [Column]`, `/ C L` | the **value list**: every value of the column with a tick and its number of rows – Space or a click ticks, typing searches, Enter applies, Esc leaves |
| `:filter` (also `/ C V`, menu *Edit*) | only the rows with the value at the cursor       |
| `:filter Status = open`, `:filter Amount >= 100` | a condition for a column: `=`, `<>`, `<`, `<=`, `>`, `>=`, `in(a, b)`, `contains(text)`, `empty`, `required` (not empty) |
| `:filter Status`                   | that column, by its value in the cursor row         |
| `:filter off`, `:filter off Status` | remove all filters of the table / that of a column |
| click on **■ filter** in the layer bar | show all rows for now, and filter again (not saved) |
| right-click on **filter** (layer view) | all rows with the rules; typing in a column edits its rule, Del removes it |

In the value list, `(all)` ticks or unticks everything listed. With a text
typed, Enter keeps just the values listed (type `pa`, Enter: only *paid*).
The ticks become the rule of the column: `= open`, `in(open, paid)`, `empty`,
`required` – or none when everything is ticked.

Rules for several columns narrow each other; a new rule for a column replaces
its old one. A filtered column carries **▼** in its header, the layer bar
counts the rows shown (`■ filter 2/6`), and row numbers keep their gaps.

- Arrows, PgUp/PgDn and the mouse wheel step over hidden rows. Clearing,
  copying, cutting, pasting and deleting rows leave hidden rows alone; a
  selection across hidden rows applies to the shown ones. Filling (Ctrl+D,
  the handle) fills the shown rows – a series runs on over them, formulas
  move their references by the real distance. `:replace` leaves hidden rows
  alone too; `:dedupe`, `:duplicates` and `:group` look at all rows. Moving
  rows asks for all rows to be shown first.
- The row with the cursor is always shown: a new row can be typed in full
  before the filter hides it, and F4 or a link reaches a hidden row.
- Formulas and footer rows calculate over **all** rows (`sum(B)` does not
  change with the filter). For the rows shown there is `subtotal`: a footer
  cell `=subtotal(sum)` (or `avg`, `count`, `min`, `max` …), in formulas
  `subtotal(sum; Amount)` or `subtotal(avg; Journal!Amount)`.
- The print preview, PDF and grid layouts show the filtered rows (`rows:`
  picks from them); with the layer switched off, all of them. A layout
  element can narrow further on its own: `where: Status = open`.
- In slices and scenarios the filter judges the values of the level shown.
- Exports to xlsx and ods contain all rows and the filter as the autofilter
  of the sheet, with the same rows hidden – Excel and LibreOffice show the
  filter buttons and can change or clear it. csv and tsv hold all rows.

### Column view

Which columns are shown and how is a layer too, `view` – one rule per column,
nothing in the data changes:

````markdown
```layer view
Item: freeze width 24
Note: hide
```
````

| Input                         | Effect                                                    |
|-------------------------------|-----------------------------------------------------------|
| `:hide`, `:hide Note Remark`  | hide the columns of the selection / the named ones (also `/ C H`, menu *View*) |
| `:unhide`, `:unhide Note`     | show all hidden columns / the named ones again            |
| `:width 20`, `:width auto`    | width of the selected columns in characters / as the content needs |
| `:freeze`, `:freeze Item`, `:freeze off` | the columns up to the cursor column / the named one stay in place while scrolling sideways (a line marks them) |
| click on **■ view** in the layer bar | show everything as it is for now, and back         |

The cursor steps over hidden columns; the layer bar counts them (`■ view −2`).
Hidden columns stay in the file, in formulas and in selections across them
(copying or clearing a range takes them along). They are left out of the
print preview and the PDF, and of layout tables that do not name their
`columns:`; xlsx and ods exports mark them hidden and take the widths (xlsx).
The header row with the column names always stays in sight.

### Named views

A view is the filter and the column settings of a table under a name:

| Input                   | Effect                                                        |
|-------------------------|---------------------------------------------------------------|
| `:view save open items` | save the current filter and column view as *open items*       |
| `:view open items`      | show it again: its rules replace those of the `filter` and `view` layers (one undo step) |
| `:view`                 | list the saved views (Tab completes their names)              |
| `:view off`             | all rows and columns                                          |
| `:view delete open items` | forget a view                                               |

Views are kept in a layer `views`, one line each
(`open items: filter Status: = open | view Note: hide`). The order of the
rows is not part of a view – sorting changes the table itself.

### Split mode (as in Midnight Commander)

**Ctrl+O** splits the screen. The **left window is always the main window**
with the data; on the right there are helper views:

- **ⓘ inspector** – follows the cursor on the left: raw content, value, type
  with all its properties, a choice list for `enum`/`bool` (Enter or a click
  sets the value), which cells a formula uses and which use the value,
  format, comment, validation failures.
- **‹› source** – the Markdown source; the table on the left is highlighted.
- **▥ charts**, another table, the types.

**Tab** switches the active window, **Ctrl+U** swaps the windows (only when
both show data), **Ctrl+O** closes the split. The divider `│` can be grabbed
and moved with the mouse, between 40/60 and 60/40. The hint line at the
bottom belongs to the active window.

### Charts

Charts are blocks of their own in the file and appear in the right window
next to their data (click on `▥ Name`). Bar, line, area, scatter and
histogram; for tensors with `x`, `series` and `filter` as in the pivot view.
The chart highlights the row under the cursor, a click on a bar jumps to the
row, **t** changes the type. `:chart [type]` creates a chart from the
selected columns. `=spark(B1:B12)` draws a sparkline into a cell.
In terminals with a graphics protocol (Kitty, Ghostty, iTerm2, WezTerm) line,
area and scatter charts are drawn as images – in the chart window and in
layouts – with their labels as text; elsewhere, and while a menu or dialog is
open, in braille. Bar charts stay text. `HARDCALC_GRAPHICS=none` keeps
everything text.

### Grid layouts

`:layout [Name]` creates a report (heading, tables, charts next to them) and
opens the **layout editor** (tab `▤ Name`): pages with a ruler, line numbers
and visible **page breaks**.

| Input                         | Effect                                                   |
|-------------------------------|----------------------------------------------------------|
| arrows, PgUp / PgDn           | jump to the next element                                 |
| typing, Enter, double-click   | edit the content of the element                          |
| **F3 Move**                   | pick up the element (green) – then:                      |
| ↳ arrows                      | move                                                     |
| ↳ **+** / **−**               | taller / shorter; what lies below in the grid moves along |
| ↳ **>** / **<**               | wider / narrower                                         |
| ↳ **=**                       | as tall as the content needs                             |
| ↳ **a**                       | automatic height on / off (`height: auto`)               |
| ↳ Enter / Esc / F3            | put down                                                 |
| mouse                         | drag, handle ◢ for the size, a rectangle on free paper = new element |
| F5 / F6 / F7 / F8             | duplicate / change kind / choose paper / delete (printing: menu *File → Print preview*) |
| **Space**, `:overview`        | overview: every element as a labelled box, a whole page per screen |

Element kinds: heading, text (with live values), value with label, note,
table, chart. Elements that reach over a page break are marked in red.

A table element takes options after the table name, typed in the editor as
in the file:

| Option                     | Effect                                                    |
|----------------------------|-----------------------------------------------------------|
| `columns: Item, Amount`    | only these columns, in this order                         |
| `z: February`              | one slice or scenario                                     |
| `rows: 10`                 | the first 10 rows                                         |
| `rows: last 12`            | the last 12 rows                                          |
| `rows: top 5 by Amount`    | the 5 rows with the largest amount (`bottom`: smallest)   |
| `rows: total`              | only the footer rows                                      |
| `where: Status = open`     | only the rows that meet the condition (with `rows:` – the excerpt is taken from them) |
| `height: auto`             | as tall as the data; what lies below moves along          |

Footer rows (totals) are always shown and always sum the whole table. A
table that does not fit its element says `… 24 more rows` (marked in the
editor) and keeps its footer rows. With `height: auto` the element grows and
shrinks with the data, and a long table runs on to the next pages with its
header repeated; an element pushed over a page end starts on the next page.
The file keeps the rows as designed – only when you move or resize something
in the editor are the rows written down as they are shown. Headings, texts
and notes take `height: auto` too. `:layout` creates its tables this way.

The **overview** (Space) helps where the page is larger than the window:
several rows share a line, every element is a box with its name and what it
shows (`▦ Expenses · rows: top 5 by Amount · auto`, `✂ 24 rows hidden`).
Selecting, moving, resizing and the mouse work as on the page. In terminals that send Alt+arrows (iTerm2, Ghostty, Kitty,
WezTerm), these move elements without F3 too.

### Printing and PDF

*File → Print preview* (F9) or `:print` opens the print preview. It shows the
page character by character as it is in the PDF, with its own function key
bar:

| Key     | Effect                                                    |
|---------|-----------------------------------------------------------|
| F2      | write the PDF (`name.pdf` next to the file)               |
| F3      | open the PDF in the viewer                                |
| F4      | print (through `lp`, macOS/Linux)                         |
| F5      | portrait / landscape                                      |
| F6      | choose paper: A4, A3, Letter, Legal, Tabloid/Ledger       |
| F7 / F8 | smaller / larger font                                     |
| F9      | scope: current table · whole document · layouts           |
| i       | image preview on/off                                      |
| + / −   | zoom: several pages side by side (2, 4, 6, 12, 24), 0 or i back to one page |
| F10/Esc | close                                                     |

The scope **whole document** prints the file as a flow layout: headings,
text with live values `{=…}`, tables, charts, comments as footnotes. What is
visible is printed; long tables repeat their header, tables that are too
wide are scaled down or split into column blocks.

The **image preview** (i) shows the real PDF page – sharp in terminals with
a graphics protocol (Kitty, Ghostty, iTerm2, WezTerm), otherwise (e.g. macOS
Terminal.app) as a page overview in block graphics.
`HARDCALC_GRAPHICS=kitty|iterm|none` forces or prevents the image protocol.

### Compressed files

hardcalc writes plain Markdown by default. Large files or archives can be
saved compressed: `.hcmd.gz`, `.hcmd.zst`, `.hcmd.xz`. The compression is
recognised by the name and by the first bytes – misnamed files open too; it
is kept when saving. `:compress zstd|gzip|xz` saves under the matching name
(menu *File → Save compressed*), `:compress none` as `.hcmd` again; for
“save as” (`:w name.hcmd.gz`) the extension decides. A `.zip` with exactly
one `.hcmd` or `.md` file is read; it is saved next to it as `.hcmd`. The
content stays an ordinary Markdown document.

### TextBundle and TextPack

hardcalc saves documents with assets (images, sketches) as a
[TextBundle](https://textbundle.org) – a `.textbundle` folder – or as a
TextPack (`.textpack`, the same as ZIP) for passing on. Its own subformat is
called `.hcbundle` / `.hcpack` (technically a valid TextBundle/TextPack).
`:bundle hcbundle|hcpack|textbundle|textpack` saves that way (menu *File →
Save as bundle…*), as does `:w name.textbundle`.

- `text.md` is the complete hardcalc document, `info.json` holds the
  version, the creator and a block `org.hardcalc`.
- Every named table is also stored as TSV in `assets/raster/` (raw cells,
  formulas as typed) – good for Git, diffs and other tools. If a TSV was
  changed outside hardcalc, hardcalc takes over its rows when opening.
- Other files in `assets/` are kept when saving.
- Bundles of other programs (Bear, Ulysses, iA Writer …) open normally, but
  changes are saved as a new file `Name (hardcalc).textbundle` – unless the
  bundle is `transient`.
- `hardcalc export file.hcmd handover.textpack` writes a TextPack with
  `transient: true` (for exchange only).

### Import and export

```bash
hardcalc import book.xlsx              # → book.hcmd (xlsx, xlsm, xlsb, xls, ods, csv, tsv)
hardcalc export budget.hcmd out.xlsx   # or out.ods, out.csv, out.tsv
```

In the interface: `:import file` adds the sheets to the open document,
`:export file` writes.

- **Import:** every sheet becomes a table, its first row the header. Formulas
  with SUM, AVERAGE, MIN, MAX, COUNT, IF, ROUND, ABS, SQRT including
  references to other sheets are translated; all others come in as the last
  calculated **value, with the original formula as a comment**. CSV:
  separators are detected, Windows-1252 files are read too.
- **Export xlsx/ods:** formulas are translated (also `Table!Column`,
  `Table!B2`, filters as `SUMIFS`, your own functions inlined) and saved with
  their result; types become number formats, `format` cell formats,
  `comments` notes, `enum` and validation rules drop-downs, charts native
  Excel charts (not yet in ods). Whatever has no equivalent comes as a value
  with the hardcalc formula as a note.
- **Export CSV:** calculated values, `;` with a decimal comma on screen, otherwise `,`;
  from the command line one file per table, from the interface the current
  one.

Not transferred are layouts and running text (export) as well as formatting,
charts, macros and pivot tables (import).

**Directly in LibreOffice Calc:** the extension in [libreoffice/](libreoffice/)
opens `.hcmd` files in Calc and saves them, without hardcalc being installed.
Text, types, layers, charts and layouts are kept unchanged when saving.
Download `hardcalc-libreoffice-<version>.oxt` from the
[latest release](https://github.com/hardcalc/hardcalc/releases/latest) and
open it in LibreOffice (*Tools → Extensions*), or let hardcalc do it:

```bash
hardcalc libreoffice install       # finds LibreOffice (unopkg) and adds the extension
hardcalc libreoffice uninstall
hardcalc libreoffice export        # only writes hardcalc-libreoffice-<version>.oxt
```

The extension is built into hardcalc. LibreOffice from the Mac App Store
cannot install extensions with Python parts; use the build from libreoffice.org.

### Decimal point or comma

Files always hold numbers with a decimal point (`1200.50`), so they read the
same everywhere. Only the screen, printing and PDF follow the locale: with a
German locale hardcalc shows `1200,50`, and a number can be typed and edited
that way – it is saved as `1200.50`. `HARDCALC_DECIMAL=comma|point`
overrides the locale. Files written with a decimal comma are read and saved
with the point.

### Light and dark terminals

hardcalc asks the terminal for its background color at start and later in
pauses and adapts its colors – an automatic light/dark switch is followed.
`:theme light|dark` (or `HARDCALC_THEME=light|dark`) fixes the scheme and
then colors the whole screen itself; `:theme auto` follows the terminal
again.

Rows and columns are set off subtly in gray (odd rows and columns a bit
lighter, or a bit darker in the light scheme, crossings a bit more).
`:bands` turns this off and on again.

### Animations

hardcalc switches on and off like an old tube screen: a line of light
opens into the picture, and on quitting collapses back into a line and a
dot. Menus and dialogs slide in from above, and switching tables turns the
window like a cube.

While a formula is edited, the cells and ranges it refers to are
highlighted, each in its own colour, and dots flow from them to the
formula cell – from sources out of sight they come in at the edge on
their side, where an arrow points to them. Tabs of other tables the
formula takes data from (`Sales!B2`, `Sales[North, Value]`) take the
colour too. `:animations off` (short `:anim`) switches them off, `:animations on`
back on; `HARDCALC_ANIMATIONS=0` starts without them.

In large windows animations draw fewer frames per second, so the terminal
keeps up. When frames still take too long (a slow machine or terminal), the
animations switch themselves off and the status line says so; switched on
again with `:anim on`, they stay on. A debug build is too slow for smooth
animations; use `cargo run --release`.

### Notes on macOS Terminal.app

Out of the box, Terminal.app does not pass some key combinations on to
programs: ⌥↑/⌥↓ and Shift+↑/↓ arrive as plain arrows, PgUp/PgDn scroll the
terminal itself. hardcalc always offers a way without these keys (F3 Move
with + − > < in the layout editor, `-` / `+` on column and row headers, the
mouse). `:keys` shows in the status line what hardcalc receives from a key.
To use Alt+arrows: *Settings → Profiles → Keyboard*, ⌥↑ → `\033[1;3A`,
⌥↓ → `\033[1;3B`, or “Use Option as Meta key”. Likewise for Shift:
⇧↑ → `\033[1;2A`, ⇧↓ → `\033[1;2B`. iTerm2, Ghostty, Kitty and WezTerm send
these keys out of the box.

### Context menu and key bindings

`/` opens the context menu at the cursor cell (as in Lotus 1-2-3), as do a
right-click or Ctrl+click at the click position, the menu key and Shift+F10 –
where the terminal delivers them. Every entry has a marked letter, and
sequences serve as shortcuts: `/ R I` inserts a row, `/ C S` sorts. Inside a
formula `/` stays division; `'/` at the start types a slash.

All keys, mouse actions and menu sequences are defined in one place
(`src/tui/keymap.rs`). From there come the labels in the menu, the help page
**Keys** (F1) and the complete matrix [docs/keys.md](docs/keys.md)
(`hardcalc --keymap-markdown`). Only what demonstrably works in the running
terminal is shown: on macOS the menu key and Shift+F10 are missing, without
the kitty keyboard protocol the menu key, and GNOME Terminal intercepts
Shift+F10. What arrives counts as confirmed and is remembered per terminal;
`:keytest` (or `t` on the help page Keys) checks the menu key, Shift+F10 and
right-click one after the other.

### Commands (`:`)

For power users; the help (F1) lists them all.

| Command | Effect |
|---|---|
| `:w [file]`, `:q`, `:q!`, `:wq` | save / quit |
| `:B5` | go to a cell |
| `:sort [column] [desc]` | sort (without arguments: the current column, again: descending) |
| `:filter [Column [condition]]`, `:filter off [Column]` | column filter: only rows that meet the condition (without one: the value at the cursor); `off` removes it |
| `:values [Column]` | the values of a column as a list to tick (the filter of the column) |
| `:find text`, `:next`, `:prev`, `:replace old new` | find (Ctrl+F, Ctrl+G) and replace |
| `:hide [Column …]`, `:unhide [Column …]`, `:width n\|auto`, `:freeze [Column\|off]` | column view: hide, width, frozen columns |
| `:view [Name]`, `:view save Name`, `:view delete Name`, `:view off` | named views of filter and column settings |
| `:when Column condition format` | format the cells of a column by a condition (`off` removes it) |
| `:duplicates [Column]`, `:dedupe [Column …]` | show values that occur more than once · delete repeated rows |
| `:group [Column]` | table with counts and sums per value of a column |
| `:addrow`, `:delrow`, `:addcol Name[:Type]`, `:delcol` | rows / columns |
| `:footer [Name]` | add a footer row below the table (a summary per column) |
| menu *File → Open…*, Ctrl+E, `:e`, `:append` (without a file) | file dialog (with unsaved changes hardcalc asks first: save / discard / cancel): browse folders, typing searches, Enter opens or appends, ← one level up, `~` home, F4 / Ctrl+A steps through the files shown: all, with only the documents hardcalc reads offered (the others grey, the default) → all offered → only the documents, Ctrl+N / Ctrl+D sort by name / date (again: reverse); a sidebar like the Finder’s lists favourites (Desktop, Documents, Downloads, home) and locations (disks and volumes, iCloud Drive, cloud storage such as OneDrive or Synology Drive) – click one, or Tab into it and press Enter; the sidebar starts with its own section **Recent** (the last 20 files opened or saved), also Ctrl+R in the dialog, menu *File → Recent files…* or `:recent` |
| `:e file`, `:e! file` | open another file (xlsx/ods/csv are imported); with unsaved changes it asks, `!` discards them without asking |
| `:append file` | append the tables, layers, charts, layouts, types, functions and text of another file; table names already in use get a number |
| `:new`, `:new!` | new document with a playground |
| `:table [Name]` | new table |
| `:playground [Name]` | free sheet as in Excel, fills the window with empty cells |
| `:shape [matrix\|playground]` | playground ↔ normal table (matrix); to a matrix only for one connected range |
| `:fill [on\|off]` | fill the current table with empty cells up to the edge of the window (saved with the file) |
| `:pivot [row axis] [column axis]` | pivot view |
| `:chart [type]`, `:layout [Name]` | create a chart / grid layout |
| `:layer <name>`, `:layer new <name>`, `:layers all\|table`, `:solo [layer]` | show/hide a layer, new layer, scope, layer view |
| `:slice Name`, `:scenario Name`, `:z [b\|Name]`, `:z rename Name`, `:z delete` | add, switch, rename and delete slices and scenarios |
| `:functions` | tab with the functions |
| `:print`, `:pdf [file]` | print preview / PDF |
| `:recent` | the recently opened files |
| `:` (nothing after it) | run the last command again |
| `:import file`, `:export file` | import and export |
| `:theme auto\|light\|dark`, `:keys` | color scheme, key diagnostics |
| `:bands [on\|off]` | set off rows and columns in gray (default: on) |
| `:animations [on\|off]`, `:anim` | sliding menus, turning tables, flowing formula sources (default: on) |
| `:undo`, `:redo`, `:help` | undo, redo, help |

## hardconf – editing configuration files

The project includes a second program: **hardconf**, an editor for TOML
configuration files. On the left is the tree of the configuration, on the
right a form and the documentation of the field under the cursor.

If the file ends with a `#%` comment block holding its schema (types,
defaults, hints, documentation), hardconf checks every input against it,
offers choice lists, masks passwords and adds entries only where the schema
allows them. The file stays ordinary TOML; the program that reads it notices
nothing of the schema. hardconf opens any other TOML file as a generic tree
editor. The title bar shows which case applies (“✓ hardconf v1” or “plain
TOML”).

```bash
hardconf examples/backup.toml      # with a schema
hardconf examples/general.toml     # plain TOML
hardconf                           # choose a file in the file dialog
EDITOR=hardconf backupd config     # as the editor of another program
```

Only changed values are saved; formatting and comments are kept. Quitting
without saving leaves the file untouched. The format is described in
[CONFIG-SPEC.md](CONFIG-SPEC.md).

## Command line

```bash
hardcalc                               # interface with a new document (playground)
hardcalc <file>                        # interface (.hcmd, or import of xlsx/ods/csv …)
hardcalc --version                     # version
hardcalc desktop install|uninstall     # start menu entry and .hcmd file association
hardcalc check <file> [--json]         # overview, type, formula, chart and validation errors (file:line:)
hardcalc fmt <file>                    # print the file in canonical form
hardcalc eval <file>                   # print the file with calculated values instead of formulas
hardcalc eval <file> -e <expr> [--raw] [--json]  # one result per -e; exit code 1 on an error value
hardcalc calc <file> [--ascii]         # print the calculation sheet with the way to the result
hardcalc report <file> [out.pdf|.typ]  # calculation report with typeset formulas (Typst)
hardcalc pdf <file> [Layout]           # the whole document or one layout as PDF
hardcalc import <file> [out.hcmd]      # convert a spreadsheet to hardcalc
hardcalc export <file> <out>           # to .xlsx, .ods, .csv or .tsv
```

## Status

hardcalc is young: the format, types and formulas, the interface with split
view and inspector, charts, flow and grid layouts, printing and PDF, import
and export, layers, calculation sheets and the configuration editor hardconf
work and are tested. Open points and ideas are listed in
[docs/ROADMAP.md](docs/ROADMAP.md). Bug reports and suggestions are welcome
as [issues](https://github.com/hardcalc/hardcalc/issues); how to build, test
and send changes is described in [CONTRIBUTING.md](CONTRIBUTING.md), how to
report security problems in [SECURITY.md](SECURITY.md).

## License

MIT, see [LICENSE](LICENSE).
