# hardcalc Markdown for LibreOffice Calc

A LibreOffice extension (`.oxt`) with which Calc opens and saves hardcalc
documents – in the Open and Save As dialogs as a file type of its own,
**“hardcalc Markdown (.hcmd)”**, by double-click or drag and drop. Nothing
but LibreOffice needs to be installed: the filter is pure Python and runs
with the Python that LibreOffice brings along.

## What happens when opening

- Every table becomes a sheet with the name of the table
  (`## Expenses :: matrix` → sheet *Expenses*); anonymous tables are called
  *Table 1*, *Table 2* …
- Row 1 is the header as it is in the file (`Amount:Euro`), in bold. hardcalc
  data row 1 is therefore Calc row 2.
- Numbers become numbers (`1200.00`; also `1200,00` and `1.200,50` are read), dates become
  date values, everything else text. Columns with a text type (`text`,
  `enum`, your own types based on them) stay text, also `01069`.
  `decimal(2)` or two decimal places throughout give the number format
  `0.00`.
- **Formulas** are translated to Calc and calculated there:

  | hardcalc                    | Calc                                      |
  |-----------------------------|-------------------------------------------|
  | `=B1/sum(B)`                | `=B2/SUM($B$2:$B$4)`                      |
  | `=Amount * 1.19`            | `=B3*1.19` (column = cell of the same row) |
  | `=sum(Expenses!Gross)`      | `=SUM($Expenses.$E$2:$E$4)`               |
  | `=Expenses[Rent, Amount]`   | `=$Expenses.B2`                           |
  | `=sum(Sales[Region=North])` | `=SUMIFS(…)`                              |
  | `=gross(Amount)`            | your own function inlined: `=(B2*1.19)`   |

  What Calc cannot do (slices and scenarios, footer references, `spark`,
  recursion) stays in the cell as text `=…`, with a comment; it is written
  back unchanged when saving.
- The **`filter` layer** becomes the autofilter of the sheet: Calc hides the
  rows and shows the filter buttons (comparisons, `in(…)`, `contains(…)`,
  `empty`, `required`). Columns hidden by the **`view` layer** are hidden in
  Calc. Both layers stay in the file as they are when saving – a filter
  changed in Calc is not written back. A rule Calc cannot take leaves the
  sheet unfiltered (see the log).
- Everything except the tables – running text, `types`, `functions`, layers,
  footer rows, slices, charts, layouts, calculation sheets – is not visible in
  Calc but is carried along in the document (custom document property
  `hardcalc`, kept in an `.ods` saved in between too).

## What happens when saving

