# hardcalc – file format `.hcmd` (version 1)

A hardcalc file is **valid Markdown**. Opened in any Markdown viewer, it
shows headings, text and tables. hardcalc additionally reads types, shapes
(vector/matrix/tensor), formulas, your own functions, layers, charts and
layouts – all in fenced code blocks that an ordinary viewer shows as code.

Principle: *simple tables stay simple.* A plain Markdown table without any
annotation is already a valid hardcalc matrix.

## 1. Structure

```
Document = [ Version ] { Text block | Table | Types | Functions | Layer | Chart | Layout | Code block }
```

### Format version

The first line of a file names the version of this format it is written
in:

```markdown
<!-- hardcalc 1 -->
```

It is an HTML comment, so Markdown viewers do not show it. hardcalc writes
the line whenever it saves a file (`hardcalc fmt` too) and reads it only as
the first line.

- **Version 1** is this document.
- **A file without the line** is from before the versions. It is read as
  version 1, with one difference: `,` between the arguments of functions is
  accepted and written as `;` (section 4). Saving adds the line.
- **A higher version** than the program knows is read as well as it can
  be; hardcalc says so when opening, `hardcalc check` warns, and saving
  keeps the higher number.

The number rises only when files of the new version would be read wrongly
by older programs or the other way round.

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

`aligned` after the fill flag (`## Lenses :: matrix aligned`, with a layer
`## Account :: matrix fill aligned [January]`) selects padded columns instead
of the compact form, see section 9.

Tables without a heading are anonymous `matrix[*, *]`. So is a table below
a heading that says `::` but names no shape (`## std::map overview`): the
heading stays a heading, and `hardcalc check` warns. Table names should
differ in more than upper and lower case; `check` warns about names used
twice.

Tables are not looked for in code blocks (fenced with three or more
backticks or tildes; an unclosed block runs to the end of the file), in
lines indented by four spaces or a tab (unless they hang below a list
item), and in HTML comments that start a line (`<!--` … `-->`; one that is
never closed hides nothing). hardcalc writes code blocks with backticks,
with tildes only where the info line itself holds a backtick.

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
is inferred from the values (`auto`). The type follows the last `:` outside
brackets and quotes and starts with a letter – `Time 12:00` is a column name
without a type, `Rate:decimal(1) special("-": "none")` a column *Rate*.

### Cells

- A cell that starts with `=` is a **formula**.
- `\=` starts text that begins with `=` and is no formula: `\=ok` shows
  `=ok`. (Markdown reads `\=` the same way.) This holds without a type and
  in `text` columns; imports write such text this way.
- `\|` stands for a literal `|`.
- An empty cell has no value (`null`).
- Numbers are written with a decimal point and without thousands
  separators (`1200.50`). A decimal comma (`1200,50`, `1.200,50`) is read
  too and written with the point. How numbers are shown – point or comma –
  follows the locale of the user, not the file.
- A single `,` or `.` is always the decimal separator: `1,200` is 1.2, as
  `1.200` is. Separators between thousands are read only where they group
  the digits in threes (`1.200.000`, `1,234,567`, `1 200`, `1'200`).
  Anything else is text and stays as written: `1.2.3`, `192.168.1.1`,
  `030 1234567`. Without a type, digits with a leading zero (`01234`, `007`)
  are text too; a column of type `int` reads them as numbers.
