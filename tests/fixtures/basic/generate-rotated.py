#!/usr/bin/env python3
"""Regenerate tests/fixtures/basic/rotated.pdf.

A minimal PDF 1.4 with 4 US-Letter pages carrying /Rotate 0, 90, 180, 270
(one each), each rendering "Rotate N" in built-in Helvetica. Used by the
P4.HF hardening tests: page decorations (watermark / background /
header-footer) must compensate for page rotation so their output reads
upright in the *displayed* orientation. No external deps.

Run from anywhere:
    python3 tests/fixtures/basic/generate-rotated.py
"""
from pathlib import Path

OUT = Path(__file__).parent / "rotated.pdf"
ROTATIONS = [0, 90, 180, 270]

# Object ids: 1 catalog, 2 pages, 3 font, then per page i: 4+2i page, 5+2i content.
kids = " ".join(f"{4 + 2 * i} 0 R" for i in range(len(ROTATIONS)))
objects: list[bytes] = [
    b"<< /Type /Catalog /Pages 2 0 R >>",
    f"<< /Type /Pages /Kids [{kids}] /Count {len(ROTATIONS)} >>".encode(),
    b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
]
for i, rot in enumerate(ROTATIONS):
    content_id = 5 + 2 * i
    objects.append(
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
        + f"/Rotate {rot} ".encode()
        + b"/Resources << /Font << /F1 3 0 R >> >> "
        + f"/Contents {content_id} 0 R >>".encode()
    )
    stream = f"BT /F1 18 Tf 72 700 Td (Rotate {rot}) Tj ET".encode()
    objects.append(
        b"<< /Length " + str(len(stream)).encode() + b" >>\nstream\n"
        + stream + b"\nendstream"
    )

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
print(f"wrote {OUT} ({len(out)} bytes, {len(ROTATIONS)} pages, /Rotate {ROTATIONS})")
