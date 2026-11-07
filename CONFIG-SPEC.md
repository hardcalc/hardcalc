# hardconf – configuration files with a built-in schema (draft v0)

`hardconf` is the configuration editor of the hardcalc project. It edits
**TOML files that carry their own specification**: types, structs, defaults,
hints and documentation are kept in a schema block at the end of the file.
On the left hardconf shows the tree of the configuration, on the right a form
for the selected node and the documentation of the field under the cursor.

Principles:

1. **The file stays valid TOML.** The schema block consists of comments
   only. The program that reads the file needs to know nothing about it, and
   any text editor remains a full alternative.
2. **Documentation once per type, not once per instance.** Ten jobs do not
   mean the same comment block ten times.
3. **One type system with hardcalc.** Your own types are written exactly as
   in a ```` ```types ```` block ([SPEC.md](SPEC.md) §3), extended by a few
   modifiers that configurations need (§3.2).
4. **Minimal changes when saving.** hardconf changes only the values that
   were edited. Formatting, order and comments are kept so that diffs stay
   readable.
5. **It works without a schema too.** hardconf opens a TOML file without a
   schema block as a generic tree editor. The types are then derived from the
   values.

## 1. Structure of a file

```toml
# any comments and data, ordinary TOML
revision = "a41c…"

[targets.nas1]
id       = 3
host     = "nas.example.lan"
ssh_port = 22
tls      = "pin"

[jobs.photos]
id    = 1
token = "tok_3c9…"

#%hardconf 1
#%
#%types
#%  Port = int range 1..65535
#%
#%root "Backup configuration"
#%  targets : map(Target, Alias)
#%  jobs    : map(Job, Name)
#%
#%struct Target "Storage target"
#%  /// A server that receives backups over SSH.
#%  id?      : int readonly
#%  host     : text            "Name or address of the server"
#%  ssh_port : Port = 22
#%  …
```

A file consists of these parts:

| Part | Recognised by | Section |
|---|---|---|
| **Data** | everything except `#%` lines, ordinary TOML | 4 |
| **Schema** | lines that start with `#%` (without leading spaces) | 2, 3 |
| **Diagnostics** | lines `#%error …` / `#%warning …` | 6 |

**Place of the schema.** All `#%` lines of the file form the schema block in
file order. Only TOML comment lines count: a line inside a multi-line string
(`"""…"""`) that starts with `#%` is part of the value. Programs that write a
file put the block **at the end**. hardconf reads it anywhere, leaves it
where it is when saving and never changes it.

**Identifier.** The first schema line that is not a diagnostic (§6) reads
`#%hardconf <version>`. Diagnostics before it are allowed, because a program
typically writes them at the start of the file. This document describes
version `1`. Without the identifier, the `#%` lines are treated as ordinary
comments and the file is opened without a schema.

The content of a schema line is everything after `#%`, without leading
spaces. Indentation is only a reading aid and has no meaning. `#%hardconf`,
`#%error` and `#%warning` always follow `#%` directly.

## 2. The schema block

```
Schema    = Identifier { Section }
Section   = Types | Root | Struct
Types     = "types" { TypeDefinition }
Root      = "root" [Hint] { Doc } { Field }
Struct    = "struct" Name [Hint] { Doc } { Field }
Field     = Name ["?"] ":" TypeExpression ["=" Default] { Attribute } [Hint] { Doc }
Attribute = "readonly" | "when" Condition
Doc       = "///" Text
Hint      = string in "…"
```

Strings in the schema (hints, titles, `pattern`, `unit`, `special`) are
written in `"…"`. The only escape is `\"`; all other backslashes stay as they
are, so that regular expressions like `"^[^@\s]+$"` stay readable. Defaults
(`= …`), on the other hand, are real TOML literals.

Empty `#%` lines are allowed. Lines that start with `#% #` are comments on
the schema itself and are ignored.

### 2.1 Types

```
#%types
#%  Port     = int range 1..65535
#%  Limit    = int range 0..        "0 = unlimited"
#%  TlsMode  = enum(verify, ca, pin)
#%  Euro     = decimal(2) unit "€"
#%  Pattern  = text pattern "^[^\s]+$"
#%  Password = text secret special("********": "keep the stored password")
```