- A row with more cells than the header has columns loses the cells beyond
  when saved, as in GitHub's Markdown; `hardcalc check` warns about it.

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
| Open    |         | =sumif(Status; "open"; Amount) |         |
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
- Names must not look like references (`B2`) or the letters of a level
  (one or two lower-case letters: `b`, `ab`), must not be only digits or
  `*`, and must not contain `,` `:` `.` `"` `[` `]` `` ` `` `@` `!`. Where
  a name or a letter may stand (`z:` in layouts, `:z`), the name wins.

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
| `offset(B2; rows; columns[; levels])` | shifted reference, as in Excel |

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
Code  = text pattern "^[A-Z]{2,4}$" length ..4
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
is always `.`. The arguments of a function are separated by `;` and by
nothing else: `=round(sum(B) / 3; 2)`. (A `,` there is an error; only files
without a format version may still say it, section 1.) The `,` separates
the keys in brackets: `Expenses[Rent, Amount]`.

### References

| Notation                      | Meaning                                                |
|-------------------------------|--------------------------------------------------------|
| `B2`                          | cell (column B, 2nd data row; the header row does not count) |
| `B`, `Amount`                 | whole column as a vector (a name wins over a letter)   |
| `B2:D5`                       | range as a vector (row by row)                         |
| `B:B`, `B:D`                  | whole columns as a vector (row by row, like `B2:D5`)   |
| `Speed`                       | the row with this name in the 1st column → its last value (a column of this name wins over the row) |
| `B2:4`, `B2:D`                | short form: after the `:` only what changes (`B2:B4`, `B2:D2`) |
| `Expenses!Amount`, `Expenses!B2`, `Expenses!B2:B4`, `Expenses!B` | column, cell, range of another table (`"My sheet"!B2` for names with spaces) |
| `Amount[Rent]`, `Amount[Total]` | cell of the column in the row or footer row with this name |
| `Expenses[Rent, Amount]`      | cell by row key (1st column) and column; a key with spaces is quoted: `Cameras["Nikon Z7 II", Price]` |
| `Sales[Region=North]`         | filter; gives the last column (value column) as a vector |
| `@Amount`, `[@Amount]`        | value of the column in the same row – also in functions that otherwise take the whole column (`sum(@Amount; @Gross)`) |
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
| hardcalc | `spark(values)` – sparkline as text · `subtotal(function; column)` – an aggregate over the rows the filter shows (section 6, `filter`) |

Names, units and options are always English and do not depend on the
language settings (`datedif(a; b; "YM")`, `text(d; "dd.mm.yyyy")`); where
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
share(part; total) = part / total
fac(n) = if(n <= 1; 1; n * fac(n - 1))
```
````

`name(parameter; …) = formula`, optionally with `# description`. In the body
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
data:   Expenses!Amount, Expenses!Gross
labels: Expenses!Item
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
| `data`    | `Table!Column[, …]` – one series per column; or a tensor         |
| `labels`  | column with the labels (column charts)                           |
| `x`       | tensor: the axis for the x direction; scatter: the column of x values |
| `series`  | tensor: the axis whose values form the series                    |
| `filter`  | tensor: `Axis=Value, …` for the other axes                       |
| `bins`    | histogram: number of classes                                     |
| `title`   | title (default: the name)                                        |

Columns are named as in formulas, `Table!Column`; the older form
`Table.Column` is read too. All values of a chart must have the same unit;
it labels the axis. For
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
| `Amount[<conditions>]` | the cells of the column that meet them: `Amount[> 1000]`, `Status[in(open, due)]` (conditions as in `filter`) |
| `*`                | the whole table                 |

### `format`

Rules: `align left|right|center`, `fg <color>`, `bg <color>`, `bold`,
`italic`, `underline`, `percent(n)` (value × 100 with n places and `%`),
`decimals(n)`, `wrap` / `wrap n` (break long text into lines of at most n
characters, without n: 40; the file keeps the text as it is, only the
display, the PDF and spreadsheet exports break lines; numbers are never
broken). Several settings are combined with spaces
(`fg yellow bold`); later rules override earlier ones. Colors: `#rrggbb` or
a name – `black`, `red`, `green`, `yellow`, `blue`, `magenta`, `cyan`,
`gray`, `darkgray`, `white` and `lightred` … `lightcyan` (`grey` and
`light-blue` are read too). Any other name is reported by `hardcalc check`
and has no effect. `percent(n)` keeps the unit of the value: 2 € shows as
`200.0 € %`.

To take back what an earlier rule sets: `nobold`, `noitalic`,
`nounderline`, `nowrap`, `fg none`, `bg none` – e.g. a cell in a bold column:

