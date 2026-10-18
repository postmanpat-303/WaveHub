#!/usr/bin/env python3
"""Installer bitmaps from the elephant icon (needs Pillow).
Outputs installer/welcome.bmp (164x314, welcome/finish pages) and installer/header.bmp (150x57).
Usage: python3 scripts/make_branding.py"""
import os
from PIL import Image, ImageDraw, ImageFont

ROOT = os.path.join(os.path.dirname(__file__), "..")
P = lambda *a: os.path.join(ROOT, *a)
PINK = (251, 37, 134)
PAPER = (250, 247, 242)
INK = (40, 32, 48)
icon = Image.open(P("assets", "icons", "wavehub_1024.png")).convert("RGBA")

def font(size, bold=False):
    name = "Poppins-SemiBold.ttf" if bold else "Poppins-Regular.ttf"
    return ImageFont.truetype(P("assets", "fonts", name), size)

def waves(d, w, top, color, n=3):
    for i in range(n):
        y = top + i * 14
        d.arc((-40, y, w + 40, y + 60), 200, 340, fill=color, width=3)

# Welcome page: soft paper background, pink band with waves, icon, name.
W, H = 164, 314
im = Image.new("RGB", (W, H), PAPER)
d = ImageDraw.Draw(im)
d.rectangle((0, 0, W, 150), fill=PINK)
waves(d, W, 108, (255, 120, 180))
ic = icon.resize((112, 112), Image.LANCZOS)
im.paste(ic, ((W - 112) // 2, 30), ic)
d.text((W // 2, 182), "Wave Hub", font=font(22, True), fill=INK, anchor="mm")
d.text((W // 2, 210), "Free for", font=font(12), fill=(110, 100, 120), anchor="mm")
d.text((W // 2, 226), "Windows and Linux", font=font(12), fill=(110, 100, 120), anchor="mm")
waves(d, W, 262, (255, 200, 225))
im.save(P("installer", "welcome.bmp"))

# Header: white with the icon on the right.
W, H = 150, 57
im = Image.new("RGB", (W, H), (255, 255, 255))
ic = icon.resize((49, 49), Image.LANCZOS)
im.paste(ic, (W - 53, 4), ic)
ImageDraw.Draw(im).text((6, H // 2), "Wave Hub", font=font(17, True), fill=INK, anchor="lm")
im.save(P("installer", "header.bmp"))
print("installer/welcome.bmp, installer/header.bmp")
