# hardcalc – file format `.hcmd` (draft v0)

A hardcalc file is **valid Markdown**. Opened in any Markdown viewer, it
shows headings, text and tables. hardcalc additionally reads types, shapes
(vector/matrix/tensor), formulas, your own functions, layers, charts and
layouts – all in fenced code blocks that an ordinary viewer shows as code.

Principle: *simple tables stay simple.* A plain Markdown table without any
annotation is already a valid hardcalc matrix.

## 1. Structure

```
Document = { Text block | Table | Types | Functions | Layer | Chart | Layout | Code block }
```

| Block | Notation | Section |
|---|---|---|
| **Text block** | any Markdown, kept unchanged (live values `{=…}`) | 7 |
| **Table** | Markdown table, optionally with a heading `## Name :: Shape` | 2 |
| **Footer rows** | ```` ```footer [Table] ```` right after the table | 2 |
| **Slices, scenarios** | ```` ```slice Name ````, ```` ```scenario Name ```` after the table | 2a |
| **Types** | ```` ```types ```` | 3 |
| **Functions** | ```` ```functions ```` | 4 |
| **Layer** | ```` ```layer <name> [@<table>] ```` | 6 |
| **Chart** | ```` ```chart <Name> ```` | 5 |
| **Layout** | ```` ```layout <Name> ```` | 8 |
| **Calculation sheet** | ```` ```hardcalc ```` | 10 |
| **Code block** | any other fenced block, kept unchanged and printed | – |

## 2. Tables and shapes

A heading of the form

```
## <Name> :: <Shape>
```

directly (possibly after blank lines) before a Markdown table names the table
and sets its shape.

| Shape                        | Meaning                                            |
|------------------------------|----------------------------------------------------|
| `vector`                     | one data column (plus an optional label column)    |
| `matrix` / `matrix[R, C]`    | 2D; `R`, `C` = a number, `*` (any) or an axis name |
| `tensor[A1, A2, …, An]`      | n-dimensional, named axes                          |
| `playground`                 | a free sheet as in Excel: columns A, B, C …, only the used range is saved (empty edges are dropped) |

The shape may be followed by `fill` or `nofill` (`## Lenses :: matrix fill`,
`## Cameras :: playground nofill`, before a level name:
`## Account :: matrix fill [January]`). This decides whether the view fills
the table with empty cells up to the edge of the window. Without it,
playgrounds without footer rows fill, other tables do not.

Tables without a heading are anonymous `matrix[*, *]`.

### Tensors in “long format”

A tensor is stored readably as a table in which every axis is a key column
and the last column holds the value:

```markdown
## Sales :: tensor[Month, Region, Product]

| Month | Region | Product | Value:Euro |
|-------|--------|---------|------------|
| Jan   | North  | A       | 120.00     |
| Jan   | South  | A       |  80.00     |
```

The terminal interface shows one 2D plane at a time; the other axes are
chosen as filters.

### Column headers

```
Name[:Type]
```

`Amount:Euro` means: column *Amount*, type *Euro*. Without a type, the type
is inferred from the values (`auto`).

### Cells

- A cell that starts with `=` is a **formula**.
- `\|` stands for a literal `|`.
- An empty cell has no value (`null`).
- Numbers are written with a decimal point and without thousands
  separators (`1200.50`). A decimal comma (`1200,50`, `1.200,50`) is read
  too and written with the point. How numbers are shown – point or comma –
  follows the locale of the user, not the file.

### Footer rows

Summary rows below a table (like the total row of Excel tables, but any
number of them) are kept in a `footer` block right after the table. The name
after it is only for readers; the block belongs to the table before it.

````markdown
```footer Expenses
|         | Item    | Amount                         | Status  |
|---------|---------|--------------------------------|---------|
| Total   | 3 items | =sum                           | =counta |
| Average |         | =avg                           |         |
| Open    |         | =sumif(Status, "open", Amount) |         |
| Budget  | planned | 1500.00                        |         |
```
````

The block is a Markdown table: the first column holds the names of the
footer rows, the others are columns of the table (by name or letter, in any
order, missing ones stay empty). Cells are written as in the data area:
value, text or formula with `=`. Values that do not fit the column type count
as text. An aggregate on its own (`=sum`, `=avg`, `=min`, `=max`, `=count`,
`=counta`, `=countblank`, `=median`, `=stdev`, `=product`; also the German
names `=summe`, `=mittelwert`, `=anzahl`) applies to its own column. A
formula must give a single value – there is no `@Column` here. Amounts in the
unit of the column are shown like the column; counts and other units as they
are.

