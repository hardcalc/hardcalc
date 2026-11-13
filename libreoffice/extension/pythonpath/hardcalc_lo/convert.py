"""Conversion between a hardcalc document and plain sheet data, without UNO.

Import: `plan_import(src)` turns the Markdown into one `SheetPlan` per
table (header row, typed cells, Calc formulas) and a state dict. The state
is kept in the Calc document; it holds the original Markdown so that the
export can put the tables back into the text, types, layers, footers,
charts and layouts around them, and the original spelling of values and
formulas that did not change.

Export: `export(sheets, state)` turns the sheet data back into Markdown.
"""

import datetime
import re

from . import markdown, styles
from .formula import FormulaError, col_letters
from .translate import Book, TableInfo, from_calc, parse_functions, to_calc

STATE_VERSION = 1
# The version of the file format new documents are written in (SPEC.md §1).
FORMAT_VERSION = 1

# Calc sheet names must not contain these.
_BAD_SHEET_CHARS = "[]*?:/\\"

NOTE_KEPT = "hardcalc formula, not computed in Calc (kept as text)"


# ------------------------------------------------------------ values


def parse_number(s):
    """hardcalc's number reading: `1200,50`, `1.200,50`, `1,200.50` → (value, comma, decimals)."""
    s = s.strip().lstrip("+")
    if not s or not any(c.isdigit() for c in s):
        return None
    sign, body = ("-", s[1:].lstrip()) if s.startswith("-") else ("", s)

    def grouped(t, sep):
        # Separators between thousands group the digits in threes: `1.2.3` is no number.
        parts = t.split(sep)
        return (1 <= len(parts[0]) <= 3 and all(p.isdigit() for p in parts)
                and all(len(p) == 3 for p in parts[1:]))

    for sep in (" ", "'"):
        if sep in body:
            whole = re.split(r"[.,]", body, maxsplit=1)[0]
            rest = body[len(whole):]
            if not grouped(whole, sep) or " " in rest or "'" in rest:
                return None
            body = whole.replace(sep, "") + rest
            break
    commas, dots = body.count(","), body.count(".")

    def both(sep, decimal):
        whole, fraction = body.split(decimal, 1)
        if not grouped(whole, sep) or sep in fraction or decimal in fraction:
            return None
        return whole.replace(sep, "") + "." + fraction

    if commas and dots:
        comma = body.rfind(",") > body.rfind(".")
        norm = both(".", ",") if comma else both(",", ".")
    elif commas == 1:
        norm, comma = body.replace(",", "."), True
    elif commas > 1:
        norm, comma = (body.replace(",", "") if grouped(body, ",") else None), False
    elif dots > 1:
        norm, comma = (body.replace(".", "") if grouped(body, ".") else None), True
    else:
        norm, comma = body, False
    if norm is None:
        return None
    norm = sign + norm
    if not re.match(r"^-?(\d+\.?\d*|\.\d+)([eE][+-]?\d+)?$", norm):
        return None
    try:
        v = float(norm)
    except ValueError:
        return None
    if v != v or v in (float("inf"), float("-inf")):
        return None
    decimals = len(norm.split(".", 1)[1]) if "." in norm and "e" not in norm.lower() else 0
    return v, comma, decimals


_EPOCH = datetime.date(1899, 12, 30)


def parse_date(s):
    """`2026-10-08` or `08.10.2026` → spreadsheet serial (days since 1899-12-30)."""
    s = s.strip()
    m = re.match(r"^(\d{4})-(\d{1,2})-(\d{1,2})$", s) or None
    if m:
        y, mo, d = int(m.group(1)), int(m.group(2)), int(m.group(3))
    else:
        m = re.match(r"^(\d{1,2})\.(\d{1,2})\.(\d{4})$", s)
        if not m:
            return None
        d, mo, y = int(m.group(1)), int(m.group(2)), int(m.group(3))
    try:
        return float((datetime.date(y, mo, d) - _EPOCH).days)
    except ValueError:
        return None


