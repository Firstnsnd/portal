#!/usr/bin/env python3
"""Generate assets/portal.ico from the existing PNG ladder (stdlib only).

ICO containers may embed PNG-compressed entries directly — valid since
Windows Vista and what every modern Windows app ships. The 16/32/64/128/256
PNGs are wrapped verbatim; no pixel data is re-encoded here.

Usage: python3 scripts/gen-ico.py   (from the repo root)
Output: assets/portal.ico — committed; CI never generates it.
"""

import os
import struct

SIZES = [16, 32, 64, 128, 256]
SRC = "assets/portal-icon-{}.png"
OUT = "assets/portal.ico"


def build_ico(pngs):
    # ICONDIR: reserved(0), type(1 = icon), image count
    out = bytearray(struct.pack("<HHH", 0, 1, len(pngs)))
    offset = 6 + 16 * len(pngs)  # header + one ICONDIRENTRY per image
    for size, png in pngs:
        # ICONDIRENTRY: width/height bytes (0 means 256), palette count,
        # reserved, color planes, bits per pixel, data length, data offset.
        wbyte = 0 if size >= 256 else size
        out += struct.pack("<BBBBHHII", wbyte, wbyte, 0, 0, 1, 32, len(png), offset)
        offset += len(png)
    for _, png in pngs:
        out += png
    return bytes(out)


def main():
    pngs = []
    for size in SIZES:
        with open(SRC.format(size), "rb") as f:
            pngs.append((size, f.read()))
    ico = build_ico(pngs)
    with open(OUT, "wb") as f:
        f.write(ico)
    print(f"wrote {OUT} ({len(ico)} bytes, {len(pngs)} sizes)")


if __name__ == "__main__":
    assert os.path.isdir("assets"), "run from the repo root"
    main()