```layer format
B: bold
B2: nobold
```

### `validate`

Checks: `required`, `unique`, comparisons (`>= 0`, `< 100`, `<> 0`),
`in(a, b, c)`, `contains(text)` (upper and lower case alike), `empty`. Empty
cells are only flagged by `required`. Failures are shown by `hardcalc check`
and in the interface (red background).

A value with a space, comma, bracket or quote is written in double quotes,
a quote inside doubled: `= "due soon"`, `in(open, "a, b")`, `= "say ""hi"""`.

### `filter`

Column filters. Every rule names a **column** and the conditions its cells
must meet, written like the checks of `validate`:

````markdown
```layer filter
Status: in(open, paid)
Amount: >= 100
Item: contains(rent)
```
````

| Condition              | A row is shown if the cell …                          |
|------------------------|-------------------------------------------------------|
| `= x`, `<> x`, `< x`, `<= x`, `> x`, `>= x` | compares so with `x` – as numbers if both are numbers, else as text |
| `in(a, b, c)`          | equals one of the values                              |
| `contains(text)`       | contains the text (upper and lower case alike)        |
| `empty`                | is empty                                              |
| `required`             | is not empty                                          |

A row is shown when it meets **every** rule of its table; several conditions
in one rule (`>= 100 < 500`) must all hold. An empty cell meets only `empty`
and `<>`. Values are the calculated ones (a formula counts with its result);
with slices and scenarios, those of the level shown.

The file always holds all rows in their order; the layer only says which
ones a program shows. Hiding rows is the task of the program that displays
or evaluates the table: hardcalc hides them on screen, in the print preview,
the PDF and in grid layouts (`rows:` picks from the rows shown). Formulas,
footer rows, charts and live values are **not** affected – `sum(Amount)` is
the sum of all rows. Only `subtotal(function; column)` follows the filter:
`subtotal(sum; Amount)`, `subtotal(avg; Journal!Amount)` aggregate the rows
of the column that the filter of its table shows (functions: those a footer
may name alone). In a footer row, `=subtotal(sum)` applies to its own column.
The column is a whole column of the table; without a filter all rows count.
While hardcalc shows all rows (filter layer switched off), `subtotal` counts
all rows too. Exported as `SUBTOTAL(109, …)`. Whether the layer is switched on is a view setting like
that of every layer and is not saved; a file opens with its filters on.

The xlsx and ods exports pass the filter on as the autofilter of the sheet
(header and data rows, without the footer rows) and mark the hidden rows as
filtered: comparisons, `in(…)`, `contains(…)`, `empty` and `required` become
the matching conditions; what Excel cannot say in one column filter (more
than two comparisons, mixed conditions) becomes the list of the values
shown. csv and tsv hold all rows.

A rule with another target than a column (`B2`, `3`, `row[…]`, `*`), with
`unique` or with a condition that cannot be read hides nothing and is
reported by `hardcalc check`.

Predefined layers:

| Layer       | Purpose                                            |
|-------------|----------------------------------------------------|
| `format`    | presentation (alignment, colors, number format)    |
| `validate`  | rules (`>= 0`, `required`, `unique`, `in(…)`)      |
| `filter`    | column filters: rows that do not meet them are hidden |
| `view`      | how columns are shown: hidden, their width, frozen |
| `views`     | named views: saved rules of `filter` and `view`    |
| `comments`  | notes on cells (footnotes when printed, see 7)     |
| `formulas`  | built in: highlights formula cells (default: cyan, italic); a block of your own changes the presentation |
| `links`     | jumps to other places of the document (see below)  |
| `scenario`  | *planned:* “what if” overrides of values           |

A layer whose table does not exist (`@Name` unknown, or no table before
it) is kept and has no effect; `hardcalc check` reports it. It also warns
about a rule whose target meets nothing – a column the table does not
have, a row or cell beyond it (in a playground or a `fill` table, cells
beyond the data are fine).

### `view`