Formulas and text reach footer rows like rows, through `Column[Name]`:
`Amount[Total]` in the same table, `Expenses!Amount[Total]` from elsewhere
(`{=Expenses!Amount[Total]}`). Names with spaces are quoted
(`Amount["Ø price"]`); names contain no `:`, `.` or `"`. A `footer` block
that does not directly follow a table or names an unknown column is kept
unchanged and reported by `hardcalc check`. hardcalc writes the block with
all columns of the table in their order.

## 2a. Slices and scenarios (z axis)

**Slices** (their own values, e.g. months) and **scenarios** (only
differences) can be stacked on a table, which makes it three-dimensional.
The table itself is **a** on the z axis, the blocks after it are b, c, …
Every level has its letter and a name. (Layers like `format` are something
else: presentation, section 6.)

````markdown
## Account :: matrix [January]

| Item   | Inflow | Balance      |
|--------|--------|--------------|
| Salary | 100    | =C1[-1] + B1 |

```slice February
| Item   | Inflow | Balance |
|--------|--------|---------|
|        | 50     |         |
```

```scenario Optimistic
| Inflow |
|--------|
| 120    |
```
````

- `[January]` after the shape names a; the blocks follow in the order b, c,
  … and belong to the last table before them.
- A block is a table with (some) columns of the table, rows in the same
  order; cells as in the data area. An **empty** cell takes the formula of
  the table – so formulas are written only once and calculate on every level
  with that level's values.
- **Slice:** empty cells without a formula stay empty – except in the first
  column: the row names show through.
  **Scenario:** empty cells show the values of a as well; a scenario holds
  only its differences.
- Formula columns stay uniform here too: a formula in a uniform (or empty)
  column of a slice applies to the whole column of that slice, new rows get
  it (values stay exceptions).
- Names must not look like references or letters (`B2`, `b`) and must not
  contain `,` `:` `.` `"` `[` `]`.

### References on the z axis

| Notation | Meaning |
|---|---|
| `B2` | cell on the same level as the formula |
| `B2b`, `$B$2$b` | cell in b (the letter only follows a row) |
| `B2.February`, `Amount.February` | cell / column in the slice or scenario with this name |
| `B2[-1]`, `Amount[+1]`, `@Amount[-1]` | one level before / after that of the formula; empty outside the stack |
| `Account!B2.February`, `Account!B2b`, `Amount[Total].February` | also in other tables and footer rows |
| `B2a:B2c`, short `B2a:c` | stack of a cell over a–c |
| `B2c:B4c`, short `B2:4c` | range in c (a letter at the end applies to both corners) |
| `B2a:D4c` | cuboid |
| `offset(B2, rows, columns[, levels])` | shifted reference, as in Excel |

After the `:` only what changes is written; missing parts come from the
start. A formula always reads **other tables** at their level a, unless the
reference names the level: `=Plan!B2` is Plan's table itself, also from
within a slice; `=Plan!B2.Optimistic` the scenario. In its own table a
formula stays on its level. When a slice or scenario is deleted, the letters
of later ones move up in formulas; they also follow when the order is changed
(a, the table itself, always stays first). A table is printed at the level
last shown, with its name in the title (`Account · b February`); in layouts
`z:` chooses the level. Charts and export show a.

## 3. Types

### Base types

`int`, `decimal(n)`, `float`, `text`, `bool`, `date`, `time`, `datetime`,
`duration`, `enum(a, b, …)`, `ref`, `unit "<symbol>"`.

### Your own types

```types
Euro    = decimal(2) unit "€"
Status  = enum(open, paid, cancelled)
Percent = decimal(1) unit "%"
IBAN    = text check iban
Address = record(street: text, zip: text, city: text)
```

Units are part of the type; the formula engine checks dimensions
(`€ + $` → error, `m / s * s` → `m`).

More modifiers (shared with hardconf, [CONFIG-SPEC.md](CONFIG-SPEC.md) §3.2):

```types
Port  = int range 1..65535
Code  = text pattern "^[A-Z]{2.4}$" length ..4
Mail  = text check email
Rate  = decimal(1) unit "%" special("-": "not recorded")
```

