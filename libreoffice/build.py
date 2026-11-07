#!/usr/bin/env python3
"""Packs the extension into dist/hardcalc-libreoffice-<version>.oxt.

    python3 build.py                       # .oxt without update check
    python3 build.py --update-url URL      # .oxt that looks for updates
                                           # at URL (an update.xml is written too)

The version comes from extension/description.xml.
"""

import argparse
import os
import re
import zipfile

HERE = os.path.dirname(os.path.abspath(__file__))
SRC = os.path.join(HERE, "extension")
DIST = os.path.join(HERE, "dist")
IDENTIFIER = "org.hardcalc.libreoffice"

UPDATE_XML = """<?xml version="1.0" encoding="UTF-8"?>
<description xmlns="http://openoffice.org/extensions/update/2006"
             xmlns:xlink="http://www.w3.org/1999/xlink">
  <identifier value="{identifier}"/>
  <version value="{version}"/>
  <update-download>
    <src xlink:href="{download}"/>
  </update-download>
</description>
"""


def version():
    with open(os.path.join(SRC, "description.xml"), encoding="utf-8") as f:
        return re.search(r'<version value="([^"]+)"', f.read()).group(1)


def build(update_url=None, download_url=None):
    v = version()
    os.makedirs(DIST, exist_ok=True)
    target = os.path.join(DIST, "hardcalc-libreoffice-%s.oxt" % v)
    with zipfile.ZipFile(target, "w", zipfile.ZIP_DEFLATED) as z:
        for root, dirs, files in os.walk(SRC):
            dirs[:] = sorted(d for d in dirs if d != "__pycache__")
            for name in sorted(files):
                if name.endswith((".pyc", ".pyo")) or name.startswith("."):
                    continue
                path = os.path.join(root, name)
                arc = os.path.relpath(path, SRC).replace(os.sep, "/")
                if arc == "description.xml" and update_url:
                    with open(path, encoding="utf-8") as f:
                        text = f.read()
                    info = ('<update-information>\n    <src xlink:href="%s"/>\n  </update-information>'
                            % update_url)
                    z.writestr(arc, text.replace("<!--UPDATE-INFORMATION-->", info))
                else:
                    z.write(path, arc)
    if update_url:
        download = download_url or update_url.rsplit("/", 1)[0] + "/" + os.path.basename(target)
        with open(os.path.join(DIST, "update.xml"), "w", encoding="utf-8") as f:
            f.write(UPDATE_XML.format(identifier=IDENTIFIER, version=v, download=download))
    return target


if __name__ == "__main__":
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--update-url", help="URL of update.xml (the .oxt then looks for updates there)")
    ap.add_argument("--download-url", help="URL of the .oxt in update.xml (default: next to update.xml)")
    a = ap.parse_args()
    print(build(a.update_url, a.download_url))