How the columns of a table are shown. Every rule names a column; its
settings are combined with spaces:

````markdown
```layer view
Item: freeze width 24
Note: hide
```
````

| Setting   | Effect                                                              |
|-----------|---------------------------------------------------------------------|
| `hide`    | the column is not shown                                             |
| `width n` | the column is n characters wide (longer content is cut on screen)   |
| `freeze`  | this column and those left of it stay in place while scrolling sideways |

Nothing in the data depends on it: hidden columns keep their place in
formulas, references and letters. hardcalc leaves hidden columns out on
screen, in the print preview, the PDF and in layout tables without
`columns:`; the xlsx and ods exports mark them hidden. A rule with another
target than a column or with an unknown setting does nothing and is reported
by `hardcalc check`.

### `views`

Named views of a table: every rule is a name and the rules of the `filter`
and `view` layers it stands for, separated by ` | `:

````markdown
```layer views
open items: filter Status: = open | view Note: hide
all of March: filter Date: >= 2026-03-01 < 2026-04-01
```
````

Showing a view replaces the rules of the table's `filter` and `view` layers
by these; the block itself has no effect on what is shown. A name contains no
`:`, `|`, `[` or `]`.

### `links`

Every rule makes its cells a link; the body is the destination, written like
a reference in a formula (section 4):

````markdown
```layer links
A: Details[@Area, Amount]
B1: Expenses
B2: "My costs"!B2
C5: Expenses[Rent, Amount]
```
````

| Destination               | Leads to                                            |
|---------------------------|-----------------------------------------------------|
| `Expenses`                | the first cell of the table                         |
| `B5`, `Amount`            | cell / top of the column in the own table           |
| `Expenses!B5`, `Expenses!Amount` | cell / top of the column of another table    |
| `Expenses[Rent]`          | the row with this key (1st column)                  |
| `Expenses[Rent, Amount]`  | the cell by key and column                          |
| `Expenses[@Amount]`       | the same row of the other table (as in formulas)    |
| `Expenses[@Item, Amount]` | the row whose key is the value of `Item` in the link's own row |

A cell address (`B5`, `Expenses!B5`) may lie beyond the data: in a playground
the cursor goes exactly there, in a table that does not fill as near as it can (the `+`
row or column). Destinations by key, row number or column lie within the data.
A table name wins over an address it looks like (`Sheet2`).
A destination by key stays right when rows are sorted or inserted. If
several rules meet a cell, the last one wins. `hardcalc check` reports
destinations that lead nowhere (a destination with `@` is tried from every
cell of its rule). Renaming a table renames it in the destinations.

In the interface, a cell with a link shows `↗` in its last column, in the
colours of the cell's text; the column is one wider for it, so the text (also a
right-aligned number) stays whole. A rule for a cell or range (`A5`, `A5:B7`)
also marks empty cells beyond the data, and the layer view shows it there;
rules for columns and rows only mark the cells of the table. A click on
`↗` jumps (a click elsewhere selects the cell as usual); F4 on the cell
opens the go-to prompt with the destination, Enter or a second F4 jumps. Every jump – also
F4 with a destination typed by hand – is a step in the undo history: Ctrl+Z
goes back, Ctrl+Y on again. `:back` and the `← Overview!A2` in the status
line go back from the last jump and keep the changes made since.

Layer names of your own are allowed; their rules are saved and shown in the
layer view but have no other effect. Which layers are shown or hidden in the
interface is a view setting and is not saved in the file.
The layer bar always lists the predefined layers (`formulas`, `format`,
`validate`, `filter`, `comments`, `links`), then the own ones. A layer without rules for
the shown table is grey (`○`) and cannot be switched; a right-click opens its
layer view, and the first rule typed there creates the block.

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
- Options follow the content as `key: value`, in any order; hardcalc writes
  them as `columns:`, `rows:`, `where:`, `label:`, `z:`, `height:`. In `heading`,
  `text` and `note` only a closing `height: auto` is an option; everything
  else there is text.
