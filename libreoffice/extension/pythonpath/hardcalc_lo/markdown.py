"""Splits a `.hcmd` document into text and table segments and writes tables
back in hardcalc's canonical form (ports of src/parse.rs and src/write.rs).

Everything that is not a table – text, fenced blocks (types, layers,
footers, charts, layouts, …) – is kept byte for byte, so a round trip
through Calc only rewrites the tables.
"""

import re
import unicodedata


class Text(object):
    def __init__(self, lines):
        self.lines = lines


class Table(object):
    def __init__(self, level, name, shape, header, aligns, rows):
        self.level = level      # heading level, None for an anonymous table
        self.name = name
        self.shape = shape      # text after `::`, e.g. `matrix[*, 5] fill`
        self.header = header    # raw header cells (`Amount:Euro`)
        self.aligns = aligns    # "", "left", "right", "center"
        self.rows = rows
        self.raw = None         # source lines (heading to last row), if read

    @property
    def playground(self):
        return (self.shape or "").split(" ")[0] == "playground"


def fence_marker(line):
    t = line.lstrip()
    if t.startswith("````"):
        return "````"
    if t.startswith("```"):
        return "```"
    return None


def table_heading(line):
    """`## Name :: shape` → (level, name, shape)."""
    level = len(line) - len(line.lstrip("#"))
    if level == 0 or level > 6:
        return None
    rest = line[level:]
    if not rest.startswith(" ") or "::" not in rest:
        return None
    name, shape = rest[1:].split("::", 1)
    return level, name.strip(), shape.strip()


def is_table_row(line):
    return line.lstrip().startswith("|")


def split_row(line):
    """`| a | b \\| c |` → ["a", "b | c"]."""
    t = line.strip()
    if t.startswith("|"):
        t = t[1:]
    cells, cur = [], []
    i = 0
    while i < len(t):
        c = t[i]
        if c == "\\" and i + 1 < len(t) and t[i + 1] == "|":
            cur.append("|")
            i += 2
            continue
        if c == "|":
            cells.append("".join(cur).strip())
            cur = []
        else:
            cur.append(c)
        i += 1
    if "".join(cur).strip():
        cells.append("".join(cur).strip())
    return cells


def is_separator(line):
    cells = split_row(line)
    if not cells:
        return False
    for c in cells:
        inner = c.strip().lstrip(":").rstrip(":")
        if not inner or set(inner) != {"-"}:
            return False
    return True


def parse_align(sep):
    s = sep.strip()
    left, right = s.startswith(":"), s.endswith(":")
    if left and right:
        return "center"
    if left:
        return "left"
    if right:
        return "right"
    return ""


def _parse_table(lines, start):
    header = split_row(lines[start])
    seps = split_row(lines[start + 1])
    aligns = [parse_align(seps[i]) if i < len(seps) else "" for i in range(len(header))]
    rows = []
    i = start + 2
    while i < len(lines) and is_table_row(lines[i]):
        cells = split_row(lines[i])
        cells = (cells + [""] * len(header))[:len(header)]
        rows.append(cells)
        i += 1
    return header, aligns, rows, i


def parse(src):
    """Text → list of Text and Table segments (lines without line ends)."""
    lines = src.split("\n")
    segments = []
    text = []
    i = 0

    def flush():
        if text:
            segments.append(Text(list(text)))
            del text[:]

    while i < len(lines):
        line = lines[i]
        fence = fence_marker(line)
        if fence:
            # The whole fenced block is text; an unclosed fence runs to the end.
            text.append(line)
            i += 1
            while i < len(lines) and not lines[i].lstrip().startswith(fence):
                text.append(lines[i])
                i += 1
            if i < len(lines):
                text.append(lines[i])
                i += 1
            continue
        heading = table_heading(line)
        if heading:
            j = i + 1
            while j < len(lines) and not lines[j].strip():
                j += 1
            if j + 1 < len(lines) and is_table_row(lines[j]) and is_separator(lines[j + 1]):
                flush()
                header, aligns, rows, nxt = _parse_table(lines, j)
                table = Table(heading[0], heading[1], heading[2], header, aligns, rows)
                table.raw = lines[i:nxt]
                segments.append(table)
                i = nxt
                continue
        if is_table_row(line) and i + 1 < len(lines) and is_separator(lines[i + 1]):
            flush()
            header, aligns, rows, nxt = _parse_table(lines, i)
            table = Table(None, None, None, header, aligns, rows)
            table.raw = lines[i:nxt]
            segments.append(table)
            i = nxt
            continue
        text.append(line)
        i += 1
    flush()
    return segments


def fences(lines):
    """Fenced blocks in text lines: [(first line, last line, info)]."""
    out = []
    i = 0
    while i < len(lines):
        fence = fence_marker(lines[i])
        if fence:
            start = i
            head = lines[i].lstrip()[len(fence):].strip()
            i += 1
            while i < len(lines) and not lines[i].lstrip().startswith(fence):
                i += 1
            out.append((start, min(i, len(lines) - 1), head))
        i += 1
    return out


