"""UNO components of the hardcalc extension for LibreOffice Calc.

- HardcalcFilter: import and export filter (XFilter, XImporter, XExporter)
- HardcalcDetect: type detection (XExtendedFilterDetection) – accepts
  `*.hcmd` and `.md` files with hardcalc tables or
  blocks, so ordinary `.md` files keep opening in Writer.

The conversion itself lives in pythonpath/hardcalc_lo (no UNO there).
"""

import base64
import json
import os
import sys
import tempfile
import traceback
import zlib

import uno
import unohelper
from com.sun.star.document import XExporter, XExtendedFilterDetection, XFilter, XImporter
from com.sun.star.lang import XInitialization, XServiceInfo

try:
    from hardcalc_lo import convert, detect, styles
except Exception:
    # Without this the component just fails to load and LibreOffice falls
    # back to Writer silently; the log tells why.
    with open(os.path.join(tempfile.gettempdir(), "hardcalc-libreoffice.log"), "a", encoding="utf-8") as _f:
        _f.write("hardcalc: module not loadable (Python %s)\n%s\n" % (sys.version.split()[0], traceback.format_exc()))
    raise

FILTER_IMPL = "org.hardcalc.libreoffice.calc.MarkdownFilter"
DETECT_IMPL = "org.hardcalc.libreoffice.calc.MarkdownDetect"
TYPE_NAME = "hardcalc_Markdown"
STATE_PROPERTY = "hardcalc"

# com.sun.star.sheet.CellFlags.FORMULA
FORMULA = 16
# com.sun.star.util.NumberFormat
NF_DATE, NF_TIME, NF_LOGICAL = 2, 4, 1024
# com.sun.star.beans.PropertyAttribute.REMOVABLE
REMOVABLE = 128

def log(message):
    """Errors go to stderr and %TEMP%/hardcalc-libreoffice.log (for troubleshooting)."""
    sys.stderr.write(message + "\n")
    try:
        with open(os.path.join(tempfile.gettempdir(), "hardcalc-libreoffice.log"), "a", encoding="utf-8") as f:
            f.write(message + "\n")
    except Exception:
        pass


def props(descriptor):
    return dict((p.Name, p.Value) for p in descriptor)


def read_stream(stream, limit=None):
    chunks, total = [], 0
    while limit is None or total < limit:
        n, data = stream.readBytes(None, 65536)
        if n <= 0:
            break
        chunks.append(bytes(data.value))
        total += n
    return b"".join(chunks)


def file_access(ctx):
    return ctx.ServiceManager.createInstanceWithContext("com.sun.star.ucb.SimpleFileAccess", ctx)


# ------------------------------------------------------------ state


def save_state(doc, state):
    data = base64.b64encode(zlib.compress(json.dumps(state).encode("utf-8"))).decode("ascii")
    container = doc.getDocumentProperties().getUserDefinedProperties()
    info = container.getPropertySetInfo()
    if info.hasPropertyByName(STATE_PROPERTY):
        container.setPropertyValue(STATE_PROPERTY, data)
    else:
        container.addProperty(STATE_PROPERTY, REMOVABLE, data)


def load_state(doc):
    try:
        container = doc.getDocumentProperties().getUserDefinedProperties()
        if not container.getPropertySetInfo().hasPropertyByName(STATE_PROPERTY):
            return None
        data = container.getPropertyValue(STATE_PROPERTY)
        return json.loads(zlib.decompress(base64.b64decode(data)).decode("utf-8"))
    except Exception:
        log("hardcalc: state not readable\n" + traceback.format_exc())
        return None


# ------------------------------------------------------------ formatting