- The file is put together again exactly as it was; only tables that have
  changed are written anew (in hardcalc's canonical form). Unchanged tables
  stay byte for byte the same – diffs in Git show only real changes.
- Unchanged values and formulas keep their notation (`1200.00`,
  `=Amount * 1.19`); numbers written with a decimal comma are written with
  the point. New or changed formulas are translated back:
  `=B5/SUM($B$2:$B$5)` → `=B4/sum(B)`; a range over the whole column (also
  open-ended below) becomes the column again.
- Formulas without a hardcalc equivalent (e.g. `VLOOKUP`) are saved as their
  calculated value; the Calc formula is then kept in a block
  ```` ```layer comments @Table ```` at the end of the file.
- Renamed sheets rename the table, deleted sheets remove it, new sheets are
  appended as `## Name :: matrix` at the end.
- Documents that do not come from a hardcalc file (e.g. an `.xlsx`) are
  written as a new hardcalc file with one table per sheet.
- Line endings (LF/CRLF) stay as in the file; numbers are always written
  with a decimal point.

When saving, Calc asks, as for every foreign format, whether to keep the
format. In the Save dialog Calc appends `.hcmd`.

## Formatting

Bold, italic, underline, font and background color and the horizontal
alignment go both ways:

- **When opening**, the rules of the `format` layer are applied to the cells
  (`B: bold`, `3: italic`, `B2:D4: fg red` …).
- **When saving**, the formatting in Calc replaces these settings in the
  layer. They become rules as short as possible: a mostly bold column becomes
  `B: bold` (exceptions `B2: nobold`), a whole row `3: italic`, the rest cells
  and rectangles. Colors are named as in hardcalc (`red`, `lightblue` …) or
  `#rrggbb`.
- If nothing changed in the formatting, the block stays byte for byte as it
  was.
- Kept are what Calc does not know in this form: rules with a condition
  (`row[Status=open]: fg yellow` – not visible in Calc) and the number
  formats `percent(n)`/`decimals(n)`.

## Detection

`.hcmd` files always belong to the extension. Other `.md` files are
otherwise opened by LibreOffice in Writer; the extension takes them over only
if they have hardcalc content (a heading
`## Name :: matrix|vector|tensor|playground` or a block `types`,
`functions`, `layer`, `footer`, `hardcalc`, `slice`, `scenario`, `chart`,
`layout`). All other `.md` files still go to Writer.

## Troubleshooting

- **macOS, LibreOffice from the Mac App Store:** this edition cannot install
  extensions with Python components. The sandbox prevents the helper process
  that LibreOffice uses to activate them
  (`DeploymentException … NoConnectException: couldn't connect to pipe`);
  through the Extension Manager LibreOffice crashes. Remedy: use the edition
  from <https://www.libreoffice.org/download/>. Windows and the normal
  macOS/Linux packages are not affected.
- **The file opens in Writer or as text:** the Python components of the
  extension are not loaded. Usually LibreOffice was running during the
  installation – on Windows often invisibly as the Quickstarter in the
  notification area of the taskbar. Quit LibreOffice completely (including
  the Quickstarter) or restart the computer. LibreOffice recognises `.hcmd`
  by its name even without the components; then an error message should
  appear when opening instead of Writer.
- **Error message when opening or saving:** details are in
  `%TEMP%\hardcalc-libreoffice.log` (macOS/Linux: in LibreOffice's temp
  folder), even if the Python module could not be loaded at all.
- **Is the extension installed?** *Tools → Extension Manager*.

## Limits

- Font, font size, borders and conditional formatting from Calc are not
  transferred (see *Formatting*). Opening a file draws a thin solid grid around
  every table; hardcalc stores no borders, so the grid is not saved.
- Layer rules with cell references (`B1: …` in `comments`) do not move
  along when rows are inserted or moved in Calc.
- Units in formulas (`30{d}`) are lost in Calc; the number stays.
- Footer rows are not shown in Calc (they stay in the file).

## Building

```bash
python3 libreoffice/build.py          # → libreoffice/dist/hardcalc-libreoffice-<version>.oxt
```

The version is in [extension/description.xml](extension/description.xml). For
a new version, raise `<version value="…"/>` there and build again.

## Installing

Open the `.oxt` by double-click or install it in LibreOffice through *Tools →
Extension Manager → Add*, then quit LibreOffice completely (on Windows the
Quickstarter too) and start it again. Remove it in the Extension Manager as
well. A new version is simply installed over the old one.

**Double-click on `.hcmd`:** once, choose *Open with → Choose another app →
LibreOffice* in the Explorer and tick “Always use this app”; LibreOffice
recognises the file and opens it in Calc. `.md` is not affected.

Optional: `python3 libreoffice/build.py --update-url https://…/update.xml`
builds an `.oxt` that looks for updates itself and writes `dist/update.xml`;
put both on a web server.

## Structure and tests

| File | Content |
|---|---|
| `extension/hardcalc_filter.py` | UNO components: filter (`XFilter`, `XImporter`, `XExporter`) and detection (`XExtendedFilterDetection`) |
| `extension/registry/Types.xcu`, `Filters.xcu` | file type and filter |
| `extension/pythonpath/hardcalc_lo/` | conversion without UNO: Markdown, formula parser, translation, import/export |
| `tests/` | tests of the conversion |

```bash
python3 -m unittest discover -s libreoffice/tests
```

The Python modules are ports of `src/parse.rs`, `src/write.rs`,
`src/formula.rs` and the formula translation in `src/export.rs` /
`src/import.rs`; changes to the format there should be followed here.
Errors of the filter end up in `%TEMP%\hardcalc-libreoffice.log`.
