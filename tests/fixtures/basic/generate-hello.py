#!/usr/bin/env python3
"""Regenerate tests/fixtures/basic/hello.pdf.

Produces a byte-identical minimal PDF 1.4 with one US-Letter page that
renders "Hello, VibePDF." using built-in Helvetica. No external deps.

Run from anywhere:
    python3 tests/fixtures/basic/generate-hello.py
"""
from pathlib import Path

OUT = Path(__file__).parent / "hello.pdf"

objects: list[bytes] = []

def add(body: bytes) -> int:
    objects.append(body)
    return len(objects)

catalog = b"<< /Type /Catalog /Pages 2 0 R >>"
pages = b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>"
page = (
    b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
    b"/Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>"
)
font = b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>"
stream = b"BT /F1 24 Tf 72 720 Td (Hello, VibePDF.) Tj ET"
contents = (
    b"<< /Length " + str(len(stream)).encode() + b" >>\nstream\n"
    + stream + b"\nendstream"
)

for obj in (catalog, pages, page, font, contents):
    add(obj)

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