- `where:` shows only the rows that meet a condition on a column, written
  as in a `filter` rule: `where: Status = open`, `where: Amount >= 100`. It
  adds to the filter of the table; `rows:` takes its excerpt from the rows
  left. The title names it (`Expenses · Status = open`).
- `rows:` shows a part of a table: `rows: 10` (or `first 10`), `rows: last
  10`, `rows: top 5 by Column`, `rows: bottom 5 by Column` (ranked by the
  numbers or dates of that column, largest or smallest first), `rows: total`
  (no data rows). Footer rows are always shown and are those of the whole
  table. The title of the element names the excerpt (`Expenses · top 5 by
  Amount`).
- A table that does not fit its element ends with `… n more rows`, followed
  by its footer rows if there is room.
- `height: auto` (not for charts): the element takes as many rows as its
  content needs, starting at its first row. The rows written are the
  designed size: elements below it that share a grid column keep their
  distance to its real end, elements beside it stay. Such an element is not
  cut at a page end; it runs on to the next page, a table repeating its
  title and header there. An element that is pushed across a page end, and
  fits on a page, starts at the top of the next page.

  ```
  1-12 / 4-10   table    Expenses   rows: top 5 by Amount
  1-12 / 12-20  table    Journal    height: auto
  1-12 / 22-23  note     Follows the journal, however long it gets.
  ```

## 9. Canonical form

hardcalc writes files in a fixed, deterministic form (the format version
in the first line, compact tables, normalised separator lines) so that
diffs in Git stay readable.
`hardcalc fmt` produces this form; it is idempotent.

Tables are written without padding:

```markdown
## Expenses :: matrix

| Item | Amount:Euro |
|---|--:|
| Rent | 1200.00 |
| A much longer item | 5 |
```

Alignment is only in the separator line (`:--` left, `--:` right, `:-:`
centred). A longer value changes only its own line, so Git shows small
diffs; the table is less tidy in a plain text editor. This applies to the
table, its footer and its slices and scenarios.

`aligned` in the heading (`## Expenses :: matrix aligned`) writes the table
with equally wide columns instead, which looks better in an editor but changes
every line of the table when a value grows. `compact` is accepted in the
heading too and is not written. A table without a heading cannot say
`aligned` and is always compact. `:compact [on|off]` switches it for the
table in the interface.

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
the column names, then the cells as in the table (formulas with `=`). The
`file` entry names the file; tables whose names would give the same file
(`Plan` and `plan`, `A/B` and `A_B`) get a number (`Plan 2.tsv`). If a
TSV differs from its checksum, it was changed outside hardcalc; its rows
replace those of the table when opening (columns by name). `assets/cache/`
may be missing and is not read. Bundles with a foreign `creatorIdentifier`
are not overwritten, unless they are `transient`; neither are bundles with
a higher `formatVersion` – both are saved as a new file. Files whose path
would lead out of the bundle (`../x`) are ignored.

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
| `[Name] …` | label of the line at its start (`[Eq. 3.1] q_d = …`, `[Bending] σ_Ed ≤ f_yd`): shown before the line when printed, in the report and in `hardcalc calc`; it does not take part in the calculation |
| `name(a; b) = expression` | own function (`M_ss(q; L) = q·L^2/8`), usable in the lines below it: `M = M_ss(q_d; L)` |
| `plot(y[; y2 …][; x: x][; type: t][; title: text])` | chart of ranges (see below); with `[Name]` it is the chart `Name` |
| `#full`, `#formula`, `#result` | display of the following lines: full way to the result (default) / formula and result / result only; at the end of a line for that line only |

- **Labels** are text: no number in them is rewritten and no ASCII name
  turned into a Greek letter. `[]` is no label; a heading or a mode line
  has none.
- **Numbers** with a decimal point; a decimal comma is read too and written
  with the point. **Arguments** are separated by `;`, as in formulas:
  `max(a; b)`. On
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
  `tan`, `cot` (angles in ° or rad), `asin`, `acos`, `atan`, `atan2(y; x)`
  (result in °), `sinh`, `cosh`, `tanh`, `ln`, `log`, `exp`; the constant
  `π`. Built-in names are found whatever their case; own functions as
  written.