def serial_text(serial):
    """Serial → `2026-10-08`, `2026-10-08 14:30` or `14:30` (as hardcalc's import)."""
    days = int(serial // 1)
    minutes = int(round((serial - days) * 1440))
    if minutes >= 1440:
        days, minutes = days + 1, minutes - 1440
    time = "%02d:%02d" % (minutes // 60, minutes % 60)
    if days == 0:
        return time
    d = _EPOCH + datetime.timedelta(days=days)
    if minutes == 0:
        return d.isoformat()
    return "%s %s" % (d.isoformat(), time)


def point_text(t):
    """A number as files hold it, with a decimal point: `1.200,50` → `1200.50`;
    anything else as it is."""
    p = parse_number(t)
    if p is None or not p[1]:
        return t
    return render_number(p[0], p[2])


def render_number(v, decimals=None):
    if decimals is not None:
        s = "%.*f" % (decimals, v)
    elif v == int(v) and abs(v) < 1e15:
        s = str(int(v))
    else:
        s = "%.15g" % v
    if s.startswith("-") and float(s.replace(",", ".")) == 0:
        s = s[1:]
    return s


def _same(a, b):
    return abs(a - b) <= 1e-9 * max(1.0, abs(a), abs(b))


def _text_value(t):
    """The number or date serial a cell text stands for, if any."""
    p = parse_number(t)
    if p is not None:
        return p[0]
    return parse_date(t)


# Base types whose cells stay text in Calc.
_TEXT_BASES = ("text", "enum", "bool", "time", "datetime", "duration", "ref", "record")


def _kind(base):
    word = re.split(r"[\s(]", base, 1)[0] if base else ""
    if word in _TEXT_BASES:
        return "text"
    if word == "date":
        return "date"
    if word in ("int", "decimal", "float", "unit"):
        return "number"
    return "auto"


def _decimals_of(base):
    m = re.match(r"^decimal\((\d+)\)", base or "")
    return int(m.group(1)) if m else None


# ------------------------------------------------------------ names


def sheet_name(name):
    s = "".join("_" if c in _BAD_SHEET_CHARS else c for c in name).strip().strip("'")
    return s or "Table"


def _unique(name, taken):
    base, k = name, 2
    while name.lower() in taken:
        name = "%s %d" % (base, k)
        k += 1
    taken.add(name.lower())
    return name


def table_name(sheet):
    """A sheet name as hardcalc table name (`::` and `|` are not allowed)."""
    return sheet.strip().replace("::", ":").replace("|", "/") or "Table"


# ------------------------------------------------------------ import


class SheetPlan(object):
    def __init__(self, sheet, header):
        self.sheet = sheet
        self.header = header
        # rows of cells: None (empty), ("n", float), ("d", serial), ("s", text), ("f", calc formula)
        self.cells = []
        self.decimals = {}      # column → decimals for the number format
        self.dates = set()      # columns holding dates
        self.notes = []         # (row, col, text) – sheet positions, header is row 0
        self.formulas = {}      # "r,c" → hardcalc formula (sheet positions)
        self.texts = {}         # "r,c" → original text where it differs from the default
        self.kept = []          # formulas Calc holds as text (not computed there)
        # Formatting to apply in order: (r0, r1, c0, c1, parts) in data rows.
        self.format_ops = []
        # The `filter` layer: [(column, [(operator, value)])] – see parse_checks.
        self.filters = []
        # Columns the `view` layer hides.
        self.hidden_columns = []


_LAYER = re.compile(r"^layer\s+([^\s@]+)[^@]*(?:@(.*))?$")


def format_blocks(segments):
    """`layer format` blocks: [(segment index, first line, last line, table index)].
    A block belongs to its `@Table` or the last table above it."""
    return layer_blocks(segments, "format")


def layer_blocks(segments, layer):
    """The blocks of layer `layer`, as format_blocks gives those of `format`."""
    tables = [s for s in segments if isinstance(s, markdown.Table)]
    by_name = dict((t.name, i) for i, t in enumerate(tables) if t.name is not None)
    out = []
    ti = -1
    for k, seg in enumerate(segments):
        if isinstance(seg, markdown.Table):
            ti += 1
            continue
        for start, end, info in markdown.fences(seg.lines):
            m = _LAYER.match(info)
            if not m or m.group(1) != layer:
                continue
            owner = by_name.get(m.group(2).strip()) if m.group(2) else (ti if ti >= 0 else None)
            if owner is not None:
                out.append((k, start, end, owner))
    return out


def format_rules(segments):
    """Table index → [(target, body)] of its format layer, in document order."""
    out = {}
    for k, start, end, owner in format_blocks(segments):
        body = segments[k].lines[start + 1:end]
        out.setdefault(owner, []).extend(markdown.layer_rules(body))
    return out


def layer_rules_by_table(segments, layer):
    """Table index → [(target, body)] of its layer `layer`, in document order."""
    out = {}
    for k, start, end, owner in layer_blocks(segments, layer):
        body = segments[k].lines[start + 1:end]
        out.setdefault(owner, []).extend(markdown.layer_rules(body))
    return out


_OPERATORS = ("<=", ">=", "<>", "!=", "=", "<", ">")


def _take_value(s):
    """A word or a "quoted text" ("" for a quote inside) at the start of `s`
    and the rest; None if a quote is not closed."""
    if not s.startswith('"'):
        end = 0
        while end < len(s) and not s[end].isspace():
            end += 1
        return s[:end], s[end:]
    out = []
    i = 1
    while i < len(s):
        if s[i] == '"':
            if s[i + 1:i + 2] == '"':
                out.append('"')
                i += 2
                continue
            return "".join(out), s[i + 1:]
        out.append(s[i])
        i += 1
    return None


def _take_list(s):
    """The values of `a, "b, c", d)` up to the closing bracket and the rest."""
    out = []
    rest = s
    while True:
        rest = rest.lstrip()
        if rest.startswith('"'):
            taken = _take_value(rest)
            if taken is None:
                return None
            value, after = taken
        else:
            ends = [i for i in (rest.find(","), rest.find(")")) if i >= 0]
            if not ends:
                return None
            value, after = rest[:min(ends)].strip(), rest[min(ends):]
        out.append(value)
        after = after.lstrip()
        if after.startswith(")"):
            return out, after[1:]
        if not after.startswith(","):
            return None
        rest = after[1:]


def parse_checks(body):
    """The conditions of a filter rule as hardcalc reads them:
    `>= 50 < 1000` → [(">=", "50"), ("<", "1000")], `in(a, "b c")` →
    [("in", ["a", "b c"])], also ("contains", text), ("empty", None),
    ("required", None). None if the rule cannot be read."""
    out = []
    rest = body.strip()
    while rest:
        if rest.startswith("required"):
            out.append(("required", None))
            rest = rest[len("required"):].lstrip()
        elif rest == "empty" or rest.startswith("empty "):
            out.append(("empty", None))
            rest = rest[len("empty"):].lstrip()
        elif rest.startswith("in(") or rest.startswith("contains("):
            name = rest[:rest.index("(")]
            taken = _take_list(rest[len(name) + 1:])
            if taken is None or (name == "contains" and len(taken[0]) != 1):
                return None
            out.append(("in", taken[0]) if name == "in" else ("contains", taken[0][0]))
            rest = taken[1].lstrip()
        else:
            op = next((o for o in _OPERATORS if rest.startswith(o)), None)
            if op is None:
                return None
            after = rest[len(op):].lstrip()
            taken = _take_value(after)
            if taken is None or (taken[0] == "" and not after.startswith('"')):
                return None
            out.append(("<>" if op == "!=" else op, taken[0]))
            rest = taken[1].lstrip()
    return out or None


def _column_of(target, columns):
    """The column a rule target names: by name, else by its letters."""
    if target in columns:
        return columns.index(target)
    if target and all("A" <= ch <= "Z" for ch in target):
        c = 0
        for ch in target:
            c = c * 26 + ord(ch) - ord("A") + 1
        return c - 1 if c - 1 < len(columns) else None
    return None


def filter_plan(rules, columns):
    """The rules of a `filter` layer as [(column, checks)]. A rule that
    cannot be read makes the whole filter unknown: None (nothing is hidden)."""
    out = []
    for target, body in rules:
        c = _column_of(target, columns)
        if c is None:
            continue
        checks = parse_checks(body)
        if checks is None:
            return None
        out.append((c, checks))
    return out


def hidden_columns(rules, columns):
    """The columns a `view` layer hides."""
    out = []
    for target, body in rules:
        c = _column_of(target, columns)
        if c is not None and "hide" in body.split() and c not in out:
            out.append(c)
    return out


def _book(segments, tables, sheets, functions):
    infos = []
    for t, sheet in zip(tables, sheets):
        cols = [markdown.split_header(h)[0] for h in t.header]
        labels = [r[0].strip() if r else "" for r in t.rows]
        infos.append(TableInfo(t.name, sheet, cols, len(t.rows), labels))
    return Book(infos, functions)


def plan_import(src):
    """Markdown → (list of SheetPlan, state dict)."""
    src = src.lstrip("﻿")
    crlf = "\r\n" in src
    text = src.replace("\r\n", "\n")
    segments = markdown.parse(text)
    types = markdown.parse_types(segments)
    functions = {}
    for body in markdown.fenced_blocks(segments, "functions"):
        functions.update(parse_functions(body))
    tables = [s for s in segments if isinstance(s, markdown.Table)]

    taken = set()
    sheets = []
    anonymous = 0
    for t in tables:
        if t.name is None:
            anonymous += 1
            sheets.append(_unique("Table %d" % anonymous, taken))
        else:
            sheets.append(_unique(sheet_name(t.name), taken))
    book = _book(segments, tables, sheets, functions)
    rules_by_table = format_rules(segments)
    filters_by_table = layer_rules_by_table(segments, "filter")
    views_by_table = layer_rules_by_table(segments, "view")

    plans = []
    state_sheets = []
    for ti, (t, sheet) in enumerate(zip(tables, sheets)):
        plan = SheetPlan(sheet, list(t.header))
        plan.format_ops = styles.operations(rules_by_table.get(ti, []), book.tables[ti].columns, len(t.rows))
        plan.filters = filter_plan(filters_by_table.get(ti, []), book.tables[ti].columns) or []
        plan.hidden_columns = hidden_columns(views_by_table.get(ti, []), book.tables[ti].columns)
        kinds, decimals = [], []
        for h in t.header:
            _, ty = markdown.split_header(h)
            base = markdown.base_type(ty, types) if ty else ""
            kinds.append(_kind(base))
            decimals.append(_decimals_of(base))
        # Numbers written with the same number of decimals keep them in Calc.
        seen = [set() for _ in t.header]
        for row in t.rows:
            for c, cell in enumerate(row):
                if cell and not cell.startswith("=") and kinds[c] in ("number", "auto"):
                    p = parse_number(cell)
                    if p is not None:
                        seen[c].add(p[2])
        for c in range(len(t.header)):
            if decimals[c] is None and len(seen[c]) == 1:
                d = next(iter(seen[c]))
                decimals[c] = d if d > 0 else None
            if decimals[c] is not None:
                plan.decimals[c] = decimals[c]

        for r, row in enumerate(t.rows):
            out = []
            for c, cell in enumerate(row):
                pos = "%d,%d" % (r + 1, c)
                if not cell:
                    out.append(None)
                    continue
                if cell.startswith("="):
                    try:
                        out.append(("f", to_calc(cell, book, ti, r)))
                        plan.formulas[pos] = cell
                    except (FormulaError, RecursionError, ValueError, IndexError):
                        out.append(("s", cell))
                        plan.notes.append((r + 1, c, NOTE_KEPT))
                        plan.kept.append(cell)
                    continue
                if cell.startswith("\\="):
                    # Text that starts with `=`: in Calc the text without the mask.
                    out.append(("s", cell[1:]))
                    continue
                kind = kinds[c]
                value = None
                if kind in ("number", "auto"):
                    p = parse_number(cell)
                    if p is not None:
                        value = ("n", p[0])
                        if render_number(p[0], decimals[c]) != cell:
                            plan.texts[pos] = cell
                if value is None and kind in ("date", "auto"):
                    d = parse_date(cell)
                    if d is not None:
                        value = ("d", d)
                        plan.dates.add(c)
                        if serial_text(d) != cell:
                            plan.texts[pos] = cell
                out.append(value or ("s", cell))
            plan.cells.append(out)
        plans.append(plan)
        state_sheets.append({
            "sheet": sheet,
            "table": ti,
            "decimals": dict((str(k), v) for k, v in plan.decimals.items()),
            "texts": plan.texts,
            # filled in by the filter: "r,c" → [Calc formula as read back, hardcalc formula]
            "formulas": {},
            # formulas Calc holds as text: they come back as formulas, any
            # other text that starts with `=` comes back masked (`\=…`)
            "kept": plan.kept,
        })
    state = {"v": STATE_VERSION, "source": text, "crlf": crlf, "sheets": state_sheets}
    return plans, state


# ------------------------------------------------------------ export


class SheetData(object):
    """What the filter reads from a sheet (positions are 0-based, row 0 = header)."""

    def __init__(self, name, values, formulas=None, kinds=None, styles=None):
        self.name = name
        self.values = values            # rows of float / str ("" = empty)
        self.formulas = formulas or {}  # (r, c) → Calc formula
        self.kinds = kinds or {}        # (r, c) → "date", "time", "datetime", "bool"
        # Text style of every cell, rows like `values` (styles.DEFAULT if plain);
        # None: unknown, the format layer stays as it is.
        self.styles = styles


def _used(data):
    rows = len(data.values)
    last_row, last_col = -1, -1
    for r in range(rows):
        for c, v in enumerate(data.values[r]):
            if (v != "" and v is not None) or (r, c) in data.formulas:
                last_row = max(last_row, r)
                last_col = max(last_col, c)
    for (r, c) in data.formulas:
        last_row = max(last_row, r)
        last_col = max(last_col, c)
    return last_row + 1, last_col + 1


def _cell_text(v):
    return v.replace("\r\n", " ").replace("\n", " ").replace("\r", " ")


def _match(sheets, entries):
    """Sheet index → state entry: by sheet name, then the rest in order (renamed sheets)."""
    out = {}
    used = set()
    names = [s.name for s in sheets]
    for i, n in enumerate(names):
        for k, e in enumerate(entries):
            if k not in used and e["sheet"] == n:
                out[i] = e
                used.add(k)
                break
    rest_sheets = [i for i in range(len(sheets)) if i not in out]
    rest_entries = [k for k in range(len(entries)) if k not in used]
    for i, k in zip(rest_sheets, rest_entries):
        out[i] = entries[k]
    return out


def export(sheets, state=None):
    """Sheet data (+ the state from the import) → Markdown text."""
    state = state if state and state.get("v") == STATE_VERSION else None
    segments = markdown.parse(state["source"]) if state else []
    tables = [s for s in segments if isinstance(s, markdown.Table)]
    matches = _match(sheets, state["sheets"]) if state else {}

    # Geometry and names first: formulas may refer to any sheet.
    infos, metas = [], []
    for i, data in enumerate(sheets):
        rows, cols = _used(data)
        entry = matches.get(i)
        seg = tables[entry["table"]] if entry and entry["table"] < len(tables) else None
        if seg is not None and data.name == entry["sheet"]:
            name = seg.name
        else:
            name = table_name(data.name)
        header = []
        for c in range(cols):
            v = data.values[0][c] if rows > 0 and c < len(data.values[0]) else ""
            if v is None:
                v = ""
            if isinstance(v, float):
                v = render_number(v, None)
            v = _cell_text(v).strip()
            header.append(v or col_letters(c))
        columns = [markdown.split_header(h)[0] for h in header]
        labels = []
        for r in range(1, rows):
            v = data.values[r][0] if data.values[r] else ""
            if isinstance(v, float):
                labels.append(render_number(v, None))
            else:
                labels.append(_cell_text(v or "").strip())
        infos.append(TableInfo(name, data.name, columns, max(rows - 1, 0), labels))
        metas.append((rows, cols, entry, seg, header))
    book = Book(infos)

    generated = {}
    notes = []
    for i, data in enumerate(sheets):
        rows, cols, entry, seg, header = metas[i]
        if rows == 0:
            continue
        decimals = dict((int(k), v) for k, v in (entry or {}).get("decimals", {}).items())
        texts = (entry or {}).get("texts", {})
        # Without the list (a state of an older version): as before, unmasked.
        kept = (entry or {"kept": []}).get("kept")
        known = (entry or {}).get("formulas", {})
        out_rows = []
        for r in range(1, rows):
            row = data.values[r] if r < len(data.values) else ()
            out = []
            for c in range(cols):
                v = row[c] if c < len(row) else ""
                pos = "%d,%d" % (r, c)
                if (r, c) in data.formulas:
                    f = data.formulas[(r, c)]
                    k = known.get(pos)
                    if k and k[0] == f:
                        out.append(k[1])
                        continue
                    try:
                        out.append(from_calc(f, book, i))
                        continue
                    except (FormulaError, RecursionError, ValueError, IndexError):
                        notes.append((i, "%s%d" % (col_letters(c), r), "original formula: %s" % f))
                if isinstance(v, float):
                    kind = data.kinds.get((r, c))
                    t = texts.get(pos)
                    if kind == "bool":
                        out.append("true" if v else "false")
                    elif t is not None and _text_value(t) is not None and _same(_text_value(t), v):
                        out.append(point_text(t))
                    elif kind in ("date", "time", "datetime"):
                        out.append(serial_text(v))
                    else:
                        out.append(render_number(v, decimals.get(c)))
                else:
                    t = _cell_text(v or "")
                    if t.startswith("=") and kept is not None and t not in kept:
                        t = "\\" + t
                    out.append(t)
            out_rows.append(out)
        if seg is not None:
            level, shape, aligns = seg.level, seg.shape, seg.aligns
            if infos[i].name is not None and level is None:
                level, shape = 2, "matrix"
        else:
            level, shape, aligns = 2, "matrix", []
        if seg is not None and len(seg.header) == len(header):
            # Alignment by column name, else by position.
            old = dict(zip(seg.header, seg.aligns))
            aligns = [old.get(h, seg.aligns[c]) for c, h in enumerate(header)]
        table = markdown.Table(level, infos[i].name, shape, header, aligns, out_rows)
        if (seg is not None and seg.raw is not None and seg.name == table.name
                and seg.header == table.header and seg.rows == table.rows):
            table.raw = seg.raw
        generated[i] = table

    by_table = dict((e["table"], i) for i, e in matches.items())
    result = []
    sheet_of = {}   # id of a table in `result` → sheet index
    ti = 0
    for seg in segments:
        if isinstance(seg, markdown.Table):
            i = by_table.get(ti)
            if i is not None and i in generated:
                sheet_of[id(generated[i])] = i
                result.append(generated.pop(i))
            ti += 1
        else:
            result.append(seg)
    # Sheets that were not in the document become new tables at the end.
    for i in sorted(generated):
        _blank_line(result)
        sheet_of[id(generated[i])] = i
        result.append(generated[i])
        result.append(markdown.Text([""]))
    _update_formats(result, sheets, infos, sheet_of)
    # Formulas Calc could not hand back: values in the table, formulas as comments.
    by_sheet = {}
    for i, target, text in notes:
        by_sheet.setdefault(i, []).append("%s: %s" % (target, text))
    for i in sorted(by_sheet):
        name = infos[i].name
        info = "layer comments" + (" @%s" % name if name else "")
        # Saved before: the block is already in the document.
        present = set(l.strip() for body in markdown.fenced_blocks(result, info) for l in body)
        lines = [l for l in by_sheet[i] if l not in present]
        if lines:
            _blank_line(result)
            result.append(markdown.Text(["```" + info] + lines + ["```", ""]))
    text = markdown.write(result)
    if not state:
        # A new document names the format version it is written in. (One read
        # from a file keeps its first line, with or without a version.)
        text = "<!-- hardcalc %d -->\n\n%s" % (FORMAT_VERSION, text.lstrip("\n"))
    if not text.endswith("\n"):
        text += "\n"
    if state and state.get("crlf"):
        text = text.replace("\n", "\r\n")
    return text


def _update_formats(result, sheets, infos, sheet_of):
    """Replaces the format layer of every table whose cells Calc formatted
    differently from what the layer says."""
    tables = [s for s in result if isinstance(s, markdown.Table)]
    blocks = format_blocks(result)
    edits = []      # (segment index, first, last, new lines) – replace
    inserts = []    # (table position in result, rule lines)
    for t_index, table in enumerate(tables):
        i = sheet_of.get(id(table))
        if i is None or sheets[i].styles is None:
            continue
        info = infos[i]
        rows, cols = info.rows, len(info.columns)
        grid = [list((sheets[i].styles[r + 1] if r + 1 < len(sheets[i].styles) else [])[:cols]) for r in range(rows)]
        for row in grid:
            row.extend([styles.DEFAULT] * (cols - len(row)))
        own = [b for b in blocks if b[3] == t_index]
        old = []
        for k, start, end, _ in own:
            old.extend(markdown.layer_rules(result[k].lines[start + 1:end]))
        new = styles.new_layer(old, grid, info.columns)
        if new is None:
            continue
        lines = ["%s: %s" % (target, body) for target, body in new]
        if own:
            for n, (k, start, end, _) in enumerate(own):
                edits.append((k, start, end, lines if n == 0 and lines else None))
        elif lines:
            inserts.append((result.index(table), lines))
    # Later positions first, so earlier indexes stay valid.
    for k, start, end, lines in sorted(edits, key=lambda e: (e[0], e[1]), reverse=True):
        seg = result[k].lines
        if lines is not None:
            seg[start + 1:end] = lines
        else:
            a, b = start, end + 1
            if a > 0 and seg[a - 1] == "":
                a -= 1
            elif b < len(seg) and seg[b] == "":
                b += 1
            del seg[a:b]
    for pos, lines in sorted(inserts, reverse=True):
        block = ["```layer format"] + lines + ["```"]
        if pos + 1 < len(result) and isinstance(result[pos + 1], markdown.Text):
            seg = result[pos + 1].lines
            at = 0
            for start, end, info in markdown.fences(seg):
                between = seg[at:start]
                if all(not l.strip() for l in between) and info.startswith(("footer", "slice ", "scenario ", "layer ")):
                    at = end + 1
                else:
                    break
            seg[at:at] = [""] + block
        else:
            result.insert(pos + 1, markdown.Text([""] + block + [""]))


def _blank_line(result):
    """Ends the document so far with exactly one blank line before a new block."""
    while result and isinstance(result[-1], markdown.Text) and result[-1].lines and result[-1].lines[-1] == "":
        result[-1].lines.pop()
        if not result[-1].lines:
            result.pop()
    if result:
        result.append(markdown.Text([""]))
