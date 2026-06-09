#!/usr/bin/env python3
"""Regenerate tests/fixtures/basic/annots.pdf.

A minimal PDF 1.4 with ONE US-Letter page carrying a /Square markup
annotation (a red rectangle). Unlike a /Link, a markup annotation has no
page destination, so it survives both page import AND the dangling-reference
prune — making it the right fixture for "insert preserves annotations"
(P2-PAGE-005) once dangling internal links are pruned on save.

No external deps. Run from anywhere:
    python3 tests/fixtures/basic/generate-annots.py
"""
from pathlib import Path

OUT = Path(__file__).parent / "annots.pdf"

#  1 catalog | 2 pages | 3 page | 4 square-annot | 5 font | 6 contents
objects: list[bytes] = []


def add(body: bytes) -> None:
    objects.append(body)


stream = b"BT /F1 18 Tf 72 740 Td (Annotated) Tj ET"
contents = (
    b"<< /Length " + str(len(stream)).encode() + b" >>\nstream\n" + stream + b"\nendstream"
)

add(b"<< /Type /Catalog /Pages 2 0 R >>")
add(b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>")
add(
    b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
    b"/Resources << /Font << /F1 5 0 R >> >> "
    b"/Contents 6 0 R /Annots [4 0 R] >>"
)
# A /Square markup annotation (red border) — no page destination.
add(
    b"<< /Type /Annot /Subtype /Square /Rect [100 600 300 700] "
    b"/C [1 0 0] /F 4 >>"
)
add(b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>")
add(contents)

out = bytearray()
out += b"%PDF-1.4\n"
out += b"%\xe2\xe3\xcf\xd3\n"  # binary-file marker
offsets: list[int] = []
for i, body in enumerate(objects, start=1):
    offsets.append(len(out))
    out += f"{i} 0 obj\n".encode() + body + b"\nendobj\n"
xref_offset = len(out)
out += b"xref\n"
out += f"0 {len(objects) + 1}\n".encode()
out += b"0000000000 65535 f \n"
for off in offsets:
    out += f"{off:010d} 00000 n \n".encode()
out += b"trailer\n"
out += f"<< /Size {len(objects) + 1} /Root 1 0 R >>\n".encode()
out += b"startxref\n"
out += f"{xref_offset}\n".encode()
out += b"%%EOF\n"

OUT.write_bytes(out)
print(f"wrote {OUT} ({len(out)} bytes)")
