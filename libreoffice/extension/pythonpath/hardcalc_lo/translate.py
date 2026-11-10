"""Formula translation between hardcalc and LibreOffice Calc.

Calc formulas are written and read in the grammar of the UNO API
(`XCell.setFormula` / `getFormula`): English function names, `;` between
arguments, `$'Sheet'.B3` for other sheets.

Every table becomes a sheet with the header in row 1, so hardcalc data row
1 is Calc row 2. Mirrors the export (src/export.rs) and import
(src/import.rs) of hardcalc.
"""

import re

from . import builtins
from .formula import (FormulaError, col_letters, letters_to_col, parse_formula,
                      parse_partial, parse_ref)


class TableInfo(object):
    """A table as seen by formulas: hardcalc name, sheet, columns, rows."""

    def __init__(self, name, sheet, columns, rows, labels=None):
        self.name = name
        self.sheet = sheet
        self.columns = list(columns)
        self.rows = rows
        self.labels = list(labels) if labels is not None else []


class Book(object):
    def __init__(self, tables, functions=None):
        self.tables = tables
        # name → (params, body tree)
        self.functions = functions or {}

    def by_name(self, name):
        for i, t in enumerate(self.tables):
            if t.name == name:
                return i
        return None

    def by_sheet(self, sheet):
        for i, t in enumerate(self.tables):
            if t.sheet == sheet:
                return i
        return None


def parse_functions(lines):
    """`functions` block lines → {name: (params, body tree)}; bad lines are skipped."""
    out = {}
    for line in lines:
        t = line.strip()
        if not t or t.startswith("#"):
            continue
        if " #" in t:
            t = t.split(" #", 1)[0]
        if "=" not in t:
            continue
        head, body = t.split("=", 1)
        head = head.strip()
        if "(" in head:
            name, params = head.split("(", 1)
            if not params.endswith(")"):
                continue
            # `;` between parameters; files from before format version 1 say `,`.
            params = [p.strip() for p in params[:-1].replace(",", ";").split(";") if p.strip()]
        else:
            name, params = head, []
        name = name.strip().lower()
        if not name or builtins.is_builtin(name):
            continue
        try:
            out[name] = (params, parse_formula(body.strip()))
        except FormulaError:
            continue
    return out


def substitute(e, params, args):
    kind = e[0]
    if kind == "name":
        if e[1] in params:
            i = params.index(e[1])
            return args[i] if i < len(args) else e
        return e
    if kind in ("neg", "at", "pct"):
        return (kind, substitute(e[1], params, args))
    if kind == "bin":
        return ("bin", e[1], substitute(e[2], params, args), substitute(e[3], params, args))
    if kind in ("call", "index"):
        return (kind, e[1], [substitute(a, params, args) for a in e[2]])
    return e


def number(n):
    if n == int(n) and abs(n) < 1e15:
        return str(int(n))
    return repr(n)


_PRECEDENCE = {"=": 1, "<>": 1, "<": 1, "<=": 1, ">": 1, ">=": 1, "&": 2, "+": 3, "-": 3, "*": 4, "/": 4, "^": 5}


def quote_sheet(sheet):
    return "$'%s'." % sheet.replace("'", "''")


# ------------------------------------------------------------ hardcalc → Calc


