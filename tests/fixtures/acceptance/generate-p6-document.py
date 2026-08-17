#!/usr/bin/env python3
"""Regenerate tests/fixtures/acceptance/p6-document.pdf.

The acceptance fixture for P6-SEC-010 (true redaction). Everything in it is
positioned deliberately, because the interesting cases in redaction are all
about *where* the region falls:

  page 1, Helvetica (Core-14, so glyph advances are exactly known):
    y=740  "Employee record"              — the control. Must survive.
    y=700  "SSN: 123-45-6789"             — one run. A region over the digits
                                            only exercises the partial split:
                                            "SSN:" stays, the number goes.
    y=660  "Department: Engineering"      — outside any test region.

  page 2:
    the same kind of text, but inside a **Form XObject**. A page-content walk
    cannot see it, so redaction must refuse rather than report success. The
    number there (987-65-4321) is the marker for that case.

  page 4:
    a raw RGB image whose pixel bytes spell SECRETPIXELDATA. Removing the `Do`
    that draws it is not enough — the image stream stays in the file and a byte
    search still finds it. The object has to go too.

  page 3:
    text in a font with no metrics we can trust — an unrecognised /BaseFont and
    no /Widths array. Its advances are unknowable, so a region touching part of
    the run must take the *whole* run. This is the fixture for the module's
    governing principle, and without it that branch is never exercised.

Helvetica advances, per 1000 em: S=667 N=722 ':'=278 space=278, digits=556,
'-'=333. So at 12pt, "SSN: " is 31.3pt wide and the number is 68.0pt — the
digits run from x=103.3 to x=171.4, which is where the test's region goes.

No external deps. Run from anywhere:
    python3 tests/fixtures/acceptance/generate-p6-document.py
"""
from pathlib import Path

OUT = Path(__file__).parent / "p6-document.pdf"

CONTENT1 = b"""BT /F1 14 Tf 72 740 Td (Employee record) Tj ET
BT /F1 12 Tf 72 700 Td (SSN: 123-45-6789) Tj ET
BT /F1 12 Tf 72 660 Td (Department: Engineering) Tj ET
"""

# Page 2 draws its text through a Form XObject, which a page-content walk
# cannot see into.
CONTENT2 = b"""q 1 0 0 1 72 640 cm /Fx1 Do Q
"""

FORM = b"""BT /F1 12 Tf 10 50 Td (Contractor SSN: 987-65-4321) Tj ET
"""

# 16 pixels of RGB whose bytes are readable, so a byte search can prove the
# image object itself was deleted and not merely unreferenced.
IMAGE_DATA = b"SECRETPIXELDATA!" * 3

# One run, in a font with no usable metrics. 555-0100 is the marker.
UNMEASURABLE = b"""BT /F2 12 Tf 72 700 Td (Account: 555-0100) Tj ET
BT /F2 12 Tf 72 640 Td (Elsewhere on the page) Tj ET
"""


def stream(dict_body: bytes, data: bytes) -> bytes:
    return (
        b"<< " + dict_body + b" /Length " + str(len(data)).encode() + b" >>\n"
        b"stream\n" + data + b"\nendstream"
    )


objects: list[bytes] = [
    # 1 catalog
    b"<< /Type /Catalog /Pages 2 0 R >>",
    # 2 page tree
    b"<< /Type /Pages /Kids [3 0 R 6 0 R 9 0 R 12 0 R] /Count 4 >>",
    # 3 page 1 — plain page content
    b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
    b"/Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>",
    # 4 Helvetica
    b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
    # 5 content for page 1
    stream(b"", CONTENT1),
    # 6 page 2 — text hidden inside a form
    b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
    b"/Resources << /XObject << /Fx1 8 0 R >> >> /Contents 7 0 R >>",
    # 7 content for page 2
    stream(b"", CONTENT2),
    # 8 the form XObject, with its own font resource
    stream(
        b"/Type /XObject /Subtype /Form /BBox [0 0 300 100] "
        b"/Resources << /Font << /F1 4 0 R >> >>",
        FORM,
    ),
    # 9 page 3 — a font whose advances we cannot know
    b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
    b"/Resources << /Font << /F2 10 0 R >> >> /Contents 11 0 R >>",
    # 10 no /Widths, and a name no metric table knows
    b"<< /Type /Font /Subtype /Type1 /BaseFont /AcmeSans >>",
    # 11 content for page 3
    stream(b"", UNMEASURABLE),
    # 12 page 4 — one image, placed at 100,600, 200pt square
    b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
    b"/Resources << /XObject << /Im1 14 0 R >> >> /Contents 13 0 R >>",
    # 13 content for page 4
    stream(b"", b"q 200 0 0 200 100 600 cm /Im1 Do Q\n"),
    # 14 the image itself
    stream(
        b"/Type /XObject /Subtype /Image /Width 16 /Height 1 "
        b"/ColorSpace /DeviceRGB /BitsPerComponent 8",
        IMAGE_DATA,
    ),
]

out = bytearray(b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n")
offsets = [0]
for i, body in enumerate(objects, start=1):
    offsets.append(len(out))
    out += str(i).encode() + b" 0 obj\n" + body + b"\nendobj\n"

xref_at = len(out)
n = len(objects) + 1
out += b"xref\n0 " + str(n).encode() + b"\n"
out += b"0000000000 65535 f \n"
for off in offsets[1:]:
    out += f"{off:010d} 00000 n \n".encode()
out += (
    b"trailer\n<< /Size " + str(n).encode() + b" /Root 1 0 R >>\n"
    b"startxref\n" + str(xref_at).encode() + b"\n%%EOF\n"
)

OUT.write_bytes(bytes(out))
print(f"wrote {OUT} ({len(out)} bytes)")
