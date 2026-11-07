"""Text formatting between the `format` layer and Calc, without UNO.

Calc handles bold, italic, underline, text color, fill color and the
horizontal alignment. On opening, the rules of the format layer are applied
to cell ranges in order (later rules win, as in hardcalc). On saving, the
formatting of the cells replaces those parts of the layer: whole columns
become column rules (`B: bold`), whole rows row rules (`3: italic`), the
rest cells and rectangles (`B2: fg red`, `A1:C2: bg yellow`).

Kept as they are, since Calc has no plain cell formatting for them: rules
with a condition (`row[Status=open]: …`) and the number formats
`percent(n)` / `decimals(n)`.
"""

import re

from .formula import col_letters, letters_to_col

# hardcalc color names → RGB as Calc shows them (and back).
PALETTE = {
    "black": 0x000000,
    "white": 0xFFFFFF,
    "red": 0xFF0000,
    "green": 0x00B050,
    "yellow": 0xFFFF00,
    "blue": 0x0070C0,
    "magenta": 0xFF00FF,
    "cyan": 0x00FFFF,
    "gray": 0x808080,
    "darkgray": 0x404040,
    "lightred": 0xFF8080,
    "lightgreen": 0x90EE90,
    "lightyellow": 0xFFFFE0,
    "lightblue": 0xADD8E6,
    "lightmagenta": 0xFFB0FF,
    "lightcyan": 0xE0FFFF,
}
ALIASES = {"grey": "gray", "darkgrey": "darkgray"}
_NAMES = dict((v, k) for k, v in PALETTE.items())

# A cell's style: (bold, italic, underline, fg, bg, align); colors as int RGB.
DEFAULT = (False, False, False, None, None, None)
BOLD, ITALIC, UNDERLINE, FG, BG, ALIGN = range(6)
FAMILIES = ("bold", "italic", "underline", "fg", "bg", "align")
ALIGNS = ("left", "center", "right")


def color_value(text):
    """`red` / `#ff0000` → 0xFF0000; None if unknown."""
    t = text.strip().lower()
    t = ALIASES.get(t, t)
    if t in PALETTE:
        return PALETTE[t]
    if re.match(r"^#[0-9a-f]{6}$", t):
        return int(t[1:], 16)
    return None


def color_text(value):
    return _NAMES.get(value, "#%06x" % value)


def parts(body):
    """`fg red bold align right` → ["fg red", "bold", "align right"]."""
    out = []
    words = body.split()
    i = 0
    while i < len(words):
        w = words[i]
        if w in ("align", "fg", "bg") and i + 1 < len(words):
            out.append("%s %s" % (w, words[i + 1]))
            i += 2
        else:
            out.append(w)
            i += 1
    return out


def family(part):
    first = part.split()[0] if part.split() else ""
    if first.startswith("no") and first[2:] in ("bold", "italic", "underline"):
        return first[2:]
    return first


def calc_part(part):
    """Whether Calc carries this part of a rule."""
    return family(part) in FAMILIES


def apply_part(style, part):
    """The style after one rule part (unknown parts change nothing)."""
    s = list(style)
    words = part.split()
    w = words[0] if words else ""
    arg = words[1] if len(words) > 1 else None
    if w in ("bold", "italic", "underline"):
        s[FAMILIES.index(w)] = True
    elif w in ("nobold", "noitalic", "nounderline"):
        s[FAMILIES.index(w[2:])] = False
    elif w in ("fg", "bg") and arg is not None:
        if arg == "none":
            s[FAMILIES.index(w)] = None
        else:
            v = color_value(arg)
            if v is not None:
                s[FAMILIES.index(w)] = v
    elif w == "align" and arg in ALIGNS:
        s[ALIGN] = arg
    return tuple(s)


def _a1(s):
    m = re.match(r"^([A-Z]+)([0-9]+)$", s)
    if not m or int(m.group(2)) < 1:
        return None
    return int(m.group(2)) - 1, letters_to_col(m.group(1))


def region(target, columns, rows):
    """Data cells of a rule target as (r0, r1, c0, c1), clipped; "cond" for
    `row[…]`; None if it names nothing in the table."""
    t = target.strip()
    ncols = len(columns)
    if t.startswith("row[") and t.endswith("]"):
        return "cond"
    if rows == 0 or ncols == 0:
        return None
    if t == "*":
        return 0, rows - 1, 0, ncols - 1
    if t.isdigit():
        r = int(t) - 1
        return (r, r, 0, ncols - 1) if 0 <= r < rows else None
    if ":" in t:
        a, b = (_a1(x.strip()) for x in t.split(":", 1))
        if a and b:
            r0, r1 = sorted((a[0], b[0]))
            c0, c1 = sorted((a[1], b[1]))
            r1, c1 = min(r1, rows - 1), min(c1, ncols - 1)
            return (r0, r1, c0, c1) if r0 <= r1 and c0 <= c1 else None
    cell = _a1(t)
    if cell:
        r, c = cell
        return (r, r, c, c) if r < rows and c < ncols else None
    if t in columns:
        c = columns.index(t)
    elif re.match(r"^[A-Z]+$", t) and letters_to_col(t) < ncols:
        c = letters_to_col(t)
    else:
        return None
    return 0, rows - 1, c, c


