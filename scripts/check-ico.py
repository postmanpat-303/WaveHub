#!/usr/bin/env python3
"""Check that an .ico (default assets/icons/wavehub.ico) keeps the layout Windows needs.

Explorer, Edge's download list and the shell icon cache often can't read PNG frames at small
sizes and show the generic program/installer icon instead. So the icon must have:
  256 px            PNG
  128/64/48/32/24/16 px   32-bit BMP (DIB) with an AND mask
Don't regenerate it with Pillow's ICO writer (Image.save(..., format="ICO")): that stores every
size as PNG. Give it .ico files, or .exe files to check the icon built into them (that needs
`pip install pefile`; the icon is also written out next to the exe as <name>.extracted.ico).
"""
import struct
import sys

NEED = {256: "PNG", 128: "BMP", 64: "BMP", 48: "BMP", 32: "BMP", 24: "BMP", 16: "BMP"}


def frames(data):
    _, kind, n = struct.unpack("<HHH", data[:6])
    if kind != 1:
        raise SystemExit("not an icon file")
    for i in range(n):
        w, h, _, _, _, bpp, size, off = struct.unpack("<BBBBHHII", data[6 + 16 * i:22 + 16 * i])
        d = data[off:off + size]
        yield (w or 256), bpp, ("PNG" if d[:8] == b"\x89PNG\r\n\x1a\n" else "BMP")


def ico_from_exe(path):
    """Rebuild the first icon group of a Windows program as an .ico file."""
    import pefile

    pe = pefile.PE(path)
    res = {}
    for t in pe.DIRECTORY_ENTRY_RESOURCE.entries:
        for e in t.directory.entries:
            lang = e.directory.entries[0].data.struct
            res[(t.id, e.id)] = pe.get_data(lang.OffsetToData, lang.Size)
    group = next(v for (t, _), v in sorted(res.items(), key=lambda kv: (kv[0][0], kv[0][1] or 0)) if t == 14)
    _, kind, n = struct.unpack("<HHH", group[:6])
    entries, images = [], []
    for i in range(n):
        w, h, c, r, planes, bpp, size, rid = struct.unpack("<BBBBHHIH", group[6 + 14 * i:20 + 14 * i])
        img = res[(3, rid)]
        entries.append((w, h, c, r, planes, bpp, len(img)))
        images.append(img)
    out = struct.pack("<HHH", 0, 1, n)
    off = 6 + 16 * n
    for (w, h, c, r, planes, bpp, size), img in zip(entries, images):
        out += struct.pack("<BBBBHHII", w, h, c, r, planes, bpp, size, off)
        off += size
    return out + b"".join(images)


def main(path):
    data = open(path, "rb").read()
    if data[:2] == b"MZ":
        data = ico_from_exe(path)
        open(path + ".extracted.ico", "wb").write(data)
    found = {w: (fmt, bpp) for w, bpp, fmt in frames(data)}
    bad = []
    for w, fmt in NEED.items():
        got = found.get(w)
        print(f"{w:>4} px: {got[0] + ' ' + str(got[1]) + '-bit' if got else 'missing'}")
        if not got or got[0] != fmt or got[1] != 32:
            bad.append(f"{w} px should be 32-bit {fmt}")
    if bad:
        raise SystemExit(f"{path}: " + "; ".join(bad))
    print(f"{path}: OK")


if __name__ == "__main__":
    for p in sys.argv[1:] or ["assets/icons/wavehub.ico"]:
        main(p)