`range a..b` (numbers, open on one side with `a..` / `..b`), `pattern "re"`
(must match the whole text), `length a..b` (characters), `special("w":
"meaning")` (an additionally allowed value), `secret`, `multiline`. Checks:
`iban`, `email`, `host`, `ip`, `cidr`, `url`. Inside `"…"` only `\"` is an
escape; backslashes in patterns stay as they are.

Cells are read according to their type; invalid values (a wrong enum value,
an invalid IBAN, outside `range`, not a number …) are reported by
`hardcalc check`. Without a type (`auto`), hardcalc recognises: date, number,
truth value (`true/false`, `yes/no`; also `ja/nein`, `wahr/falsch`), number
with unit (`5 m/s`), otherwise text. `time`, `datetime`, `duration`, `ref`
and `record` are stored as text for now.

## 4. Formulas and functions

A cell that starts with `=` is a formula. The decimal separator in formulas
is always `.`, arguments are separated by `,` or `;`.

### References

| Notation                      | Meaning                                                |
|-------------------------------|--------------------------------------------------------|
| `B2`                          | cell (column B, 2nd data row; the header row does not count) |
| `B`, `Amount`                 | whole column as a vector (a name wins over a letter)   |
| `B2:D5`                       | range as a vector (row by row)                         |
| `Speed`                       | the row with this name in the 1st column → its last value |
| `B2:4`, `B2:D`                | short form: after the `:` only what changes (`B2:B4`, `B2:D2`) |
| `Expenses!Amount`, `Expenses!B2`, `Expenses!B2:B4`, `Expenses!B` | column, cell, range of another table (`"My sheet"!B2` for names with spaces) |
| `Amount[Rent]`, `Amount[Total]` | cell of the column in the row or footer row with this name |
| `Expenses[Rent, Amount]`      | cell by row key (1st column) and column; a key with spaces is quoted: `Cameras["Nikon Z7 II", Price]` |
| `Sales[Region=North]`         | filter; gives the last column (value column) as a vector |
| `@Amount`, `[@Amount]`        | value of the column in the same row – also in functions that otherwise take the whole column (`sum(@Amount, @Gross)`) |
| `@Expenses!Amount`, `Expenses[@Amount]` | the same row of another table |
| `$B$2`, `B$2`, `$B2`          | like `B2`; `$` anchors the column or row when filling (Ctrl+D, fill handle) |

Table names may contain letters, digits, spaces, `_` and `-`. Names that are
not simple identifiers are quoted in formulas: `"My costs"!B2`,
`"My costs"!Amount`, `"My costs"[Rent, Amount]`. In charts, layouts and
layers (`@My costs`) the name is written without quotes.

`!` is the only character for tables; the `.` stands for slices and
scenarios (section 2a).

### Vectors

Operators work element by element (`=Amount * 1.19`), scalars are broadcast.
If a cell formula gives a vector as long as the table, the cell takes the
element of its own row (“implicit intersection”).

### Operators and literals

`+ - * / ^`, `&` (join text), `%` (postfix, `/100`),
`= <> < <= > >=`, numbers, `"text"`, `true`/`false`.
Number with unit: `30{d}`, `5{m/s}`, `100{€}`.

### Units

`+`, `-` and comparisons require the same units; `*` and `/` combine them
(`m/s * s` → `m`). Date − date gives days (`d`). A formula result must fit
the unit of the column; in `decimal(n)` columns it is rounded half up to `n`
places.

### Functions

The usual Excel functions, with the same names and meaning (case does not
matter); the tab **ƒ functions** shows the complete list with descriptions:

| Group | Functions |
|---|---|
| Aggregates | `sum`, `avg` (`average`, `mean`), `min`, `max`, `count`, `counta`, `countblank`, `product`, `median`, `stdev` |
| Conditions | `if`, `ifs`, `switch`, `iferror`, `and`, `or`, `not`, `sumif`, `countif`, `averageif`, `sumifs`, `countifs`, `averageifs`, `minifs`, `maxifs` |
| Information | `isblank`, `isnumber`, `istext`, `iserror` |
| Lookup | `xlookup`, `match`, `index` |
| Math | `abs`, `sqrt`, `round`, `roundup`, `rounddown`, `int`, `mod`, `power`, `sign`, `ceiling`, `floor`, `exp`, `ln`, `log`, `pi` |
| Date | `today`, `date`, `year`, `month`, `day`, `weekday`, `edate`, `eomonth`, `datedif` |
| Text | `len`, `left`, `right`, `mid`, `upper`, `lower`, `proper`, `trim`, `concat` (`concatenate`), `textjoin`, `substitute`, `find`, `search`, `rept`, `exact`, `value`, `text` |
| hardcalc | `spark(values)` – sparkline as text |

