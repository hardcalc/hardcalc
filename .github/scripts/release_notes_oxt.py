#!/usr/bin/env python3
"""Adds a section about the LibreOffice extension to the notes of a release.

    release_notes_oxt.py <tag> <repo>

dist writes the notes; this runs afterwards and puts the section before the
download table. Does nothing if the section is there already.
"""

import json
import subprocess
import sys

MARKER = "## LibreOffice extension"


def section(tag, repo, version):
    url = "https://github.com/%s/releases/download/%s/hardcalc-libreoffice-%s.oxt" % (repo, tag, version)
    return (
        "%s\n\n"
        "Open and save hardcalc files in LibreOffice Calc: "
        "[hardcalc-libreoffice-%s.oxt](%s). "
        "Open the file in LibreOffice (*Tools → Extensions → Add*), or, with hardcalc installed:\n\n"
        "```sh\nhardcalc libreoffice install\n```\n\n" % (MARKER, version, url)
    )


def main():
    tag, repo = sys.argv[1:3]
    version = tag.lstrip("v")
    data = json.loads(subprocess.check_output(["gh", "release", "view", tag, "--repo", repo, "--json", "body"]))
    body = data["body"] or ""
    if MARKER in body:
        return
    new = section(tag, repo, version)
    at = body.find("## Download")
    body = body[:at] + new + body[at:] if at >= 0 else body.rstrip() + "\n\n" + new
    subprocess.check_call(["gh", "release", "edit", tag, "--repo", repo, "--notes", body])


if __name__ == "__main__":
    main()