_CELL = re.compile(r"^[A-Za-z]+[0-9]+$")
_RANGE_END = re.compile(r"^\s*[A-Za-z]+[0-9]+\s*:")


def layer_rules(body):
    """`target: body` lines of a layer block → [(target, body)] (split at the
    first `:` outside brackets that is not the colon of a range like `A1:B2`,
    as hardcalc does)."""
    out = []
    for line in body:
        t = line.strip()
        if not t or t.startswith("#"):
            continue
        depth = 0
        for i, ch in enumerate(t):
            if ch == "[":
                depth += 1
            elif ch == "]":
                depth -= 1
            elif ch == ":" and depth == 0:
                if _CELL.match(t[:i].strip()) and _RANGE_END.match(t[i + 1:]):
                    continue
                out.append((t[:i].strip(), t[i + 1:].strip()))
                break
    return out


def fenced_blocks(segments, info):
    """Bodies of all fenced blocks whose info string is `info`."""
    out = []
    for seg in segments:
        if not isinstance(seg, Text):
            continue
        lines = seg.lines
        i = 0
        while i < len(lines):
            fence = fence_marker(lines[i])
            if fence:
                head = lines[i].lstrip()[len(fence):].strip()
                body = []
                i += 1
                while i < len(lines) and not lines[i].lstrip().startswith(fence):
                    body.append(lines[i])
                    i += 1
                if head == info:
                    out.append(body)
            i += 1
    return out


def parse_types(segments):
    """`types` blocks → {name: definition text}."""
    types = {}
    for body in fenced_blocks(segments, "types"):
        for line in body:
            t = line.strip()
            if not t or t.startswith("#") or "=" not in t:
                continue
            name, definition = t.split("=", 1)
            types[name.strip()] = definition.strip()
    return types


_DECIMAL = re.compile(r"^decimal\((\d+)\)")


def base_type(ty, types, depth=0):
    """The base of a column type (`Euro` → `decimal(2) unit "€"`)."""
    ty = (ty or "").strip()
    word = re.split(r"[\s(]", ty, 1)[0] if ty else ""
    if word in types and depth < 8:
        return base_type(types[word], types, depth + 1)
    return ty


def split_header(cell):
    """`Amount:Euro` → ("Amount", "Euro"); `Name` → ("Name", None)."""
    if ":" in cell:
        name, ty = cell.rsplit(":", 1)
        if name.strip() and ty.strip():
            return name.strip(), ty.strip()
    return cell, None


# ------------------------------------------------------------ writing


def width(s):
    w = 0
    for ch in s:
        if unicodedata.combining(ch):
            continue
        w += 2 if unicodedata.east_asian_width(ch) in ("W", "F") else 1
    return w


def escape_cell(s):
    return s.replace("|", "\\|")


def write_table(table):
    """A table in canonical form (heading, blank line, rows), without line end."""
    header = [escape_cell(h) for h in table.header]
    rows = [[escape_cell(c) for c in r] for r in table.rows]
    ncols = len(header)
    aligns = (list(table.aligns) + [""] * ncols)[:ncols]
    # Without padding (small diffs in Git), unless the heading says `aligned`;
    # a table without a heading cannot say it.
    compact = "aligned" not in (table.shape or "").replace("[", " [").split() or table.level is None or table.name is None
    widths = []
    for i in range(ncols):
        w = max([width(header[i])] + [width(r[i]) for r in rows if i < len(r)])
        widths.append(1 if compact else max(w, 3))

    def pad(s, w, align):
        if compact:
            return s
        fill = max(w - width(s), 0)
        if align == "right":
            return " " * fill + s
        if align == "center":
            return " " * (fill // 2) + s + " " * (fill - fill // 2)
        return s + " " * fill

    def row_line(cells):
        return "| " + " | ".join(pad(c, widths[i], aligns[i]) for i, c in enumerate(cells)) + " |"

    out = []
    if table.level is not None and table.name is not None:
        out.append("%s %s :: %s" % ("#" * table.level, table.name, table.shape or "matrix"))
        out.append("")
    out.append(row_line(header))
    seps = []
    for a, w in zip(aligns, widths):
        if a == "left":
            seps.append(":" + "-" * (w + 1))
        elif a == "right":
            seps.append("-" * (w + 1) + ":")
        elif a == "center":
            seps.append(":" + "-" * w + ":")
        else:
            seps.append("-" * (w + 2))
    out.append("|" + "|".join(seps) + "|")
    for r in rows:
        out.append(row_line((list(r) + [""] * ncols)[:ncols]))
    return "\n".join(out)


def write(segments):
    parts = []
    for seg in segments:
        if isinstance(seg, Text):
            parts.append("\n".join(seg.lines))
        elif seg.raw is not None:
            parts.append("\n".join(seg.raw))
        else:
            parts.append(write_table(seg))
    return "\n".join(parts)
