"""Which files the extension takes over (the rest of `.md` stays with Writer)."""

import re

# Suffix of hardcalc documents, whatever they contain.
SUFFIXES = (".hcmd",)

_MARKERS = re.compile(
    r"^(#{1,6} [^\n]*::\s*(matrix|vector|tensor|playground)\b"
    r"|```(types|functions|hardcalc|footer|layer |slice |scenario |chart |layout ))",
    re.M,
)


def by_name(url):
    """Whether the file name alone makes it a hardcalc document."""
    path = (url or "").split("?", 1)[0].split("#", 1)[0].lower()
    return path.endswith(SUFFIXES)


def by_content(text):
    """Whether Markdown text holds hardcalc tables or blocks."""
    return _MARKERS.search(text) is not None
