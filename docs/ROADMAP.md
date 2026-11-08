# hardcalc – roadmap

A file holds **data** (tables, types), **rules** (layers) and
**presentations** (layouts, charts, comments). Presentations are blocks of
their own in the same Markdown file and *refer* to data instead of copying
it – when a number changes, report and chart are up to date.

This page lists open points and ideas. Suggestions are welcome as
[issues](https://github.com/hardcalc/hardcalc/issues).

## Spreadsheet

- **Decimal arithmetic** for amounts of money (today `f64` with rounding).
- **Stable references when inserting and deleting**: A1 references follow
  when moving and sorting, not yet when inserting or deleting rows and
  columns. In the long run, layouts, comments and text should refer by key
  (`Expenses[Rent, Amount]`) instead of A1.
- **Renaming with consequences** for types and functions (tables and columns
  already work).
- Layers **`links`** (URLs, files, other tables) and **`scenario`** (what if).
- Remember view settings (hidden layers, theme) in the file (`fill`/`nofill`
  already works).
- **Playground with several ranges → grid layout**: where `:shape matrix`
  refuses today because the contents are not connected.
  - Every data range becomes a matrix of its own with an index after the name
    (`Cameras 1`, `Cameras 2` …); the playground disappears.
  - Free texts become `text` elements; a single cell used as a heading
    (e.g. in the first row) becomes `heading`.
  - All formulas and layer targets follow, also across ranges
    (`Cameras 2!B3`) and from other tables.
  - The layout arranges the elements as the ranges lay in the playground.
- `hardcalc diff`: compare two versions cell by cell.

## Interface

- **Sixel** for the image preview (foot, xterm, mlterm …).
- Layout: an option to keep elements of the same row together when they move
  along.
- Clarity of the interface (beginner/expert view).
- Cursor movement and input still go directly through crossterm keys; the
  key map covers the program functions. In the long run they should become
  `Action`s too.

## Calculation reports

- **Releases with the built-in Typst compiler** (`--features typst`), so that
  `hardcalc report` works without installing `typst`. The fonts it embeds
  (Libertinus Serif: SIL OFL; New Computer Modern: GUST Font License;
  DejaVu Sans Mono) may be bundled, but their license texts must come along:
  ship the NOTICE of `typst-assets` as `THIRD-PARTY-FONTS.txt` in the release
  archives. Adds about 9 MB to the binary. Today the releases are built
  without it and use an installed `typst`.

## Import and export

- **Charts in the ODS export**.

## LibreOffice extension

- **Publish on extensions.libreoffice.org**, so that the extension can be
  found under *Tools → Extensions → Get more extensions online*. Needs a
  TDF account, a project page (description, category Calc, MIT, logo,
  screenshot) and one uploaded `.oxt` per release. Deliberately not
  installed by the Homebrew formula: a formula must not write into the
  LibreOffice user profile, and uninstalling or upgrading would leave a
  stale extension behind.
- **Update check**: done in CI (`.github/workflows/oxt.yml` attaches `.oxt` and
  `update.xml` to every release); to be seen with the first release after it.
- **Complete `description.xml`**: an icon, the license, and
  `LibreOffice-minimal-version` instead of `OpenOffice.org-minimal-version`.

## hardconf

- Renaming map entries, with updating of the `ref` references.
- `list(Struct)` as a table in the form (today one tree node per element).
- Search; `hardconf check` and `hardconf schema` without the interface.
- Show truncated hints of the selected row in full in the status line.
- Schema from a separate file (`#%include`), conditionally required fields,
  documentation per enum variant, a derive macro that generates the schema
  from Rust structs.
