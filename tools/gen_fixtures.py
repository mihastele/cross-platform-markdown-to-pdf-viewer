#!/usr/bin/env python3
"""Generates the PNG/JPEG fixtures used by tests and examples. PNGs are written
with plain zlib (independent of our decoder), including colour types and
interlacing that real-world files use."""
import struct, zlib, math, sys, os

def chunk(t, d):
    c = struct.pack(">I", len(d)) + t + d
    return c + struct.pack(">I", zlib.crc32(t + d) & 0xffffffff)

def png(path, w, h, ctype, depth, pixel_rows, plte=None, trns=None, interlace=False):
    """pixel_rows(y) -> bytes of one packed scanline (no filter byte)."""
    raw = bytearray()
    if not interlace:
        for y in range(h):
            raw.append(y % 5)  # cycle through all 5 filter types
            raw += filt(y % 5, pixel_rows(y, None), prev_row.get(path), ctype, depth)
            prev_row[path] = pixel_rows(y, None)
        prev_row.pop(path, None)
    else:
        raise SystemExit("use adam7()")
    write(path, w, h, ctype, depth, raw, plte, trns, 0)

prev_row = {}

def bpp(ctype, depth):
    ch = {0: 1, 2: 3, 3: 1, 4: 2, 6: 4}[ctype]
    return max(1, ch * depth // 8)

def filt(ft, row, prev, ctype, depth):
    n = bpp(ctype, depth)
    prev = prev or bytes(len(row))
    out = bytearray()
    for i, x in enumerate(row):
        a = row[i - n] if i >= n else 0
        b = prev[i]
        c = prev[i - n] if i >= n else 0
        if ft == 0: p = 0
        elif ft == 1: p = a
        elif ft == 2: p = b
        elif ft == 3: p = (a + b) // 2
        else:
            pa, pb, pc = abs(b - c), abs(a - c), abs(a + b - 2 * c)
            p = a if pa <= pb and pa <= pc else (b if pb <= pc else c)
        out.append((x - p) & 255)
    return bytes(out)

def write(path, w, h, ctype, depth, raw, plte, trns, interlace):
    data = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, depth, ctype, 0, 0, interlace))
    if plte: data += chunk(b"PLTE", bytes(plte))
    if trns: data += chunk(b"tRNS", bytes(trns))
    data += chunk(b"IDAT", zlib.compress(bytes(raw), 9)) + chunk(b"IEND", b"")
    open(path, "wb").write(data)

def rgba_at(x, y, w, h):
    # Diagonal colour gradient inside a rounded-ish badge, transparent outside.
    cx, cy, r = w / 2, h / 2, min(w, h) / 2 - 1
    d = math.hypot(x - cx, y - cy)
    if d > r:
        return (0, 0, 0, 0)
    a = 255 if d < r - 2 else int(255 * (r - d) / 2)
    return (int(255 * x / w), int(120 + 100 * y / h), int(255 * (1 - x / w)), a)

out = sys.argv[1]
W, H = 96, 64
png(f"{out}/rgba.png", W, H, 6, 8, lambda y, _: b"".join(bytes(rgba_at(x, y, W, H)) for x in range(W)))
png(f"{out}/gray16.png", 40, 24, 0, 16, lambda y, _: b"".join(struct.pack(">H", (x * 65535) // 39) for x in range(40)))
pal = [(255, 255, 255), (220, 50, 50), (40, 90, 200), (30, 160, 90)]
png(f"{out}/pal4.png", 32, 32, 3, 4,
    lambda y, _: bytes(((((x // 8 + y // 8) % 4) << 4) | (((x + 1) // 8 + y // 8) % 4)) for x in range(0, 32, 2)),
    plte=[c for p in pal for c in p], trns=[0, 255, 255, 255])

# Adam7 interlaced RGB 8-bit.
def adam7(path, w, h, px):
    passes = [(0, 0, 8, 8), (4, 0, 8, 8), (0, 4, 4, 8), (2, 0, 4, 4), (0, 2, 2, 4), (1, 0, 2, 2), (0, 1, 1, 2)]
    raw = bytearray()
    for x0, y0, dx, dy in passes:
        if w <= x0 or h <= y0: continue
        prev = None
        for y in range(y0, h, dy):
            row = b"".join(bytes(px(x, y)) for x in range(x0, w, dx))
            ft = (y + x0) % 5
            raw.append(ft)
            raw += filt(ft, row, prev, 2, 8)
            prev = row
    write(path, w, h, 2, 8, raw, None, None, 1)
adam7(f"{out}/adam7.png", 37, 29, lambda x, y: ((x * 7) % 256, (y * 9) % 256, ((x + y) * 5) % 256))

# A wide "chart" PNG for the examples.
CW, CH = 640, 260
def chart(x, y):
    bg = (250, 251, 253)
    if x < 40 or y > CH - 30: return (60, 70, 90) if (x == 40 or y == CH - 30) else bg
    bars = [0.45, 0.62, 0.38, 0.8, 0.7, 0.93]
    bw = (CW - 60) // len(bars)
    i = (x - 50) // bw
    if 0 <= i < len(bars) and (x - 50) % bw < bw - 14:
        top = int((CH - 30) * (1 - bars[i]))
        if y >= top:
            t = (y - top) / max(1, CH - 30 - top)
            return (int(60 + 30 * t), int(110 + 40 * t), int(220 - 30 * t))
    return bg
png(f"{out}/chart.png", CW, CH, 2, 8, lambda y, _: b"".join(bytes(chart(x, y)) for x in range(CW)))
print("fixtures written")