Names, units and options are always English and do not depend on the
language settings (`datedif(a, b, "YM")`, `text(d, "dd.mm.yyyy")`); where
Excel and LibreOffice Calc differ, hardcalc follows Calc (`datedif` with
`"MD"` and `"YD"`, `minifs` / `maxifs` without a match give 0).

Criteria as in Excel: `">100"`, `"<>open"`, `"open"`. German names of
German Excel are understood too: `summe`, `mittelwert`, `anzahl`, `wenn`,
`verketten`, `länge`, `heute`. Aggregates skip empty cells and text; in
arithmetic an empty cell counts as 0. `if` evaluates only the chosen branch.
Text functions work on columns element by element (`=upper(Item)`).

### Your own functions

Your own functions (like `LAMBDA` in Excel) live in a `functions` block:

````markdown
```functions
gross(x) = x * 1.19                    # gross amount with 19 % VAT
share(part, total) = part / total
fac(n) = if(n <= 1, 1, n * fac(n - 1))
```
````

`name(parameter, …) = formula`, optionally with `# description`. In the body
the parameters are local names; everything else is resolved as in the
calling cell. Functions may call each other and themselves (up to a depth of
64); `if` evaluates only the chosen branch. Columns as arguments are
processed element by element, units are checked as everywhere. Names of
built-in functions are not allowed; case does not matter.

### Evaluation

Results are calculated on demand and cached. Dependencies are recorded while
evaluating; when a cell changes, only its (transitive) dependents are
recalculated. Circular references give an error value.

## 5. Charts

A chart is a fenced block `chart <Name>` with `key: value` lines. It refers
to table data and is therefore always up to date.

````markdown
```chart Spending
type:   bar
data:   Expenses.Amount, Expenses.Gross
labels: Expenses.Item
```

```chart SalesTrend
type:   line
data:   Sales           # tensor
x:      Month
series: Region
filter: Product=A
```
````

| Key       | Meaning                                                          |
|-----------|------------------------------------------------------------------|
| `type`    | `bar`, `line`, `area`, `scatter`, `histogram`                    |
| `data`    | `Table.Column[, …]` – one series per column; or a tensor         |
| `labels`  | column with the labels (column charts)                           |
| `x`       | tensor: the axis for the x direction; scatter: the column of x values |
| `series`  | tensor: the axis whose values form the series                    |
| `filter`  | tensor: `Axis=Value, …` for the other axes                       |
| `bins`    | histogram: number of classes                                     |
| `title`   | title (default: the name)                                        |

All values of a chart must have the same unit; it labels the axis. For
tensors, several rows with the same keys are added up (as in the pivot
view). `=spark(range)` gives a sparkline (`▁▃▅█`) as text in a cell.

## 6. Layers

A layer is a fenced block that refers to the last table before it – or with
`@Name` to any table.

````markdown
```layer format
B: align right
D: percent(1)
row[Status=open]: fg yellow
```
````

Every line has the form `<target>: <rule>`. Targets:

| Target             | Meaning                         |
|--------------------|---------------------------------|
| `B`, `Amount`      | column                          |
| `3`                | row 3                           |
| `B2`, `B2:D5`      | cell / range                    |
| `row[<condition>]` | rows that meet the condition    |
| `*`                | the whole table                 |

### `format`

Rules: `align left|right|center`, `fg <color>`, `bg <color>`, `bold`,
`italic`, `underline`, `percent(n)` (value × 100 with n places and `%`),
`decimals(n)`, `wrap` / `wrap n` (break long text into lines of at most n
characters, without n: 40; the file keeps the text as it is, only the
display, the PDF and spreadsheet exports break lines; numbers are never
broken). Several settings are combined with spaces
(`fg yellow bold`); later rules override earlier ones. Colors: names (`red`,
`lightblue`, `darkgray` …) or `#rrggbb`.

To take back what an earlier rule sets: `nobold`, `noitalic`,
`nounderline`, `nowrap`, `fg none`, `bg none` – e.g. a cell in a bold column:

```layer format
B: bold
B2: nobold
```

### `validate`