class _ToCalc(object):
    def __init__(self, book, here, depth=0):
        self.book = book
        self.here = here
        self.depth = depth

    def table(self, name):
        i = self.book.by_name(name)
        if i is None:
            raise FormulaError("unknown table '%s'" % name)
        return i

    def column(self, t, name):
        cols = self.book.tables[t].columns
        if name in cols:
            return cols.index(name)
        if name and name.isascii() and name.isalpha() and name.isupper():
            c = letters_to_col(name)
            if c < len(cols):
                return c
        return None

    def prefix(self, t):
        return "" if t == self.here else quote_sheet(self.book.tables[t].sheet)

    def cell(self, t, row, col, col_abs=False, row_abs=False):
        return "%s%s%s%s%d" % (self.prefix(t), "$" if col_abs else "", col_letters(col),
                               "$" if row_abs else "", row + 2)

    def column_range(self, t, col):
        rows = self.book.tables[t].rows
        if rows == 0:
            raise FormulaError("empty table")
        l = col_letters(col)
        return "%s$%s$2:$%s$%d" % (self.prefix(t), l, l, rows + 1)

    def row_of(self, t, col, label):
        c = self.column(t, col)
        if c is None:
            raise FormulaError("unknown column '%s'" % col)
        labels = self.book.tables[t].labels
        if label not in labels:
            # Footer rows are not shown in Calc.
            raise FormulaError("row '%s' not found" % label)
        return self.cell(t, labels.index(label), c)

    def name(self, t, name, row, agg):
        info = self.book.tables[t]
        if name in info.columns:
            return self.column_ref(t, info.columns.index(name), row, agg)
        ref = parse_ref(name)
        if ref is not None and ref[2] is not None:
            col, col_abs, r, row_abs = ref
            return self.cell(t, r, col, col_abs, row_abs)
        c = self.column(t, name)
        if c is not None:
            return self.column_ref(t, c, row, agg)
        if name in info.labels:
            return self.cell(t, info.labels.index(name), len(info.columns) - 1)
        raise FormulaError("unknown name '%s'" % name)

    def column_ref(self, t, col, row, agg):
        if agg:
            return self.column_range(t, col)
        if t == self.here:
            return self.cell(t, row, col)
        raise FormulaError("a column of another table outside an aggregate")

    def expr(self, e, row, agg):
        kind = e[0]
        if kind in ("num", "qty"):
            return number(e[1])
        if kind == "str":
            return '"%s"' % e[1].replace('"', '""')
        if kind == "bool":
            return "TRUE()" if e[1] else "FALSE()"
        if kind == "name":
            return self.name(self.here, e[1], row, agg)
        if kind == "at":
            inner = e[1]
            if inner[0] == "name":
                return self.name(self.here, inner[1], row, False)
            if inner[0] == "member" or (inner[0] == "remote" and inner[2][0] == "name"):
                t = self.table(inner[1])
                col = inner[2] if inner[0] == "member" else inner[2][1]
                c = self.column(t, col)
                if c is None:
                    raise FormulaError("unknown column '%s'" % col)
                return self.cell(t, row, c)
            raise FormulaError("unsupported @ reference")
        if kind == "range":
            return self.range(self.here, e[1], e[2])
        if kind == "member":
            if self.book.by_name(e[1]) is None:
                raise FormulaError("data layers stay in hardcalc")
            t = self.table(e[1])
            c = self.column(t, e[2])
            if c is None:
                raise FormulaError("unknown column '%s.%s'" % (e[1], e[2]))
            return self.column_range(t, c)
        if kind == "layer":
            raise FormulaError("data layers stay in hardcalc")
        if kind == "remote":
            t = self.table(e[1])
            sub = _ToCalc(self.book, t, self.depth)
            inner = e[2]
            if inner[0] == "name":
                n = inner[1]
                c = sub.column(t, n)
                ref = parse_ref(n)
                if c is not None and not (ref is not None and ref[2] is not None):
                    return self.prefix(t) + sub.column_range(t, c)
                return self.prefix(t) + sub.name(t, n, row, True)
            if inner[0] == "range":
                return self.prefix(t) + sub.range(t, inner[1], inner[2])
            if inner[0] == "index" and len(inner[2]) == 1 and inner[2][0][0] in ("name", "str"):
                return self.prefix(t) + sub.row_of(t, inner[1], inner[2][0][1])
            raise FormulaError("unsupported reference to another table")
        if kind == "index":
            return self.index(e[1], e[2])
        if kind == "neg":
            return "-" + self.wrap(e[1], row, agg)
        if kind == "pct":
            return self.wrap(e[1], row, agg) + "%"
        if kind == "bin":
            return self.wrap(e[2], row, agg, e[1], False) + e[1] + self.wrap(e[3], row, agg, e[1], True)
        if kind == "call":
            return self.call(e[1], e[2], row)
        raise FormulaError("unsupported expression")

    def wrap(self, e, row, agg, parent=None, right=False):
        """A child expression, in parentheses where the operators require it."""
        s = self.expr(e, row, agg)
        if e[0] != "bin":
            return s
        if parent is None:
            return "(%s)" % s
        mine, theirs = _PRECEDENCE[e[1]], _PRECEDENCE[parent]
        if mine < theirs or (mine == theirs and (right or parent == "^")):
            return "(%s)" % s
        return s

    def range(self, t, a, b):
        ref = parse_ref(a)
        end = parse_partial(b)
        if ref is not None and ref[2] is None and end is not None and end[0] is not None and end[2] is None:
            # Whole columns: B:C
            rows = self.book.tables[t].rows
            if rows == 0:
                raise FormulaError("empty table")
            lo, hi = sorted((ref[0], end[0]))
            return "%s$%s$2:$%s$%d" % (self.prefix(t), col_letters(lo), col_letters(hi), rows + 1)
        if ref is None or ref[2] is None or end is None:
            raise FormulaError("bad range or one across data layers")
        col, col_abs, r, row_abs = ref
        c2, c2_abs, r2, r2_abs = end
        if c2 is None:
            c2, c2_abs = col, col_abs
        if r2 is None:
            r2, r2_abs = r, row_abs
        return "%s:%s%s%s%d" % (self.cell(t, r, col, col_abs, row_abs), "$" if c2_abs else "",
                                col_letters(c2), "$" if r2_abs else "", r2 + 2)

    def index(self, name, args):
        if self.book.by_name(name) is None:
            # Column[Label]
            if len(args) == 1 and args[0][0] in ("name", "str"):
                return self.row_of(self.here, name, args[0][1])
            raise FormulaError("unknown table '%s'" % name)
        t = self.table(name)
        if len(args) == 2 and not (args[0][0] == "bin" and args[0][1] == "="):
            r, c = args
            info = self.book.tables[t]
            if r[0] == "num" and r[1] >= 1:
                row = int(r[1]) - 1
            elif r[0] in ("name", "str") and r[1] in info.labels:
                row = info.labels.index(r[1])
            else:
                raise FormulaError("unsupported row key")
            if c[0] in ("name", "str"):
                col = self.column(t, c[1])
            elif c[0] == "num" and c[1] >= 1:
                col = int(c[1]) - 1
            else:
                col = None
            if col is None:
                raise FormulaError("unknown column")
            return self.cell(t, row, col)
        raise FormulaError("filters are only translated inside sum/avg/count")

    def call(self, name, args, row):
        if name in self.book.functions:
            params, body = self.book.functions[name]
            if self.depth >= 16 or len(args) != len(params):
                raise FormulaError("%s cannot be inlined" % name)
            inner = _ToCalc(self.book, self.here, self.depth + 1)
            return "(%s)" % inner.expr(substitute(body, params, args), row, False)
        # sum/avg/count over a filtered tensor → SUMIFS / AVERAGEIFS / COUNTIFS
        if (len(args) == 1 and args[0][0] == "index" and args[0][2]
                and all(c[0] == "bin" and c[1] == "=" for c in args[0][2])):
            t = self.table(args[0][1])
            last = len(self.book.tables[t].columns) - 1
            parts = []
            for cond in args[0][2]:
                l, r = cond[2], cond[3]
                if l[0] != "name":
                    raise FormulaError("unsupported filter")
                c = self.column(t, l[1])
                if c is None:
                    raise FormulaError("unknown filter column")
                if r[0] in ("name", "str"):
                    value = '"%s"' % r[1].replace('"', '""')
                elif r[0] == "num":
                    value = number(r[1])
                else:
                    raise FormulaError("unsupported filter value")
                parts.append("%s;%s" % (self.column_range(t, c), value))
            values = self.column_range(t, last)
            b = builtins.find(name)
            base = b[0] if b else name
            if base == "sum":
                return "SUMIFS(%s;%s)" % (values, ";".join(parts))
            if base == "avg":
                return "AVERAGEIFS(%s;%s)" % (values, ";".join(parts))
            if base == "count":
                return "COUNTIFS(%s)" % ";".join(parts)
            raise FormulaError("%s over a filter" % name)
        b = builtins.find(name)
        if b is None:
            raise FormulaError("function %s has no Calc equivalent" % name)
        _, calc, kinds = b
        parts = [self.expr(a, row, builtins.range_arg(kinds, i)) for i, a in enumerate(args)]
        if calc == "ROUND" and len(parts) == 1:
            parts.append("0")
        if calc == "XLOOKUP" and len(parts) in (3, 4):
            # XLOOKUP only exists in Calc 24.8 and later; this works everywhere.
            found = "INDEX(%s;MATCH(%s;%s;0))" % (parts[2], parts[0], parts[1])
            return "IFNA(%s;%s)" % (found, parts[3]) if len(parts) == 4 else found
        return "%s(%s)" % (calc, ";".join(parts))


