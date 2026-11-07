"""Built-in functions with their spreadsheet names (from src/builtins.rs).

Each entry: hardcalc name → (Calc/Excel name, argument kinds). An argument
kind `r` is a range (a column means all of it), `v` a value (a column
means the cell in the formula's row); the last letter repeats.
"""

BUILTINS = {
    "sum": ("SUM", "r"),
    "avg": ("AVERAGE", "r"),
    "min": ("MIN", "r"),
    "max": ("MAX", "r"),
    "count": ("COUNT", "r"),
    "counta": ("COUNTA", "r"),
    "countblank": ("COUNTBLANK", "r"),
    "product": ("PRODUCT", "r"),
    "median": ("MEDIAN", "r"),
    "stdev": ("STDEV", "r"),
    "if": ("IF", "v"),
    "iferror": ("IFERROR", "v"),
    "and": ("AND", "v"),
    "or": ("OR", "v"),
    "not": ("NOT", "v"),
    "sumif": ("SUMIF", "rvr"),
    "countif": ("COUNTIF", "rv"),
    "averageif": ("AVERAGEIF", "rvr"),
    "sumifs": ("SUMIFS", "rrv"),
    "countifs": ("COUNTIFS", "rv"),
    "xlookup": ("XLOOKUP", "vrrv"),
    "match": ("MATCH", "vrv"),
    "index": ("INDEX", "rv"),
    "abs": ("ABS", "v"),
    "sqrt": ("SQRT", "v"),
    "round": ("ROUND", "v"),
    "roundup": ("ROUNDUP", "v"),
    "rounddown": ("ROUNDDOWN", "v"),
    "int": ("INT", "v"),
    "mod": ("MOD", "v"),
    "power": ("POWER", "v"),
    "sign": ("SIGN", "v"),
    "ceiling": ("CEILING", "v"),
    "floor": ("FLOOR", "v"),
    "exp": ("EXP", "v"),
    "ln": ("LN", "v"),
    "log": ("LOG", "v"),
    "pi": ("PI", "v"),
    "today": ("TODAY", "v"),
    "date": ("DATE", "v"),
    "year": ("YEAR", "v"),
    "month": ("MONTH", "v"),
    "day": ("DAY", "v"),
    "weekday": ("WEEKDAY", "v"),
    "len": ("LEN", "v"),
    "left": ("LEFT", "v"),
    "right": ("RIGHT", "v"),
    "mid": ("MID", "v"),
    "upper": ("UPPER", "v"),
    "lower": ("LOWER", "v"),
    "proper": ("PROPER", "v"),
    "trim": ("TRIM", "v"),
    "concat": ("CONCAT", "r"),
    "textjoin": ("TEXTJOIN", "vvr"),
    "substitute": ("SUBSTITUTE", "v"),
    "find": ("FIND", "v"),
    "search": ("SEARCH", "v"),
    "rept": ("REPT", "v"),
    "exact": ("EXACT", "v"),
    "value": ("VALUE", "v"),
    "text": ("TEXT", "v"),
}

ALIASES = {
    "average": "avg",
    "mean": "avg",
    "mittelwert": "avg",
    "summe": "sum",
    "anzahl": "count",
    "wenn": "if",
    "concatenate": "concat",
    "verketten": "concat",
    "länge": "len",
    "heute": "today",
}

# Functions hardcalc knows but cannot be written for Calc.
NOT_EXPORTED = {"spark"}

FROM_CALC = dict((calc, name) for name, (calc, _) in BUILTINS.items())
FROM_CALC["CONCATENATE"] = "concat"


def find(name):
    """(hardcalc name, Calc name, kinds) of a built-in, aliases resolved."""
    name = ALIASES.get(name, name)
    if name in BUILTINS:
        calc, kinds = BUILTINS[name]
        return name, calc, kinds
    return None


def is_builtin(name):
    return find(name) is not None or name in NOT_EXPORTED


def range_arg(kinds, i):
    return (kinds[i] if i < len(kinds) else kinds[-1]) == "r"