def apply_part(rng, part):
    """One part of a format rule (`bold`, `fg red`, `align right` …) on a cell range."""
    words = part.split()
    w, arg = words[0], (words[1] if len(words) > 1 else None)
    if w in ("bold", "nobold"):
        rng.CharWeight = 150.0 if w == "bold" else 100.0
    elif w in ("italic", "noitalic"):
        rng.CharPosture = uno.Enum("com.sun.star.awt.FontSlant", "ITALIC" if w == "italic" else "NONE")
    elif w in ("underline", "nounderline"):
        rng.CharUnderline = 1 if w == "underline" else 0
    elif w == "fg" and arg is not None:
        value = -1 if arg == "none" else styles.color_value(arg)
        if value is not None:
            rng.CharColor = value
    elif w == "bg" and arg is not None:
        value = -1 if arg == "none" else styles.color_value(arg)
        if value is not None:
            rng.CellBackColor = value
            rng.IsCellBackgroundTransparent = value == -1
    elif w == "align" and arg in styles.ALIGNS:
        rng.HoriJustify = uno.Enum("com.sun.star.table.CellHoriJustify", arg.upper())


def read_style(ranges):
    """The text style of a range with one formatting, as styles.DEFAULT."""
    def get(name, default=None):
        try:
            return ranges.getPropertyValue(name)
        except Exception:
            return default
    bold = (get("CharWeight", 100.0) or 100.0) > 100.0
    posture = get("CharPosture")
    italic = posture is not None and getattr(posture, "value", "NONE") not in ("NONE", "DONTKNOW")
    underline = (get("CharUnderline", 0) or 0) != 0
    fg = get("CharColor", -1)
    fg = None if fg is None or fg == -1 or fg == 0xFFFFFFFF else fg & 0xFFFFFF
    bg = get("CellBackColor", -1)
    if bg is None or bg == -1 or bg == 0xFFFFFFFF or get("IsCellBackgroundTransparent", False):
        bg = None
    else:
        bg &= 0xFFFFFF
    justify = getattr(get("HoriJustify"), "value", "STANDARD")
    align = justify.lower() if justify in ("LEFT", "CENTER", "RIGHT") else None
    return (bold, italic, underline, fg, bg, align)


# ------------------------------------------------------------ import


def number_format(doc, code):
    formats = doc.getNumberFormats()
    locale = uno.createUnoStruct("com.sun.star.lang.Locale")
    locale.Language, locale.Country = "en", "US"
    key = formats.queryKey(code, locale, False)
    if key == -1:
        key = formats.addNew(code, locale)
    return key


def fill_sheet(doc, index, sheet, plan):
    ncols = len(plan.header)
    if ncols == 0:
        return
    nrows = 1 + len(plan.cells)

    def value(cell):
        if cell is None or cell[0] == "f":
            return ""
        if cell[0] in ("n", "d"):
            return float(cell[1])
        return cell[1]

    data = [tuple(plan.header)]
    for row in plan.cells:
        data.append(tuple(value(c) for c in row))
    sheet.getCellRangeByPosition(0, 0, ncols - 1, nrows - 1).setDataArray(tuple(data))

    # Formulas column by column, a run of formula cells at a time.
    for c in range(ncols):
        r = 0
        while r < len(plan.cells):
            cell = plan.cells[r][c]
            if cell is None or cell[0] != "f":
                r += 1
                continue
            start = r
            while r < len(plan.cells) and plan.cells[r][c] is not None and plan.cells[r][c][0] == "f":
                r += 1
            rng = sheet.getCellRangeByPosition(c, start + 1, c, r)
            rng.setFormulaArray(tuple((plan.cells[k][c][1],) for k in range(start, r)))

    header = sheet.getCellRangeByPosition(0, 0, ncols - 1, 0)
    header.CharWeight = 150.0  # bold
    if nrows > 1:
        for c, d in plan.decimals.items():
            code = "0." + "0" * d if d else "0"
            sheet.getCellRangeByPosition(c, 1, c, nrows - 1).NumberFormat = number_format(doc, code)
        for c in plan.dates:
            sheet.getCellRangeByPosition(c, 1, c, nrows - 1).NumberFormat = number_format(doc, "YYYY-MM-DD")
    for r0, r1, c0, c1, parts in plan.format_ops:
        rng = sheet.getCellRangeByPosition(c0, r0 + 1, c1, r1 + 1)
        for part in parts:
            apply_part(rng, part)
    annotations = sheet.getAnnotations()
    for r, c, text in plan.notes:
        address = uno.createUnoStruct("com.sun.star.table.CellAddress")
        address.Sheet, address.Column, address.Row = index, c, r
        annotations.insertNew(address, text)
    sheet.getCellRangeByPosition(0, 0, ncols - 1, nrows - 1).getColumns().OptimalWidth = True


