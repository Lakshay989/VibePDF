#!/usr/bin/env python3
"""Regenerate tests/fixtures/basic/cropped.pdf.

A minimal PDF 1.4 with one US-Letter page whose /CropBox [100 100 512 692]
is strictly inside its /MediaBox [0 0 612 792] — the print-ready "bleed"
shape. Viewers display only the CropBox. Used by the P4.HF hardening
tests: decoration *placement* must target the visible (crop) area, while
a background colour fill still covers the full MediaBox. No external deps.

Run from anywhere:
    python3 tests/fixtures/basic/generate-cropped.py
"""
from pathlib import Path

OUT = Path(__file__).parent / "cropped.pdf"

stream = b"BT /F1 18 Tf 120 650 Td (Cropped page) Tj ET"
objects: list[bytes] = [
    b"<< /Type /Catalog /Pages 2 0 R >>",
    b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
    (
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
        b"/CropBox [100 100 512 692] "
        b"/Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>"
    ),
    b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
    b"<< /Length " + str(len(stream)).encode() + b" >>\nstream\n" + stream + b"\nendstream",
]

out = bytearray()
out += b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n"
offsets: list[int] = []
for i, body in enumerate(objects, start=1):
    offsets.append(len(out))
    out += f"{i} 0 obj\n".encode() + body + b"\nendobj\n"
xref_offset = len(out)
out += b"xref\n" + f"0 {len(objects) + 1}\n".encode() + b"0000000000 65535 f \n"
for off in offsets:
    out += f"{off:010d} 00000 n \n".encode()
out += b"trailer\n" + f"<< /Size {len(objects) + 1} /Root 1 0 R >>\n".encode()
out += b"startxref\n" + f"{xref_offset}\n".encode() + b"%%EOF\n"

OUT.write_bytes(out)
print(f"wrote {OUT} ({len(out)} bytes, CropBox [100 100 512 692] in MediaBox [0 0 612 792])")
