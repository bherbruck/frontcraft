#!/usr/bin/env python3
"""Draws the FrontCraft icon, the same shapes as the page's favicon, and writes it as .ico (Windows) and .icns (macOS).

Usage: scripts/make_icons.py <out dir> [ico] [icns]   writes <out dir>/frontcraft.ico and/or .icns, both by default

Plain Python with nothing to install, so every build machine can run it. Both formats hold PNGs, which Windows
since Vista and macOS since 10.7 read.
"""
import struct
import sys
import zlib
from pathlib import Path

# On a 32 unit square, as in the favicon: the ground, the block, its lit top.
SHAPES = [
    ((0x10, 0x14, 0x0F), [(0, 0), (32, 0), (32, 32), (0, 32)]),
    ((0xA9, 0xB0, 0x8F), [(16, 4), (28, 12), (28, 20), (16, 28), (4, 20), (4, 12)]),
    ((0x5F, 0x6A, 0x4C), [(16, 4), (28, 12), (16, 20), (4, 12)]),
]
SAMPLES = 4  # per side of a pixel, for smooth edges


def inside(polygon, x, y):
    """Convex and clockwise on screen, so a point is inside when it is right of every edge."""
    for (ax, ay), (bx, by) in zip(polygon, polygon[1:] + polygon[:1]):
        if (bx - ax) * (y - ay) - (by - ay) * (x - ax) < 0:
            return False
    return True


def draw(size):
    rows = []
    scale = 32 / size
    offsets = [(i + 0.5) / SAMPLES for i in range(SAMPLES)]
    for py in range(size):
        row = bytearray([0])
        for px in range(size):
            total = [0, 0, 0]
            for oy in offsets:
                for ox in offsets:
                    x, y = (px + ox) * scale, (py + oy) * scale
                    colour = (0, 0, 0)
                    for fill, polygon in SHAPES:
                        if inside(polygon, x, y):
                            colour = fill
                    for c in range(3):
                        total[c] += colour[c]
            row += bytes(round(t / SAMPLES**2) for t in total) + b"\xff"
        rows.append(bytes(row))
    return png(size, b"".join(rows))


def png(size, raw):
    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))

    header = struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header) + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b"")


def ico(images):
    out = struct.pack("<HHH", 0, 1, len(images))
    offset = 6 + 16 * len(images)
    for size, data in images:
        side = 0 if size >= 256 else size
        out += struct.pack("<BBBBHHII", side, side, 0, 0, 1, 32, len(data), offset)
        offset += len(data)
    return out + b"".join(data for _, data in images)


def icns(images):
    kinds = {16: b"icp4", 32: b"icp5", 64: b"icp6", 128: b"ic07", 256: b"ic08", 512: b"ic09"}
    body = b"".join(kinds[size] + struct.pack(">I", 8 + len(data)) + data for size, data in images if size in kinds)
    return b"icns" + struct.pack(">I", 8 + len(body)) + body


def main():
    out = Path(sys.argv[1] if len(sys.argv) > 1 else ".")
    formats = set(sys.argv[2:]) or {"ico", "icns"}
    out.mkdir(parents=True, exist_ok=True)
    sizes = {"ico": (16, 32, 48, 64, 256), "icns": (16, 32, 64, 128, 256, 512)}
    images = {size: draw(size) for kind in formats for size in sizes[kind]}
    if "ico" in formats:
        (out / "frontcraft.ico").write_bytes(ico([(s, images[s]) for s in sizes["ico"]]))
    if "icns" in formats:
        (out / "frontcraft.icns").write_bytes(icns([(s, images[s]) for s in sizes["icns"]]))


if __name__ == "__main__":
    main()