def import_document(doc, data):
    src = data.decode("utf-8-sig", errors="replace")
    plans, state = convert.plan_import(src)
    sheets = doc.getSheets()
    doc.lockControllers()
    doc.addActionLock()
    try:
        doc.enableAutomaticCalculation(False)
        old = list(sheets.getElementNames())
        if plans:
            # The new document's own sheets make way (their names may be taken).
            for k, name in enumerate(old):
                sheets.getByName(name).setName("__hardcalc_%d" % k)
            for k, plan in enumerate(plans):
                sheets.insertNewByName(plan.sheet, k)
            for k in range(len(old)):
                sheets.removeByName("__hardcalc_%d" % k)
            for k, plan in enumerate(plans):
                fill_sheet(doc, k, sheets.getByIndex(k), plan)
        doc.enableAutomaticCalculation(True)
        doc.calculateAll()
        # Remember the formulas as Calc writes them, to recognise unchanged ones.
        for k, plan in enumerate(plans):
            if not plan.formulas:
                continue
            sheet = sheets.getByIndex(k)
            rng = sheet.getCellRangeByPosition(0, 0, len(plan.header) - 1, len(plan.cells))
            formulas = rng.getFormulaArray()
            known = state["sheets"][k]["formulas"]
            for pos, hc in plan.formulas.items():
                r, c = (int(x) for x in pos.split(","))
                known[pos] = [formulas[r][c], hc]
        save_state(doc, state)
    finally:
        doc.removeActionLock()
        doc.unlockControllers()
    doc.setModified(False)


# ------------------------------------------------------------ export


def sheet_data(doc, sheet):
    if not sheet.queryContentCells(1 | 2 | 4 | 16).getCount():  # value, date/time, string, formula
        return convert.SheetData(sheet.getName(), [])
    cursor = sheet.createCursor()
    cursor.gotoEndOfUsedArea(False)
    end = cursor.getRangeAddress()
    rng = sheet.getCellRangeByPosition(0, 0, end.EndColumn, end.EndRow)
    values = [list(row) for row in rng.getDataArray()]
    formulas = {}
    cells = rng.queryContentCells(FORMULA)
    if cells.getCount():
        array = rng.getFormulaArray()
        for a in cells.getRangeAddresses():
            for r in range(a.StartRow, a.EndRow + 1):
                for c in range(a.StartColumn, a.EndColumn + 1):
                    formulas[(r, c)] = array[r][c]
    kinds = {}
    nf = doc.getNumberFormats()
    unique = rng.getUniqueCellFormatRanges()
    grid = [[styles.DEFAULT] * len(values[0]) for _ in values] if values else []
    for k in range(unique.getCount()):
        ranges = unique.getByIndex(k)
        style = read_style(ranges)
        if style != styles.DEFAULT:
            for a in ranges.getRangeAddresses():
                for r in range(a.StartRow, min(a.EndRow, end.EndRow) + 1):
                    for c in range(a.StartColumn, min(a.EndColumn, end.EndColumn) + 1):
                        grid[r][c] = style
        try:
            t = nf.getByKey(ranges.NumberFormat).Type
        except Exception:
            continue
        if t & NF_LOGICAL:
            kind = "bool"
        elif t & NF_DATE and t & NF_TIME:
            kind = "datetime"
        elif t & NF_DATE:
            kind = "date"
        elif t & NF_TIME:
            kind = "time"
        else:
            continue
        for a in ranges.getRangeAddresses():
            for r in range(a.StartRow, min(a.EndRow, end.EndRow) + 1):
                for c in range(a.StartColumn, min(a.EndColumn, end.EndColumn) + 1):
                    if isinstance(values[r][c], float):
                        kinds[(r, c)] = kind
    return convert.SheetData(sheet.getName(), values, formulas, kinds, grid)


