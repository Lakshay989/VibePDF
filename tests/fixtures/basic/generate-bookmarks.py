#!/usr/bin/env python3
"""Regenerate tests/fixtures/basic/bookmarks.pdf.

A minimal PDF 1.4 with SIX US-Letter pages and a top-level outline of THREE
bookmarks pointing at pages 1, 3, and 5 (0-based indices 0, 2, 4). This is
the fixture for the P2-PAGE-007 "split by top-level bookmarks" test: the
split must start a new file at each bookmark's destination page, yielding
three 2-page files.

No external deps. Run from anywhere:
    python3 tests/fixtures/basic/generate-bookmarks.py
"""
from pathlib import Path

OUT = Path(__file__).parent / "bookmarks.pdf"

# Object numbers are fixed so the cross-references below line up.
#  1 catalog | 2 pages
#  3..8  page1..page6
#  9 font
#  10..15 content1..content6
#  16 outlines dict | 17,18,19 outline items (top-level bookmarks)
objects: list[bytes] = []


def add(body: bytes) -> None:
    objects.append(body)


def page(contents_obj: int) -> bytes:
    return (
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
        b"/Resources << /Font << /F1 9 0 R >> >> "
        b"/Contents " + str(contents_obj).encode() + b" 0 R >>"
    )


def contents(label: str) -> bytes:
    stream = b"BT /F1 24 Tf 72 720 Td (" + label.encode() + b") Tj ET"
    return (
        b"<< /Length " + str(len(stream)).encode() + b" >>\nstream\n"
        + stream + b"\nendstream"
    )


add(b"<< /Type /Catalog /Pages 2 0 R /Outlines 16 0 R >>")
add(b"<< /Type /Pages /Kids [3 0 R 4 0 R 5 0 R 6 0 R 7 0 R 8 0 R] /Count 6 >>")
for n in range(6):
    add(page(10 + n))  # page objects 3..8 → content 10..15
add(b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>")
for n in range(6):
    add(contents(f"Page {n + 1}"))
# Outline root + three top-level items. Dests reference page *objects*
# (obj 3 = page1/index 0, obj 5 = page3/index 2, obj 7 = page5/index 4).
add(b"<< /Type /Outlines /First 17 0 R /Last 19 0 R /Count 3 >>")
add(
    b"<< /Title (Chapter 1) /Parent 16 0 R /Next 18 0 R "
    b"/Dest [3 0 R /Fit] >>"
)
add(
    b"<< /Title (Chapter 2) /Parent 16 0 R /Prev 17 0 R /Next 19 0 R "
    b"/Dest [5 0 R /Fit] >>"
)
add(
    b"<< /Title (Chapter 3) /Parent 16 0 R /Prev 18 0 R "
    b"/Dest [7 0 R /Fit] >>"
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
print(f"wrote {OUT} ({len(out)} bytes)")