Checks: `required`, `unique`, comparisons (`>= 0`, `< 100`, `<> 0`),
`in(a, b, c)`. Empty cells are only flagged by `required`. Failures are shown
by `hardcalc check` and in the interface (red background).

Predefined layers:

| Layer       | Purpose                                            |
|-------------|----------------------------------------------------|
| `format`    | presentation (alignment, colors, number format)    |
| `validate`  | rules (`>= 0`, `required`, `unique`, `in(…)`)      |
| `comments`  | notes on cells (footnotes when printed, see 7)     |
| `formulas`  | built in: highlights formula cells (default: cyan, italic); a block of your own changes the presentation |
| `links`     | *planned:* links to URLs, files, other tables      |
| `scenario`  | *planned:* “what if” overrides of values           |

Layer names of your own are allowed; their rules are saved and shown in the
layer view but have no other effect. Which layers are shown or hidden in the
interface is a view setting and is not saved in the file.

## 7. Running text, live values and comments

The Markdown text of the file is at the same time the **flow layout** for
printing: headings, paragraphs (`**bold**`, `*italic*`), lists, tables,
charts and code blocks appear in file order. Type and layer blocks are not
printed.

**Live values:** `{=expression}` or `{=expression | format}` in the text is
replaced by the calculated value when printing (and by `hardcalc eval`).
Plain names refer to the table *above* the text; the format uses the
notation of the `format` layer:

```markdown
Rent is **{=Expenses[Rent, Share] | percent(1)}** of the expenses.
```

**Comments** live in the `comments` layer (`B1: Rent increase announced for
July`). On screen, commented cells are underlined (text in the status line
and in the inspector); when printed they get a superscript number and the
comment appears as a **footnote** below the table. A comment on a column
marks its header.

## 8. Grid layouts

Besides the flow layout, a file can hold any number of **grid layouts** –
pages on which elements are placed freely:

````markdown
```layout Report
page: A4 portrait
font: 9
grid: 12
1-12 / 1-2    heading  Budget 2026
1-6  / 4-12   table    Expenses   columns: Item, Amount, Share
7-12 / 4-12   chart    Spending
1-8  / 14-16  text     Rent is **{=Expenses[Rent, Share] | percent(1)}** of the expenses.
9-12 / 14-16  value    sum(Expenses!Gross)   label: Total expenses
1-12 / 18-19  note     All figures are provisional.
```
````

- `page:` paper (`A4`, `A3`, `Letter`, `Legal`, `Tabloid`; `Ledger` =
  Tabloid landscape) and `portrait`/`landscape`; `font:` font size in points
  – a grid row is one line of text in this size; `grid:` number of columns
  (default 12).
- Every further line places an element: `columns / rows  kind  content`.
  Columns and rows count from 1 and include the end. Rows continue across
  pages; how many fit on a page depends on paper and font. An element that
  crosses a page break is cut off there (the editor marks it in red).
- Kinds: `heading`, `text` (Markdown with live values), `value` (expression,
  optionally `label:`), `note` (shaded box), `table` (name, optionally
  `columns:` and `z:` – the slice or scenario, by name or letter; without:
  a), `chart` (name). A table element shows exactly one level; for several,
  embed the table several times:

  ```
  1-6  / 1-10   table    Account   z: January
  7-12 / 1-10   table    Account   z: February
  ```

## 9a. Files and bundles

| Form | Extension | Content |
|---|---|---|
| Markdown | `.hcmd` | default; plain `.md` is read too |
| compressed | `.hcmd.gz`, `.hcmd.zst`, `.hcmd.xz` | the same Markdown, recognised by extension and magic bytes |
| TextBundle | `.textbundle/`, `.hcbundle/` | folder: `info.json`, `text.md`, `assets/` |
| TextPack | `.textpack`, `.hcpack` | the same as ZIP |

`text.md` is the complete document and the single source of truth.
`info.json` holds `version: 2`, `type: "net.daringfireball.markdown"`,
`creatorIdentifier: "org.hardcalc"` and the block

```json
"org.hardcalc": {
  "formatVersion": 1,
  "kind": "hcbundle",
  "raster": { "Loads": { "file": "assets/raster/Loads.tsv", "checksum": "…" } }
}
```

Every named table is stored in `assets/raster/<Name>.tsv`: a header line with
the column names, then the cells as in the table (formulas with `=`). If a
TSV differs from its checksum, it was changed outside hardcalc; its rows
replace those of the table when opening (columns by name). `assets/cache/`
may be missing and is not read. Bundles with a foreign `creatorIdentifier`
are not overwritten, unless they are `transient`.

