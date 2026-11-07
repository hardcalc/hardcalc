"""hardcalc formulas: lexer and parser (a port of src/formula.rs).

The syntax tree is made of tuples:

    ("num", 1.5)            ("qty", 5.0, "m/s")     ("str", "text")
    ("bool", True)          ("name", "B2")          ("range", "B2", "4")
    ("member", "T", "Col")  ("layer", expr, sel)    ("index", "T", [args])
    ("remote", "T", expr)   ("at", expr)            ("neg", expr)
    ("pct", expr)           ("bin", "+", l, r)      ("call", "sum", [args])
"""

import re


class FormulaError(Exception):
    pass


_TWO = {"<=": "<=", ">=": ">=", "<>": "<>", "!=": "<>"}
_ONE = {
    "+": "+", "-": "-", "*": "*", "/": "/", "^": "^", "&": "&", "%": "%",
    "=": "=", "<": "<", ">": ">", "(": "(", ")": ")", "[": "[", "]": "]",
    ",": ",", ";": ",", ":": ":", ".": ".", "!": "!", "@": "@",
}


def _ident_char(c):
    return c.isalnum() or c == "_" or c == "$"


def lex(src):
    toks = []
    i, n = 0, len(src)
    while i < n:
        c = src[i]
        if c.isspace():
            i += 1
        elif c.isdigit() and toks and toks[-1] == ("op", ":"):
            # The end of a short range: B2:4, B2:4c.
            start = i
            while i < n and (src[i].isascii() and src[i].isalnum() or src[i] == "$"):
                i += 1
            toks.append(("id", src[start:i]))
        elif c.isdigit():
            start = i
            while i < n and (src[i].isdigit() or src[i] == "."):
                i += 1
            if i < n and src[i] in "eE":
                j = i + 1
                if j < n and src[j] in "+-":
                    j += 1
                if j < n and src[j].isdigit():
                    i = j
                    while i < n and src[i].isdigit():
                        i += 1
            text = src[start:i]
            try:
                toks.append(("num", float(text)))
            except ValueError:
                raise FormulaError("bad number '%s'" % text)
            j = i
            while j < n and src[j] == " ":
                j += 1
            if j < n and src[j] == "{":
                end = src.find("}", j)
                if end < 0:
                    raise FormulaError("missing '}' after unit")
                toks.append(("unit", src[j + 1:end].strip()))
                i = end + 1
        elif c == '"':
            out = []
            i += 1
            while True:
                if i >= n:
                    raise FormulaError("unterminated string")
                if src[i] == '"' and i + 1 < n and src[i + 1] == '"':
                    out.append('"')
                    i += 2
                elif src[i] == '"':
                    i += 1
                    break
                else:
                    out.append(src[i])
                    i += 1
            toks.append(("str", "".join(out)))
        elif c.isalpha() or c == "_" or c == "$":
            start = i
            while i < n and _ident_char(src[i]):
                i += 1
            toks.append(("id", src[start:i]))
        else:
            two = src[i:i + 2]
            if two in _TWO:
                toks.append(("op", _TWO[two]))
                i += 2
                continue
            if c not in _ONE:
                raise FormulaError("unexpected character '%s'" % c)
            toks.append(("op", _ONE[c]))
            i += 1
    return toks


def parse_formula(src):
    src = src.strip()
    if src.startswith("="):
        src = src[1:]
    p = _Parser(lex(src))
    e = p.expr()
    if p.pos < len(p.toks):
        raise FormulaError("unexpected %r" % (p.toks[p.pos],))
    return e