- **Integral and derivative:** `int(f; x; a; b)` is the integral of `f`
  over `x` from `a` to `b` (Simpson's rule, 256 steps), `diff(f; x; x0)` the
  derivative of `f` by `x` at `x0` (central difference). `x` is a name of
  their own while `f` is evaluated; units multiply and divide
  (`int(q; t; 0 m; L)` with `q` in kN/m → kN). Shown as ∫ and d/dx.
- **Ranges:** `{1, 1.1,,, 2}` are the 11 values from 1 to 2 in steps of 0.1:
  an empty item continues with the step of the two values before it, up to
  the value after it; `{1, 2, 5}` lists values. Inside the braces a comma
  separates (no decimal comma); a unit after `}` applies to all
  (`x = {0, 0.5,,, 5} m`). A line that uses a range is calculated for every
  value (`M = q·x·(L - x)/2`) and shows the smallest and largest value and
  how many: `M = 0 … 31.3 kN·m (11)`; it runs over the range variable it
  came from. Ranges in one line have the same length. `min(M)`, `max(M)` of
  one range give a single value. A check over a range holds when it holds
  for every value; it shows the decisive point, the highest utilisation
  (among the failing points, if any):
  `M = 31.3 kN·m ≤ M_Rd = 40.0 kNm   max η = 0.78 at x = 2.50 m ✓`.
- **Charts:** `[Moments] plot(M)` draws `M` over the range it runs over
  (`x: …` names other x values). Curves of one chart are of one kind. A plot
  is a chart like a ```` ```chart ```` block: in the window **▥ Moments**,
  in layouts (`chart Moments`) and printed after its block; the report
  draws it. Without a label it is called `plot 1`, `plot 2` …
- **Formula library:** every sheet knows the formulas hardcalc brings
  along (beams, cross sections, stresses, buckling, geometry) and those in
  `formulas.hcmd` in the hardcalc configuration folder
  (`$XDG_CONFIG_HOME/hardcalc`, `%APPDATA%\hardcalc` or
  `~/.config/hardcalc`; `HARDCALC_FORMULAS` names another file). That file
  is a hardcalc document: a heading per group, one definition per line,
  `[Title] name(params) = formula   ' note`. A definition in the sheet wins.
- **ASCII input:** `*` → ·, `<=` → ≤, `>=` → ≥, `->` → →. The tab
  **∑ calc** rewrites input this way; both are read.
- **Greek letters in names:** written as such (`γ_M`) or by their LaTeX
  name (`\gamma_M`, `\Delta`, `\varphi`); both are the same name, the
  file keeps what was written. A spelled name (`gamma_M`) is a name of its
  own and is not turned into a letter; `pi` is the constant π. An unknown
  LaTeX name (`\foo`) is an error on its line.

**Display** (full way to the result): formula, values inserted (inputs as
written, without unit), result with 3 significant digits in the display
unit:

```
q_d = 1.35·g_k + 1.5·q_k = 1.35·4.0 + 1.5·3.0 = 9.90 kN/m
σ_Ed = 95.5 N/mm² ≤ f_yd = 214 N/mm²   η = 0.45 ✓
```

`hardcalc calc file` prints the sheet this way (`--ascii`: ASCII only,
always the decimal point), `hardcalc check` reports lines that cannot be calculated.
When printed, the block appears with the way to the result, the
calculations typeset where Typst is available. The PDF embeds DejaVu Sans
Mono (only the characters used), so Greek letters, `≤`, `✓` and arrows
print as they are.

**Typesetting:** the syntax tree is the single source of truth; from it come
the one-line text (Unicode or ASCII), LaTeX (2D display in the terminal,
view ∑ preview) and Typst (calculation report, `hardcalc report`). Indexes
are lowered and set upright (`f_yk` → f<sub>yk</sub>), quotients as
fractions.
