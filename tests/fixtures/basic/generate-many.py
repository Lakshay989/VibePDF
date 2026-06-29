#!/usr/bin/env python3
"""Regenerate tests/fixtures/basic/many-pages.pdf.

A minimal PDF 1.4 with 50 US-Letter pages, each rendering "Page N" in
built-in Helvetica. Used for the watermark <2s acceptance (P4-EDIT-009)
and as a generic multi-page fixture for Track D. No external deps.

Run from anywhere:
    python3 tests/fixtures/basic/generate-many.py
"""
from pathlib import Path

OUT = Path(__file__).parent / "many-pages.pdf"
N = 50

# Object ids: 1 catalog, 2 pages, 3 font, then per page i (0-based):
#   page dict = 4 + 2*i, content = 5 + 2*i.
kids = " ".join(f"{4 + 2 * i} 0 R" for i in range(N))
objects: list[bytes] = [
    b"<< /Type /Catalog /Pages 2 0 R >>",
    f"<< /Type /Pages /Kids [{kids}] /Count {N} >>".encode(),
    b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
]
for i in range(N):
    content_id = 5 + 2 * i
    objects.append(
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
        b"/Resources << /Font << /F1 3 0 R >> >> "
        + f"/Contents {content_id} 0 R >>".encode()
    )
    stream = f"BT /F1 18 Tf 72 720 Td (Page {i + 1}) Tj ET".encode()
    objects.append(
        b"<< /Length " + str(len(stream)).encode() + b" >>\nstream\n"
        + stream + b"\nendstream"
    )

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
print(f"wrote {OUT} ({len(out)} bytes, {N} pages)")