## 9. Canonical form

hardcalc writes files in a fixed, deterministic form (aligned columns,
normalised separator lines) so that diffs in Git stay readable.
`hardcalc fmt` produces this form; it is idempotent.

## 10. Calculation sheets

A ```` ```hardcalc ```` block holds calculation lines: traceable
calculations with units, readable formulas and the way to the result. All
blocks of a file together form one calculation sheet, evaluated from top to
bottom; later blocks see the variables of earlier ones. Other Markdown
editors show the block as code.

````markdown
```hardcalc
## Bending check
f_yk   = 235 N/mm²          ' yield strength
γ_M    = 1.1
L      = 5.0 m              ' span
g_k    = 4.0 kN/m
q_k    = 3.0 kN/m
q_d    = 1.35·g_k + 1.5·q_k
M_Ed   = q_d·L^2 / 8
W_y    = Profiles!B2        ' value from the table
σ_Ed   = M_Ed / W_y → N/mm² ' output in N/mm²
f_yd   = f_yk / γ_M
σ_Ed ≤ f_yd                 ' check line
```
````

| Line | Meaning |
|---|---|
| `name = expression` | assignment; names of letters, digits, `_` (`f_tk`: index tk) |
| `a ≤ b`, `a ≥ b`, `<`, `>` | check: utilisation η (`a/b` or `b/a`) and ✓/✗ |
| `expression` | display only |
| `… → unit` | show the result in this unit |
| `## Text` | heading |
| `' Text` | comment (also at the end of a line) |
| `#full`, `#formula`, `#result` | display of the following lines: full way to the result (default) / formula and result / result only; at the end of a line for that line only |

- **Numbers** with a decimal point; a decimal comma is read too and written
  with the point. **Arguments** are separated by `;`: `max(a; b)`. On
  screen, in printing and in the report numbers follow the locale.
- **Units** follow a number: `4.0 kN/m`, `235 N/mm²`, `30°`, `5 %`. Known are
  the SI units and those common in construction: mm, cm, km, t, kN, MN, kPa,
  MPa (= N/mm²), GPa, bar, kNm, kWh, kW, l, %, ‰, °, rad, min, h, d. Values
  are calculated internally in SI; `+`, `-` and comparisons require the same
  dimension (`kN + m` → error, `kN + 500 N` → `1.50 kN`).
- **Display unit:** products and quotients keep the units of the operands
  and cancel (`kN/m² · m` → `kN/m`); otherwise the preferred unit of the
  dimension applies (force kN, force/length kN/m, stress N/mm², moment kNm).
- **Cell references** in formula notation (section 4): `Profiles!B2`,
  `"My sheet"!B2`, `Loads[Wind, q]`; an unknown name is looked up in the
  tables (row name). The unit comes from the column type
  (`decimal(0) unit "cm³"`) or from the cell.
- **Functions:** `sqrt`, `abs`, `min`, `max`, `round(x; n)`, `sin`, `cos`,
  `tan` (angles in ° or rad), `asin`, `acos`, `atan` (result in °), `ln`,
  `log`, `exp`; the constant `π`.
- **ASCII input:** `gamma_M` → γ_M, `*` → ·, `<=` → ≤, `>=` → ≥, `->` → →.
  The tab **∑ calc** rewrites input this way; both are read.

**Display** (full way to the result): formula, values inserted (inputs as
written, without unit), result with 3 significant digits in the display
unit:

```
q_d = 1.35·g_k + 1.5·q_k = 1.35·4.0 + 1.5·3.0 = 9.90 kN/m
σ_Ed = 95.5 N/mm² ≤ f_yd = 214 N/mm²   η = 0.45 ✓
```

`hardcalc calc file` prints the sheet this way (`--ascii`: ASCII only,
always the decimal point), `hardcalc check` reports lines that cannot be calculated.
When printed, the block appears with the way to the result; the standard PDF
fonts know no Greek letters, so they are spelled out there (`gamma_M`, `<=`,
`ok`).

**Typesetting:** the syntax tree is the single source of truth; from it come
the one-line text (Unicode or ASCII), LaTeX (2D display in the terminal,
view ∑ preview) and Typst (calculation report, `hardcalc report`). Indexes
are lowered and set upright (`f_yk` → f<sub>yk</sub>), quotients as
fractions.
