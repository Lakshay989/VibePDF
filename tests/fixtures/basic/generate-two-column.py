#!/usr/bin/env python3
"""Regenerate tests/fixtures/basic/two-column.pdf — the fixture that can fail
on reading order (P7-OCR-006).

Two columns of text. A person reads the whole left column, then the whole right
one. The *content stream* draws them interleaved — left line 1, right line 1,
left line 2, … — which is what page-layout tools commonly emit and what any
extractor that simply concatenates in stream order will give back, producing
"Alpha one Beta one Alpha two Beta two". That is the failure this fixture
exists to catch, so the interleaving is deliberate, not incidental.

No external dependencies.

Run from anywhere:
    python3 tests/fixtures/basic/generate-two-column.py
"""
from pathlib import Path

OUT = Path(__file__).parent / "two-column.pdf"
PAGE_WIDTH, PAGE_HEIGHT = 612, 792
LEFT_X, RIGHT_X = 72, 330
TOP_Y = 700
LEADING = 28
SIZE = 12

# Reading order is every LEFT line, then every RIGHT line.
LEFT = ["Alpha one", "Alpha two", "Alpha three", "Alpha four", "Alpha five"]
RIGHT = ["Beta one", "Beta two", "Beta three", "Beta four", "Beta five"]

drawn = []
for row, (left, right) in enumerate(zip(LEFT, RIGHT)):
    y = TOP_Y - row * LEADING
    # Interleaved on purpose: this is the order a naive extractor would return.
    drawn.append(f"BT /F1 {SIZE} Tf {LEFT_X} {y} Td ({left}) Tj ET")
    drawn.append(f"BT /F1 {SIZE} Tf {RIGHT_X} {y} Td ({right}) Tj ET")
stream = "\n".join(drawn).encode()

objects = [
    b"<< /Type /Catalog /Pages 2 0 R >>",
    b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
    (
        f"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {PAGE_WIDTH} {PAGE_HEIGHT}] "
        "/Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>"
    ).encode(),
    b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
    b"<< /Length " + str(len(stream)).encode() + b" >>\nstream\n" + stream + b"\nendstream",
]

out = bytearray(b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n")
offsets = []
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

OUT.write_bytes(bytes(out))
print(f"wrote {OUT} ({len(out)} bytes)")
