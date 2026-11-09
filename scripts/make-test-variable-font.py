# SPDX-License-Identifier: MIT OR Apache-2.0
# Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>
"""Builds the small variable font frus's text tests shape with (milestone 784).

    python scripts/make-test-variable-font.py

It writes crates/frus-text/tests/fonts/FrusTestVariable.ttf: the printable ASCII of the
bundled DejaVu Sans, renamed (the DejaVu/Bitstream Vera licence asks a modified font to
carry another name), with two axes whose effect a test can measure:

- `wdth` 50 to 100: at 50 every outline and advance is half as wide.
- `wght` 400 to 700: at 700 every outline and advance is a fifth wider.

The shapes are not a real bold or condensed design; they only have to move the advances by
an amount a test can predict. Needs fontTools (`pip install fonttools`).
"""

import os
import tempfile

from fontTools import subset
from fontTools.designspaceLib import AxisDescriptor, DesignSpaceDocument, SourceDescriptor
from fontTools.pens.transformPen import TransformPen
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.ttLib import TTFont
from fontTools.varLib import build

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SOURCE = os.path.join(ROOT, "crates", "frus-text", "assets", "DejaVuSans.ttf")
OUT = os.path.join(ROOT, "crates", "frus-text", "tests", "fonts", "FrusTestVariable.ttf")
FAMILY = "Frus Test Variable"


def subset_source(path):
    font = TTFont(SOURCE)
    options = subset.Options()
    options.layout_features = ["kern", "liga"]
    options.name_IDs = []
    options.notdef_outline = True
    options.hinting = False
    sub = subset.Subsetter(options)
    sub.populate(unicodes=range(0x20, 0x7F))
    sub.subset(font)
    for table in ("hdmx", "LTSH", "VDMX", "kern", "GPOS", "fpgm", "prep", "cvt "):
        if table in font:
            del font[table]
    # varLib reads the version 2+ fields of OS/2; DejaVu's is version 1.
    os2 = font["OS/2"]
    if os2.version < 2:
        os2.version = 4
        os2.sxHeight, os2.sCapHeight = 1120, 1493
        os2.usDefaultChar, os2.usBreakChar, os2.usMaxContext = 0, 32, 2
    name = font["name"]
    name.names = []
    for name_id, value in ((1, FAMILY), (2, "Regular"), (4, FAMILY), (6, "FrusTestVariable")):
        name.setName(value, name_id, 3, 1, 0x409)
    name.setName(
        "Derived from DejaVu Sans (Bitstream Vera licence); not Bitstream Vera.", 0, 3, 1, 0x409
    )
    font.save(path)


def scaled_master(source, path, factor):
    font = TTFont(source)
    glyf = font["glyf"]
    hmtx = font["hmtx"]
    glyph_set = font.getGlyphSet()
    for name in font.getGlyphOrder():
        glyph = glyf[name]
        if glyph.isComposite():
            for component in glyph.components:
                component.x = round(component.x * factor)
        elif glyph.numberOfContours > 0:
            pen = TTGlyphPen(glyph_set)
            glyph_set[name].draw(TransformPen(pen, (factor, 0, 0, 1, 0, 0)))
            glyf[name] = pen.glyph()
        advance, lsb = hmtx[name]
        hmtx[name] = (round(advance * factor), round(lsb * factor))
    font.save(path)


def main():
    with tempfile.TemporaryDirectory() as tmp:
        base = os.path.join(tmp, "base.ttf")
        narrow = os.path.join(tmp, "narrow.ttf")
        heavy = os.path.join(tmp, "heavy.ttf")
        subset_source(base)
        scaled_master(base, narrow, 0.5)
        scaled_master(base, heavy, 1.2)

        doc = DesignSpaceDocument()
        for tag, name, minimum, default, maximum in (
            ("wght", "Weight", 400, 400, 700),
            ("wdth", "Width", 50, 100, 100),
        ):
            axis = AxisDescriptor()
            axis.tag, axis.name = tag, name
            axis.minimum, axis.default, axis.maximum = minimum, default, maximum
            doc.addAxis(axis)
        for path, location in (
            (base, {"Weight": 400, "Width": 100}),
            (narrow, {"Weight": 400, "Width": 50}),
            (heavy, {"Weight": 700, "Width": 100}),
        ):
            source = SourceDescriptor()
            source.path = path
            source.location = location
            doc.addSource(source)
        varfont, _, _ = build(doc)
        os.makedirs(os.path.dirname(OUT), exist_ok=True)
        varfont.save(OUT)
    print(OUT, os.path.getsize(OUT), "bytes")


if __name__ == "__main__":
    main()