The syntax is that of the hardcalc ```` ```types ```` block, one type per
line in the form `Name = TypeExpression`. A hint at the end of the line
applies to every field of this type that has none of its own.

### 2.2 Root and structs

`root` describes the top level of the file, `struct Name` a named TOML
table. Every file has at most one `root` section. Without `root`, the schema
would only apply to tables reached through fields of other structs, that is,
to none. A schema without `root` is therefore an error.

The hint in the header is the title hardconf shows in the form above the
node (`Storage target: nas1`). `///` lines right after the header are the
description of the struct. For `root` they appear as an overview when
hardconf starts.

The order of the fields in the schema is the order in the form and in the
tree.

### 2.3 Fields

```
#%  ssh_port   : Port = 22
#%  tls        : TlsMode = "verify"   "Check of the API certificate"
#%    /// "verify": valid chain of a public CA.
#%    /// "ca":     valid chain of the CA in tls_ca (PEM, e.g. your own CA).
#%    /// "pin":    exactly the certificate with tls_fingerprint (SHA-256).
#%  tls_name?  : text when tls in (verify, ca)
#%  tls_ca?    : text multiline when tls = ca
#%  id?        : int readonly         "Links the entry to the database"
```

| Part | Meaning |
|---|---|
| `name` | TOML key. Anything other than `A–Z a–z 0–9 _ -` in `"…"` |
| `?` | optional: the field may be missing |
| `: TypeExpression` | type name or expression, §3 |
| `= Default` | TOML literal (`22`, `"verify"`, `true`, `[]`). Applies when the field is missing |
| `readonly` | shown, but not editable |
| `when …` | the field only has an effect under a condition, §2.4 |
| `"Hint"` | short text, in the form next to the value |
| `///` | detailed documentation, in the documentation panel |

A field **with** a default may be missing, also without `?`. The difference:
for a missing field with a default, hardconf shows the default (marked as
“default”), for a missing optional field without a default “not set”. A
field **without** `?` and without a default is required.

For struct and map fields, `= {}` is the usual default: a missing table then
means “all fields at their defaults” or “empty map”. This corresponds to
`#[serde(default)]` in Rust. An optional struct field without a default
(`global? : Rules`), on the other hand, distinguishes “missing” from
“empty”.

Like a type definition, the type expression of a field may carry modifiers
(§3.2), e.g. `notify? : Address special("": "off")` or
`display_name? : text length ..100`. They add to or replace those of the
named type. The type expression ends before `=`, `readonly`, `when` or the
closing hint.

`///` lines belong to the last field or struct header before them. hardconf
wraps them to the width of the documentation panel. Consecutive lines form a
paragraph. A line starts anew when the previous one ends with `.`, `:`, `!`
or `?`, when one of the two is empty or when it is indented. This keeps
lists like `"verify": …` and indented examples intact.

### 2.4 Conditions

```
when tls = ca
when tls != pin
when tls in (verify, ca)
when transport = "http"
when skip_hidden
```

A condition refers to a field of the same struct. A field name alone means:
the `bool` field is `true`, or the field is set. Values are written like enum
variants without quotes or as TOML literals. If the condition is not met,
hardconf shows the field dimmed. A value in it is not an error but is marked
as “has no effect”.

## 3. Type expressions

### 3.1 Base types and their form in TOML

| Type | TOML | Note |
|---|---|---|
| `int` | integer | |
| `float` | float | |
| `decimal(n)` | float | written with exactly n decimal places |
| `text` | string | |
| `bool` | boolean | input also `yes/no`, written as `true/false` |
| `date` | local date | input also `08.10.2026`, written as `2026-10-08` |
| `time` | local time | |
| `datetime` | offset/local date-time | |
| `duration` | string | e.g. `"90s"`, `"1h30m"` |
| `enum(a, b, …)` | string | choice from the variants |
| `record(f: T, …)` | inline table | anonymous struct without documentation |
| `list(T)` | array | `list(Struct)` as an array of tables `[[…]]` |
| `map(T)` / `map(T, K)` | table | any keys, values of type `T`; `K` is the type of the keys, §3.3 |
| `ref(path)` | string | key of an entry of the map at `path`, §3.4 |
| *StructName* | table | `[a.b]`, written as an inline table or with dotted keys |

There is no `auto` in schemas. Without a schema, hardconf derives the type
from the TOML value.

### 3.2 Modifiers

In addition to `unit "…"` and `check <name>` from hardcalc:

| Modifier | Applies to | Meaning |
|---|---|---|
| `range a..b` | `int`, `float`, `decimal` | range of values, open on one side with `a..` or `..b` |
| `pattern "re"` | `text` | regular expression, must match the whole value |
| `secret` | `text` | shown masked, revealed with a key |
| `multiline` | `text` | input in a multi-line text area, written as `"""…"""` |
| `special("w": "meaning")` | all | an additionally allowed value with a fixed meaning, may be repeated |
| `length a..b` | `text`, `list`, `map` | length in characters, elements or entries |

Named checks (`check`): `iban`, `email`, `host`, `ip`, `cidr`, `url`. More
are added as needed, in hardcalc and hardconf alike.

`special` covers placeholders like `"********"` (keep the password) and
`"new"` (renew the token). Such a value is always valid, even if it does not
meet `pattern` or `check`. hardconf offers it as an action in the form.

### 3.3 Maps and their keys

```
#%  jobs : map(Job, Name)
#%types
#%  Name = text pattern "^[a-z][a-z0-9_-]*$"
```

In the tree a map is a node with one child per entry, in file order. The
label of the key (“new entry in jobs – Name:”) is the hint of the key type,
without a hint its name. With `ref(path)` as the key type, it is the title of
the struct of the target map (“Storage target”). Without `K` the key is
`text`.

### 3.4 References

```
#%struct Job
#%  targets : map(TargetAccess, ref(targets))
```

`ref(path)` allows only keys that exist in the map at `path` (path syntax
§5). hardconf offers them as a choice and reports references into nothing as
errors. `ref` without an argument remains the hardcalc type for cell and
table references.

## 4. The data part

The data part is ordinary TOML 1.0. For matching it to the schema:

- A field of struct type corresponds to a TOML table, whether written as an
  `[a.b]` header, as an inline table or with dotted keys
  (`include.both = […]`).
- Keys the schema does not know are shown by hardconf with a warning (“not
  in schema”). They stay editable and are kept.
- **Free comments** in the data part are kept when saving. hardconf shows
  comments right above a table or a key in the documentation panel under
  “Comment in the file”. This keeps information visible that a generating
  program writes per instance (e.g. “Used by: photos, documents”).
- **Assigning comments.** A comment block belongs to the key or table header
  directly below it if there is no blank line in between. A comment block at
  the end of a table that is separated from the next table header by a blank
  line belongs to the table before it. In the example, “No certificate
  accepted yet …” therefore belongs to `targets.nas1`, not to
  `accounts.archive`. What stands before the first key or table and is
  separated by a blank line is the comment of the whole file.
- **Editing comments.** hardconf writes the comment of a table or key as
  `# …` lines directly above it, that of the file at the start. Parts that
  were elsewhere before (such as at the end of the table) move along. A field
  that is not in the file, or a table without a line of its own (inline
  table, dotted keys), has no place for a comment.
- **Comments and note fields.** Comments are only kept as long as the program
  does not generate the file anew. Free text that should be stored
  permanently belongs in a field of the schema, usually
  `notes? : text multiline "Notes"`, which the program then has to know as
  well.

## 5. Paths

Paths name nodes and fields, in diagnostics (§6), in `ref(…)` and on the
command line of hardconf:

```
targets.nas1.tls
jobs.photos.accounts.private.display_name
jobs."my job".include.both[2]
```

Keys as in TOML, quoted in `"…"` where needed. List elements with `[n]`,
counted from 0.

## 6. Diagnostics

A program that rejects an edited file can write its error messages into the
file as diagnostic lines before it opens the editor again:

```
#%error jobs.photos.notify "not a valid email address"
#%warning targets.nas1 "no certificate accepted yet"
#%error "the file holds two jobs with the same id"
```

Without a path, the message applies to the whole file. hardconf marks the
affected nodes in the tree and shows the message in the form at the field.
Diagnostic lines may be anywhere, also outside the schema block. They are
**removed when saving**, because after a change they are out of date.

## 7. Behaviour of hardconf

### 7.1 View

- **Format** (title bar): a mark after the file name shows what kind of file
  is open: “✓ hardconf v1” (schema block usable), “✗ hardconf schema
  problems” (schema block faulty or unusable) or “plain TOML” (no schema
  block, §7.5). The file dialog marks `.toml` files with a schema block with
  “hardconf”.
