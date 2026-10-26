#!/usr/bin/env python3
"""Draws the Kith icon (src-tauri/icons/*): a purple-to-blue rounded square with a contact silhouette."""
import os
from PIL import Image, ImageDraw, ImageFilter

S = 1024
OUT = os.path.join(os.path.dirname(__file__), "..", "src-tauri", "icons")

def gradient(size, c1, c2):
    img = Image.new("RGB", (size, size))
    px = img.load()
    for y in range(size):
        for x in range(size):
            t = (x + y) / (2 * size)
            px[x, y] = tuple(int(a + (b - a) * t) for a, b in zip(c1, c2))
    return img

def icon(size=S):
    bg = gradient(size, (203, 130, 247), (96, 150, 250)).convert("RGBA")
    mask = Image.new("L", (size, size), 0)
    ImageDraw.Draw(mask).rounded_rectangle((0, 0, size - 1, size - 1), radius=int(size * 0.225), fill=255)
    out = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    out.paste(bg, (0, 0), mask)
    # silhouette: a head and shoulders, softly shadowed
    sh = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    d = ImageDraw.Draw(sh)
    cx = size // 2
    d.ellipse((cx - size * 0.15, size * 0.24, cx + size * 0.15, size * 0.24 + size * 0.30), fill=(40, 20, 90, 120))
    d.rounded_rectangle((cx - size * 0.29, size * 0.58, cx + size * 0.29, size * 0.86), radius=int(size * 0.14), fill=(40, 20, 90, 120))
    sh = sh.filter(ImageFilter.GaussianBlur(size * 0.012)).transform(sh.size, Image.AFFINE, (1, 0, 0, 0, 1, -size * 0.012))
    out.alpha_composite(sh)
    fg = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    d = ImageDraw.Draw(fg)
    d.ellipse((cx - size * 0.15, size * 0.22, cx + size * 0.15, size * 0.22 + size * 0.30), fill=(255, 255, 255, 255))
    d.rounded_rectangle((cx - size * 0.29, size * 0.56, cx + size * 0.29, size * 0.84), radius=int(size * 0.14), fill=(255, 255, 255, 255))
    out.alpha_composite(fg)
    return out

big = icon()
for name, px in (("icon.png", 512), ("128x128@2x.png", 256), ("128x128.png", 128), ("32x32.png", 32)):
    big.resize((px, px), Image.LANCZOS).save(os.path.join(OUT, name))
big.resize((256, 256), Image.LANCZOS).save(os.path.join(OUT, "icon.ico"), sizes=[(16, 16), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])
big.resize((512, 512), Image.LANCZOS).save(os.path.join(OUT, "..", "..", "docs", "icon.png")) if os.path.isdir(os.path.join(OUT, "..", "..", "docs")) else None
print("icons written")
