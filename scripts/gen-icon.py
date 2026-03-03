#!/usr/bin/env python3
"""Generate Portal app icon as a 1024x1024 PNG using only the standard library.

Produces a Tokyo Night-themed rounded-rect icon with a terminal cursor ">_" symbol.
Output: assets/portal-icon-1024.png
"""

import struct
import zlib
import math
import os

W = H = 1024

# Tokyo Night colors
BG = (26, 27, 38)       # #1a1b26
ACCENT = (122, 162, 247) # #7aa2f7
FG = (192, 202, 245)     # #c0caf5
SHADOW = (15, 16, 24)    # darker bg for depth


def blend(bg, fg, alpha):
    """Alpha blend fg over bg, alpha 0.0-1.0."""
    return tuple(int(b * (1 - alpha) + f * alpha) for b, f in zip(bg, fg))


def sdf_rounded_rect(x, y, cx, cy, hw, hh, r):
    """Signed distance to a rounded rectangle centered at (cx,cy) with half-size (hw,hh) and radius r."""
    dx = max(abs(x - cx) - hw + r, 0)
    dy = max(abs(y - cy) - hh + r, 0)
    return math.sqrt(dx * dx + dy * dy) - r


def generate():
    pixels = bytearray(W * H * 4)  # RGBA

    cx, cy = W // 2, H // 2
    # macOS icon: rounded rect with ~22% corner radius
    margin = 20
    hw = W // 2 - margin
    hh = H // 2 - margin
    radius = int(W * 0.22)

    # ">_" glyph geometry
    # ">" chevron: left point, top, bottom
    chev_cx = cx - 100
    chev_cy = cy - 40
    chev_size = 180
    chev_thick = 48

    # "_" underscore
    us_x0 = cx + 20
    us_x1 = cx + 280
    us_y = cy + 140
    us_thick = 48

    for y in range(H):
        for x in range(W):
            d = sdf_rounded_rect(x, y, cx, cy, hw, hh, radius)

            if d > 1.5:
                # Outside: transparent
                r, g, b, a = 0, 0, 0, 0
            else:
                # Anti-aliased edge
                edge_alpha = max(0.0, min(1.0, 1.0 - d))

                # Subtle gradient: slightly lighter at top
                t = (y - (cy - hh)) / (2 * hh)
                grad = 1.0 - t * 0.15
                base = tuple(min(255, int(c * grad)) for c in BG)

                # Shadow at bottom
                if t > 0.85:
                    shadow_t = (t - 0.85) / 0.15
                    base = blend(base, SHADOW, shadow_t * 0.4)

                # Draw ">" chevron
                # Two line segments: top-right and bottom-right meeting at right point
                # Right point of chevron
                rpx = chev_cx + chev_size // 2
                rpy = chev_cy
                # Top-left of chevron
                tlx = chev_cx - chev_size // 2
                tly = chev_cy - chev_size // 2
                # Bottom-left of chevron
                blx = chev_cx - chev_size // 2
                bly = chev_cy + chev_size // 2

                color = base

                # Distance to line segment helper (inline)
                def dist_to_seg(px, py, ax, ay, bx, by):
                    dx, dy = bx - ax, by - ay
                    if dx == 0 and dy == 0:
                        return math.sqrt((px - ax) ** 2 + (py - ay) ** 2)
                    t = max(0, min(1, ((px - ax) * dx + (py - ay) * dy) / (dx * dx + dy * dy)))
                    projx = ax + t * dx
                    projy = ay + t * dy
                    return math.sqrt((px - projx) ** 2 + (py - projy) ** 2)

                # Top segment of ">"
                d1 = dist_to_seg(x, y, tlx, tly, rpx, rpy)
                # Bottom segment of ">"
                d2 = dist_to_seg(x, y, blx, bly, rpx, rpy)

                chev_d = min(d1, d2)
                half_thick = chev_thick / 2.0

                if chev_d < half_thick + 1.5:
                    glyph_alpha = max(0.0, min(1.0, half_thick + 1.0 - chev_d))
                    color = blend(color, ACCENT, glyph_alpha)

                # Draw "_" underscore
                if us_x0 - 1 <= x <= us_x1 + 1 and us_y - 1 <= y <= us_y + us_thick + 1:
                    ux_alpha = min(
                        max(0.0, min(1.0, x - us_x0 + 1.0)),
                        max(0.0, min(1.0, us_x1 + 1.0 - x)),
                        max(0.0, min(1.0, y - us_y + 1.0)),
                        max(0.0, min(1.0, us_y + us_thick + 1.0 - y)),
                    )
                    color = blend(color, FG, ux_alpha)

                r, g, b = color
                a = int(edge_alpha * 255)

            off = (y * W + x) * 4
            pixels[off] = r
            pixels[off + 1] = g
            pixels[off + 2] = b
            pixels[off + 3] = a

    return pixels


def write_png(path, pixels, width, height):
    """Write RGBA pixels as a PNG file."""
    def chunk(chunk_type, data):
        c = chunk_type + data
        crc = struct.pack('>I', zlib.crc32(c) & 0xFFFFFFFF)
        return struct.pack('>I', len(data)) + c + crc

    sig = b'\x89PNG\r\n\x1a\n'
    ihdr = struct.pack('>IIBBBBB', width, height, 8, 6, 0, 0, 0)  # 8-bit RGBA
    raw = bytearray()
    for y in range(height):
        raw.append(0)  # filter: none
        off = y * width * 4
        raw.extend(pixels[off:off + width * 4])
    compressed = zlib.compress(bytes(raw), 9)

    with open(path, 'wb') as f:
        f.write(sig)
        f.write(chunk(b'IHDR', ihdr))
        f.write(chunk(b'IDAT', compressed))
        f.write(chunk(b'IEND', b''))


if __name__ == '__main__':
    script_dir = os.path.dirname(os.path.abspath(__file__))
    project_dir = os.path.dirname(script_dir)
    out_path = os.path.join(project_dir, 'assets', 'portal-icon-1024.png')

    print(f'Generating {W}x{H} icon...')
    pixels = generate()
    write_png(out_path, pixels, W, H)
    print(f'Saved to {out_path}')