- **Tree** (left): root, structs, maps, lists of structs and map entries.
  Plain values are not in the tree but in the form. Nodes with errors or
  warnings are marked, also collapsed parent nodes.
- **Form** (top right): one field per row with name, value, hint and state
  (“default”, “not set”, “fixed”, “has no effect”, errors). The input follows
  the type: a choice for `enum` and `ref`, a toggle for `bool`, masked for
  `secret`, multi-line for `multiline`. A `multiline` text appears in the
  form as a text area of up to five lines, the hint then below the name.
  Lists of plain values appear as a list with one element per row.
  `list(Struct)` is planned as a table, one element per row and one field per
  column (today: one tree node per element).
- **Documentation panel** (bottom right): type, default, range, `///`
  documentation and the comment from the file for the field or node under
  the cursor.

### 7.2 Editing

| Action | Effect in the file |
|---|---|
| change a value | only this value is replaced |
| reset to default | the key is removed; the default of the reading program applies again (required fields without a default cannot be reset) |
| add a map entry | the key is checked against the key type (for `ref`/`enum` a choice of the keys still free); a new table after the last entry of the map and after its closing comment; only required fields are written (empty). Defaults stay those of the reading program and are not fixed in the file |
| rename a map entry | *planned:* the table header (and the headers of sub-tables) are renamed; `ref` references to it are changed too after asking |
| delete an entry | the table with its sub-tables, the comment directly above it and its own closing comment is removed; comments of the neighbouring tables stay (§4). The question names `ref` references that point into nothing afterwards |
| add a missing optional struct | an empty table with defaults is inserted |
| edit a comment | `# …` lines above the table header or key (§4); empty text removes the comment |

Adding and deleting exist only for entries of `map(…)` fields, and only as
far as the schema allows: `readonly` on the map forbids both, `length a..b`
limits the number of entries (e.g. `jobs : map(Job, Name) length 1..5 = {}`).
With a schema, hardconf adds and deletes no other tables.

### 7.3 Saving and quitting

- Saving writes the whole file anew, but with minimal changes compared to
  the original (§4, principle 4). The schema block is kept byte for byte;
  only the diagnostic lines (also those inside the block) are removed.
- If there are errors, hardconf asks before saving. The final check is up to
  the program that reads the file.
- **Quitting without saving leaves the file untouched**, the diagnostic
  lines included. Programs that use hardconf as their editor recognise from
  this that editing was cancelled.

### 7.4 Invocation

```bash
hardconf file.toml                # open
hardconf                          # choose a file in the file dialog (also later: F5 / Ctrl+E)
EDITOR=hardconf backupd config    # as the editor of another program
```

Planned: `hardconf check file.toml` (check without the interface,
diagnostics on stderr, exit code 1 on errors) and `hardconf schema file.toml`
(print the schema as an overview).

### 7.5 Without a schema

hardconf edits a file without a `#%hardconf` identifier as a generic tree. An
example is [examples/general.toml](examples/general.toml).

- **Types from the values.** An input keeps the kind of the previous value:
  text stays text, a number a number, a date a date (input also
  `24.12.2026`). If the input does not fit, it is rejected. A new list
  element follows the last element.
- **Change the type** (F9): text, multi-line text, integer, decimal number,
  truth value, date or list. The value is converted where possible
  (`"8443"` → `8443`, a value → a list with this element, a list with one
  element → that element). Otherwise an empty value of the new kind is
  created, and hardconf says so.
- **Multi-line texts:** a string with a line break or written in `"""…"""`
  or `'''…'''` appears as a text area and is edited over several lines; it
  is written as `"""…"""`.
- **New** (F7): in a table, a key. First the name, then the kind: the kinds
  above, a table (`[a.b]`, in inline and dotted tables in their form) or a
  list of tables (`[[a.b]]`). A value is edited right away. In a list of
  tables, F7 adds an element. New tables are placed after the last table of
  their parent table.
- **Delete:** F8 removes a table, an element of a list of tables or an
  inline table with its content (after asking, comments as in §7.2); Del or
  F8 on a value removes the key.
- No checks, no defaults, no masking.

## 8. Open / later

- Schema from a separate file (`#%include`), for programs that cannot append
  a block.
- A derive macro that generates the schema from Rust structs.
- Conditionally required fields (`required when …`).
- Documentation per enum variant.
- Search in the tree and the form.