class _Parser(object):
    def __init__(self, toks):
        self.toks = toks
        self.pos = 0

    def tok(self, k=0):
        i = self.pos + k
        return self.toks[i] if i < len(self.toks) else None

    def peek_op(self, op):
        return self.tok() == ("op", op)

    def eat(self, op):
        if self.peek_op(op):
            self.pos += 1
            return True
        return False

    def expect(self, op):
        if not self.eat(op):
            raise FormulaError("expected '%s'" % op)

    def ident(self):
        t = self.tok()
        if t is None or t[0] != "id":
            raise FormulaError("expected a name")
        self.pos += 1
        return t[1]

    def expr(self):
        lhs = self.concat()
        for op in ("=", "<>", "<=", ">=", "<", ">"):
            if self.eat(op):
                return ("bin", op, lhs, self.concat())
        return lhs

    def concat(self):
        lhs = self.add()
        while self.eat("&"):
            lhs = ("bin", "&", lhs, self.add())
        return lhs

    def add(self):
        lhs = self.mul()
        while True:
            if self.eat("+"):
                lhs = ("bin", "+", lhs, self.mul())
            elif self.eat("-"):
                lhs = ("bin", "-", lhs, self.mul())
            else:
                return lhs

    def mul(self):
        lhs = self.unary()
        while True:
            if self.eat("*"):
                lhs = ("bin", "*", lhs, self.unary())
            elif self.eat("/"):
                lhs = ("bin", "/", lhs, self.unary())
            else:
                return lhs

    def unary(self):
        if self.eat("-"):
            return ("neg", self.unary())
        if self.eat("+"):
            return self.unary()
        return self.power()

    def power(self):
        base = self.postfix()
        if self.eat("^"):
            return ("bin", "^", base, self.unary())
        return base

    def postfix(self):
        e = self.primary()
        while self.eat("%"):
            e = ("pct", e)
        return e

    def args(self, close):
        out = []
        if self.eat(close):
            return out
        while True:
            out.append(self.expr())
            if self.eat(close):
                return out
            self.expect(",")

    def name_or_str(self):
        t = self.tok()
        if t is not None and t[0] == "str":
            self.pos += 1
            return t[1]
        return self.ident()

    def offset_ahead(self):
        sign = {("op", "+"): 1, ("op", "-"): -1}.get(self.tok(1))
        if sign is None:
            return None
        a, b, c = self.tok(), self.tok(2), self.tok(3)
        if a == ("op", "[") and b is not None and b[0] == "num" and c == ("op", "]") and b[1] == int(b[1]):
            return sign * int(b[1])
        return None

    def layers(self, e):
        while True:
            n = self.offset_ahead()
            if n is not None:
                self.pos += 4
                e = ("layer", e, n)
            elif self.eat("."):
                e = ("layer", e, self.name_or_str())
            else:
                return e

    def range_end(self):
        t = self.tok()
        if t is not None and t[0] == "num" and t[1] == int(t[1]) and t[1] >= 1:
            self.pos += 1
            return str(int(t[1]))
        return self.ident()

    def reference(self, name):
        if self.peek_op("[") and self.offset_ahead() is None:
            self.pos += 1
            if self.eat("@"):
                col = self.ident()
                self.expect("]")
                return ("at", ("member", name, col))
            return ("index", name, self.args("]"))
        if self.eat(":"):
            return ("range", name, self.range_end())
        return ("name", name)

    def remote(self, table):
        a = self.ident()
        return ("remote", table, self.reference(a))

    def primary(self):
        t = self.tok()
        if t is None:
            raise FormulaError("unexpected end of formula")
        self.pos += 1
        kind = t[0]
        if kind == "num":
            u = self.tok()
            if u is not None and u[0] == "unit":
                self.pos += 1
                return ("qty", t[1], u[1])
            return ("num", t[1])
        if kind == "str":
            s = t[1]
            if self.eat("!"):
                return self.layers(self.remote(s))
            if self.eat("."):
                return self.layers(("member", s, self.name_or_str()))
            if self.peek_op("["):
                return self.layers(self.reference(s))
            return ("str", s)
        if t == ("op", "("):
            e = self.expr()
            self.expect(")")
            return e
        if t == ("op", "@"):
            e = self.primary()
            if e[0] in ("name", "member", "remote"):
                return ("at", e)
            if e[0] == "layer" and e[1][0] in ("name", "member", "remote"):
                return ("layer", ("at", e[1]), e[2])
            raise FormulaError("@ must be followed by a column")
        if t == ("op", "[") and self.eat("@"):
            col = self.ident()
            self.expect("]")
            return ("at", ("name", col))
        if kind == "id":
            name = t[1]
            if self.eat("!"):
                return self.layers(self.remote(name))
            if self.eat("("):
                return ("call", name.lower(), self.args(")"))
            nxt = self.tok(1)
            if self.peek_op(".") and nxt is not None and nxt[0] in ("id", "str"):
                self.pos += 1
                return self.layers(("member", name, self.name_or_str()))
            low = name.lower()
            if not self.peek_op("[") and not self.peek_op(":"):
                if low in ("true", "wahr"):
                    return ("bool", True)
                if low in ("false", "falsch"):
                    return ("bool", False)
            return self.layers(self.reference(name))
        raise FormulaError("unexpected %r" % (t,))


# ------------------------------------------------------------ references

_PART = re.compile(r"^(\$?)([A-Z]+)?(\$?)([0-9]+)?(\$?)([a-z]+)?$")


def letters_to_col(s):
    n = 0
    for ch in s:
        n = n * 26 + (ord(ch) - ord("A") + 1)
    return n - 1


def col_letters(c):
    s = ""
    while True:
        s = chr(ord("A") + c % 26) + s
        if c < 26:
            return s
        c = c // 26 - 1


def parse_ref(s):
    """`$B$2` → (col, col_abs, row, row_abs); `B` → (col, abs, None, False).

    Rows are 0-based data rows. References with a data layer (`B2b`) and
    anything else return None.
    """
    m = _PART.match(s)
    if not m or not m.group(2):
        return None
    if m.group(6) or m.group(5):
        return None
    if m.group(3) and not m.group(4):
        return None
    col = letters_to_col(m.group(2))
    if m.group(4) is None:
        return (col, bool(m.group(1)), None, False)
    row = int(m.group(4))
    if row < 1:
        return None
    return (col, bool(m.group(1)), row - 1, bool(m.group(3)))


def parse_partial(s):
    """The end of a short range: `4`, `D`, `D4`, `$D$4` → (col|None, abs, row|None, abs)."""
    m = _PART.match(s)
    if not m or m.group(6) or m.group(5):
        return None
    if not m.group(2) and not m.group(4):
        return None
    col = letters_to_col(m.group(2)) if m.group(2) else None
    row = int(m.group(4)) - 1 if m.group(4) else None
    if row is not None and row < 0:
        return None
    if col is None:
        # `$4`: the regex gives the row's `$` to the (missing) column.
        return (None, False, row, bool(m.group(1) or m.group(3)))
    return (col, bool(m.group(1)), row, bool(m.group(3)))