def to_calc(src, book, here, row):
    """hardcalc formula of data row `row` (0-based) in table `here` → Calc formula with `=`."""
    e = parse_formula(src)
    return "=" + _ToCalc(book, here).expr(e, row, False)


# ------------------------------------------------------------ Calc → hardcalc

_SHEET = r"(?:\$?'((?:[^']|'')*)'\.|\$?([^\W\d]\w*)\.)?"
_CELL = r"(\$?)([A-Z]{1,3})(?:(\$?)(\d+))?"
_REF = re.compile(_SHEET + _CELL + r"(?::" + _SHEET + _CELL + r")?")


def table_prefix(name):
    if re.match(r"^[^\W\d]\w*$", name):
        return name + "!"
    return '"%s"!' % name


def _hc_cell(col, col_abs, row, row_abs):
    return "%s%s%s%d" % ("$" if col_abs else "", col_letters(col), "$" if row_abs else "", row + 1)


class _FromCalc(object):
    def __init__(self, book, here):
        self.book = book
        self.here = here

    def sheet(self, quoted, plain):
        name = quoted.replace("''", "'") if quoted is not None else plain
        if name is None:
            return self.here
        t = self.book.by_sheet(name)
        if t is None:
            raise FormulaError("unknown sheet '%s'" % name)
        return t

    def ref(self, m, in_function):
        g = m.groups()
        t = self.sheet(g[0], g[1])
        info = self.book.tables[t]
        if info.name is None and t != self.here:
            raise FormulaError("a table without a name cannot be referenced")
        prefix = "" if t == self.here else table_prefix(info.name)
        c_abs, letters, r_abs, digits = g[2], g[3], g[4], g[5]
        col = letters_to_col(letters)
        has_second = g[9] is not None
        if not has_second:
            if digits is None:
                raise FormulaError("bad reference")
            row = int(digits) - 2
            if row < 0 or row >= info.rows or col >= len(info.columns):
                raise FormulaError("%s%s is outside the table" % (letters, digits))
            return prefix + _hc_cell(col, bool(c_abs), row, bool(r_abs))
        if g[6] is not None or g[7] is not None:
            if self.sheet(g[6], g[7]) != t:
                raise FormulaError("range across sheets")
        c2_abs, letters2, r2_abs, digits2 = g[8], g[9], g[10], g[11]
        col2 = letters_to_col(letters2)
        lo_c, hi_c = min(col, col2), max(col, col2)
        if hi_c >= len(info.columns):
            raise FormulaError("range outside the table")
        if digits is None and digits2 is None:
            if col != col2:
                raise FormulaError("only single whole columns are supported")
            return prefix + col_letters(col)
        if digits is None or digits2 is None:
            raise FormulaError("mixed range")
        r1, r2 = int(digits) - 2, int(digits2) - 2
        lo, hi = max(min(r1, r2), 0), min(max(r1, r2), info.rows - 1)
        if info.rows == 0 or lo > hi:
            raise FormulaError("range holds no data")
        # The whole data of one column (also an open end below it) is the column.
        if in_function and lo_c == hi_c and min(r1, r2) <= 0 and max(r1, r2) >= info.rows - 1:
            return prefix + col_letters(lo_c)
        return "%s%s:%s" % (prefix, _hc_cell(lo_c, bool(c_abs), lo, bool(r_abs)),
                            _hc_cell(hi_c, bool(c2_abs), hi, bool(r2_abs)))

    def translate(self, f):
        s = f[1:] if f.startswith("=") else f
        out = []
        stack = []
        i, n = 0, len(s)
        while i < n:
            c = s[i]
            if c == '"':
                j = i + 1
                while j < n:
                    if s[j] == '"' and j + 1 < n and s[j + 1] == '"':
                        j += 2
                    elif s[j] == '"':
                        j += 1
                        break
                    else:
                        j += 1
                out.append(s[i:j])
                i = j
                continue
            if c == "'" or c == "$" or c.isalpha() or c == "_":
                m = _REF.match(s, i)
                if m:
                    end = m.end()
                    nxt = s[end] if end < n else ""
                    if not (nxt == "(" or nxt.isalnum() or (nxt and nxt in "_.")) and (m.group(6) is not None or m.group(10) is not None):
                        out.append(self.ref(m, any(stack)))
                        i = end
                        continue
                j = i
                while j < n and (s[j].isalnum() or s[j] in "_."):
                    j += 1
                word = s[i:j]
                if j < n and s[j] == "(":
                    upper = word.upper()
                    for p in ("_XLFN.", "COM.MICROSOFT.", "ORG.OPENOFFICE.", "ORG.LIBREOFFICE."):
                        if upper.startswith(p):
                            upper = upper[len(p):]
                    if upper in ("TRUE", "FALSE") and s[j:j + 2] == "()":
                        out.append(upper.lower())
                        i = j + 2
                        continue
                    if upper not in builtins.FROM_CALC:
                        raise FormulaError("function %s has no hardcalc equivalent" % word)
                    out.append(builtins.FROM_CALC[upper])
                    stack.append(True)
                    out.append("(")
                    i = j + 1
                    continue
                if word.upper() in ("TRUE", "FALSE"):
                    out.append(word.lower())
                    i = j
                    continue
                raise FormulaError("unknown name '%s'" % word)
            if c.isdigit() or (c == "." and i + 1 < n and s[i + 1].isdigit()):
                j = i
                while j < n and (s[j].isdigit() or s[j] == "." or s[j] in "eE"):
                    if s[j] in "eE" and j + 1 < n and s[j + 1] in "+-":
                        j += 1
                    j += 1
                if j < n and s[j] == ":":
                    raise FormulaError("row ranges are not supported")
                out.append(s[i:j])
                i = j
                continue
            if c == ";":
                # hardcalc separates arguments by `;`, as Calc does.
                out.append(";")
            elif c == "(":
                stack.append(False)
                out.append(c)
            elif c == ")":
                if stack:
                    stack.pop()
                out.append(c)
            elif c in "{}#~!@[]\\":
                raise FormulaError("'%s' is not supported" % c)
            else:
                out.append(c)
            i += 1
        result = "".join(out)
        parse_formula(result)
        return result


def from_calc(formula, book, here):
    """Calc formula (API grammar) of a cell in table `here` → hardcalc formula with `=`."""
    return "=" + _FromCalc(book, here).translate(formula)
