#!/usr/bin/env python3
"""Draws the hardcalc logo and writes the icon files next to this script.

    python3 assets/icon/make_icons.py

The logo is the avatar of the hardcalc GitHub organisation: a dark rounded
tile, a grey corner and a yellow cell frame with a cursor. It consists of
axis-parallel rectangles only, so it is rendered exactly here (area coverage
per pixel) instead of being scaled from a bitmap. No dependencies.

Writes:
    hardcalc.svg    scalable (Linux icon theme)
    hardcalc.png    256 x 256 (Linux icon theme)
    hardcalc.ico    16 … 256 (Windows)
    hardcalc.icns   16 … 1024 with the margin of the macOS icon grid
"""

import os
import struct
import zlib

HERE = os.path.dirname(os.path.abspath(__file__))

# Geometry in the 460 x 460 units of the original avatar.
UNIT = 460.0
RADIUS = 87.0
BG = (20, 23, 28)
GREY = (58, 66, 77)
YELLOW = (242, 179, 61)
# (x0, y0, x1, y1, colour, sign): sign -1 cuts a rectangle out of the one before.
RECTS = [
    (86, 137, 137, 374, GREY, 1),     # left bar
    (137, 86, 374, 137, GREY, 1),     # top bar
    (158, 158, 352, 252, YELLOW, 1),  # cell frame …
    (172, 172, 338, 238, YELLOW, -1),  # … without its inside
    (189, 185, 205, 225, YELLOW, 1),  # cursor
]


def svg():
    parts = ['<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 460 460">',
             '<rect width="460" height="460" rx="%g" fill="#%02x%02x%02x"/>' % ((RADIUS,) + BG)]
    parts.append('<path fill="#%02x%02x%02x" d="M86 137H137V374H86ZM137 86H374V137H137Z"/>' % GREY)
    parts.append('<path fill="#%02x%02x%02x" fill-rule="evenodd" '
                 'd="M158 158H352V252H158ZM172 172V238H338V172ZM189 185H205V225H189Z"/>' % YELLOW)
    parts.append('</svg>\n')
    return "\n".join(parts)


def overlap(a0, a1, b0, b1):
    return max(0.0, min(a1, b1) - max(a0, b0))


def tile_coverage(px, py, s, lo, hi, r):
    """Share of the pixel [px, px+s) x [py, py+s) inside the rounded square lo..hi."""
    base = overlap(px, px + s, lo, hi) * overlap(py, py + s, lo, hi) / (s * s)
    if base == 0:
        return 0.0
    cx = lo + r if px < lo + r else hi - r if px + s > hi - r else None
    cy = lo + r if py < lo + r else hi - r if py + s > hi - r else None
    if cx is None or cy is None:
        return base
    n = 8
    inside = 0
    for i in range(n):
        x = px + (i + 0.5) * s / n
        for j in range(n):
            y = py + (j + 0.5) * s / n
            if not (lo <= x <= hi and lo <= y <= hi):
                continue
            dx = x - cx if (x < lo + r or x > hi - r) else 0.0
            dy = y - cy if (y < lo + r or y > hi - r) else 0.0
            if dx * dx + dy * dy <= r * r:
                inside += 1
    return inside / (n * n)


def render(size, margin=0.0):
    """RGBA rows of the logo at size x size pixels; margin as share of the size per side."""
    tile = UNIT / (1 - 2 * margin)  # logo units per image width
    s = tile / size                 # logo units per pixel
    off = margin * tile             # image origin in logo units is -off
    rows = []
    for y in range(size):
        py = y * s - off
        row = bytearray()
        for x in range(size):
            px = x * s - off
            a = tile_coverage(px, py, s, 0.0, UNIT, RADIUS)
            if a == 0:
                row += b"\0\0\0\0"
                continue
            rest = 1.0
            rgb = [0.0, 0.0, 0.0]
            for x0, y0, x1, y1, col, sign in RECTS:
                share = sign * overlap(px, px + s, x0, x1) * overlap(py, py + s, y0, y1) / (s * s)
                if share:
                    rest -= share
                    for k in range(3):
                        rgb[k] += share * col[k]
            for k in range(3):
                rgb[k] += rest * BG[k]
            row += bytes(min(255, max(0, round(v))) for v in rgb) + bytes([round(a * 255)])
        rows.append(bytes(row))
    return rows


def png(rows):
    size = len(rows)

    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))

    raw = b"".join(b"\0" + r for r in rows)
    return (b"\x89PNG\r\n\x1a\n"
            + chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(raw, 9))
            + chunk(b"IEND", b""))


def ico(images):
    """images: [(size, png bytes)] – PNG-compressed ICO (Windows Vista and later)."""
    head = struct.pack("<HHH", 0, 1, len(images))
    offset = len(head) + 16 * len(images)
    entries, data = b"", b""
    for size, blob in images:
        dim = 0 if size >= 256 else size
        entries += struct.pack("<BBBBHHII", dim, dim, 0, 0, 1, 32, len(blob), offset + len(data))
        data += blob
    return head + entries + data


def icns(images):
    """images: [(type, png bytes)]."""
    body = b"".join(kind + struct.pack(">I", len(blob) + 8) + blob for kind, blob in images)
    return b"icns" + struct.pack(">I", len(body) + 8) + body


def write(name, data):
    path = os.path.join(HERE, name)
    with open(path, "wb") as f:
        f.write(data)
    print("%s (%d bytes)" % (path, len(data)))


def main():
    write("hardcalc.svg", svg().encode())
    flat = {n: png(render(n)) for n in (16, 24, 32, 48, 64, 256)}
    write("hardcalc.png", flat[256])
    write("hardcalc.ico", ico(sorted(flat.items())))
    # macOS draws icons on a grid with a margin of 100/1024 per side.
    mac = {n: png(render(n, 100 / 1024)) for n in (16, 32, 48, 64, 128, 256, 512, 1024)}
    write("hardcalc.icns", icns([
        (b"icp4", mac[16]), (b"icp5", mac[32]), (b"icp6", mac[48]),
        (b"ic07", mac[128]), (b"ic08", mac[256]), (b"ic09", mac[512]), (b"ic10", mac[1024]),
        (b"ic11", mac[32]), (b"ic12", mac[64]), (b"ic13", mac[256]), (b"ic14", mac[512]),
    ]))


if __name__ == "__main__":
    main()