def operations(rules, columns, rows):
    """(r0, r1, c0, c1, parts) to apply in order, Calc parts only."""
    out = []
    for target, body in rules:
        reg = region(target, columns, rows)
        if reg is None or reg == "cond":
            continue
        ps = [p for p in parts(body) if calc_part(p)]
        if ps:
            out.append(reg + (ps,))
    return out


def grid_of(rules, columns, rows):
    """The style of every data cell that `rules` give."""
    grid = [[DEFAULT] * len(columns) for _ in range(rows)]
    for r0, r1, c0, c1, ps in operations(rules, columns, rows):
        for r in range(r0, r1 + 1):
            row = grid[r]
            for c in range(c0, c1 + 1):
                st = row[c]
                for p in ps:
                    st = apply_part(st, p)
                row[c] = st
    return grid


def _token(attr, value):
    name = FAMILIES[attr]
    if attr in (BOLD, ITALIC, UNDERLINE):
        return name if value else "no" + name
    if attr in (FG, BG):
        return "%s %s" % (name, "none" if value is None else color_text(value))
    return None if value is None else "align %s" % value


def rules_of(grid, ncols):
    """Short rules that give `grid`: column rules, then row rules, then cells
    and rectangles. Returns [(target, body)]."""
    rows = len(grid)
    by_target = {}
    order = []

    def add(target, attr, value):
        token = _token(attr, value)
        if token is None:
            return
        if target not in by_target:
            by_target[target] = {}
            order.append(target)
        by_target[target][attr] = token

    if rows == 0 or ncols == 0:
        return []
    for attr in range(6):
        expected = [[DEFAULT[attr]] * ncols for _ in range(rows)]
        # A column rule for the value most of the column has (the rest are
        # exceptions, like `B: bold` with `B2: nobold` from the terminal).
        for c in range(ncols):
            counts = {}
            for r in range(rows):
                v = grid[r][c][attr]
                counts[v] = counts.get(v, 0) + 1
            v, n = max(counts.items(), key=lambda kv: (kv[1], kv[0] != DEFAULT[attr]))
            if v != DEFAULT[attr] and (n == rows or (n >= 2 and 2 * n > rows)):
                add(col_letters(c), attr, v)
                for r in range(rows):
                    expected[r][c] = v
        if ncols > 1:
            for r in range(rows):
                values = set(grid[r][c][attr] for c in range(ncols))
                if len(values) == 1:
                    v = values.pop()
                    if sum(1 for c in range(ncols) if expected[r][c] != v) >= 2:
                        add(str(r + 1), attr, v)
                        expected[r] = [v] * ncols
        need = [[grid[r][c][attr] != expected[r][c] for c in range(ncols)] for r in range(rows)]
        for r in range(rows):
            for c in range(ncols):
                if not need[r][c]:
                    continue
                v = grid[r][c][attr]
                c2 = c
                while c2 + 1 < ncols and need[r][c2 + 1] and grid[r][c2 + 1][attr] == v:
                    c2 += 1
                r2 = r
                while r2 + 1 < rows and all(need[r2 + 1][k] and grid[r2 + 1][k][attr] == v for k in range(c, c2 + 1)):
                    r2 += 1
                for rr in range(r, r2 + 1):
                    for k in range(c, c2 + 1):
                        need[rr][k] = False
                a = "%s%d" % (col_letters(c), r + 1)
                target = a if (r, c) == (r2, c2) else "%s:%s%d" % (a, col_letters(c2), r2 + 1)
                add(target, attr, v)

    def key(target):
        if re.match(r"^[A-Z]+$", target):
            return (0, letters_to_col(target), 0)
        if target.isdigit():
            return (1, int(target), 0)
        first = _a1(target.split(":")[0])
        return (2, first[0], first[1])

    order.sort(key=key)
    # Within a rule: colors, then bold/italic/underline, then alignment.
    show = (FG, BG, BOLD, ITALIC, UNDERLINE, ALIGN)
    return [(t, " ".join(by_target[t][a] for a in show if a in by_target[t])) for t in order]


def new_layer(old_rules, grid, columns):
    """The rules of the format layer after saving: what Calc cannot carry
    from `old_rules`, plus rules for `grid`. None if the formatting is what
    `old_rules` give already (the block then stays as it is)."""
    rows = len(grid)
    if grid_of(old_rules, columns, rows) == grid:
        return None
    kept, conditional = [], []
    for target, body in old_rules:
        if region(target, columns, rows) == "cond":
            conditional.append((target, body))
            continue
        rest = [p for p in parts(body) if not calc_part(p)]
        if rest:
            kept.append((target, " ".join(rest)))
    # One rule per target: the new style first, then what was kept.
    merged, order = {}, []
    for target, body in rules_of(grid, len(columns)) + kept:
        if target in merged:
            merged[target] = merged[target] + " " + body
        else:
            merged[target] = body
            order.append(target)
    return [(t, merged[t]) for t in order] + conditional