def export_document(doc):
    state = load_state(doc)
    sheets = doc.getSheets()
    data = [sheet_data(doc, sheets.getByIndex(k)) for k in range(sheets.getCount())]
    text = convert.export(data, state)
    # The written file is the new original: later saves compare with it.
    try:
        plans, new_state = convert.plan_import(text)
        by_sheet = dict((d.name, d) for d in data)
        for entry, plan in zip(new_state["sheets"], plans):
            d = by_sheet.get(entry["sheet"])
            if d is None:
                continue
            for pos in plan.formulas:
                r, c = (int(x) for x in pos.split(","))
                if (r, c) in d.formulas:
                    entry["formulas"][pos] = [d.formulas[(r, c)], plan.formulas[pos]]
        was = doc.isModified()
        save_state(doc, new_state)
        if not was:
            doc.setModified(False)
    except Exception:
        log("hardcalc: state not updated\n" + traceback.format_exc())
    return text


# ------------------------------------------------------------ components


class HardcalcFilter(unohelper.Base, XFilter, XImporter, XExporter, XInitialization, XServiceInfo):
    def __init__(self, ctx):
        self.ctx = ctx
        self.doc = None
        self.mode = None

    # XInitialization
    def initialize(self, args):
        pass

    # XImporter / XExporter
    def setTargetDocument(self, doc):
        self.doc, self.mode = doc, "import"

    def setSourceDocument(self, doc):
        self.doc, self.mode = doc, "export"

    # XFilter
    def filter(self, descriptor):
        try:
            p = props(descriptor)
            if self.mode == "import":
                stream = p.get("InputStream")
                if stream is None:
                    stream = file_access(self.ctx).openFileRead(p["URL"])
                import_document(self.doc, read_stream(stream))
            else:
                data = export_document(self.doc).encode("utf-8")
                stream = p.get("OutputStream")
                if stream is None:
                    sfa = file_access(self.ctx)
                    if sfa.exists(p["URL"]):
                        sfa.kill(p["URL"])
                    stream = sfa.openFileWrite(p["URL"])
                stream.writeBytes(uno.ByteSequence(data))
                stream.flush()
                stream.closeOutput()
            return True
        except Exception:
            log("hardcalc %s failed:\n%s" % (self.mode, traceback.format_exc()))
            return False

    def cancel(self):
        pass

    # XServiceInfo
    def getImplementationName(self):
        return FILTER_IMPL

    def supportsService(self, name):
        return name in self.getSupportedServiceNames()

    def getSupportedServiceNames(self):
        # The implementation name as service name too: the configuration refers
        # to it, and it can be created by either.
        return ("com.sun.star.document.ImportFilter", "com.sun.star.document.ExportFilter", FILTER_IMPL)


class HardcalcDetect(unohelper.Base, XExtendedFilterDetection, XServiceInfo):
    def __init__(self, ctx):
        self.ctx = ctx

    def detect(self, descriptor):
        try:
            p = props(descriptor)
            if detect.by_name(p.get("URL", "")):
                return TYPE_NAME, descriptor
            stream = p.get("InputStream")
            if stream is None:
                return "", descriptor
            head = read_stream(stream, 256 * 1024)
            try:
                stream.seek(0)
            except Exception:
                pass
            if detect.by_content(head.decode("utf-8", errors="replace")):
                return TYPE_NAME, descriptor
        except Exception:
            log("hardcalc detection failed:\n" + traceback.format_exc())
        return "", descriptor

    def getImplementationName(self):
        return DETECT_IMPL

    def supportsService(self, name):
        return name in self.getSupportedServiceNames()

    def getSupportedServiceNames(self):
        return ("com.sun.star.document.ExtendedTypeDetection", DETECT_IMPL)


g_ImplementationHelper = unohelper.ImplementationHelper()
g_ImplementationHelper.addImplementation(HardcalcFilter, FILTER_IMPL, HardcalcFilter(None).getSupportedServiceNames())
g_ImplementationHelper.addImplementation(HardcalcDetect, DETECT_IMPL, HardcalcDetect(None).getSupportedServiceNames())
