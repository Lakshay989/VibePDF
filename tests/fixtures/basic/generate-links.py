#!/usr/bin/env python3
"""Regenerate tests/fixtures/basic/links.pdf.

A minimal PDF 1.4 with THREE US-Letter pages. Page 1 carries a /Link
annotation whose /Dest references page 3's *object* (not its index). This
is the fixture for the P2-PAGE-003 reference-integrity test: after deleting
page 2, the link must still target page 3 (now at index 1), because PDF
destinations are indirect object references.

No external deps. Run from anywhere:
    python3 tests/fixtures/basic/generate-links.py
"""
from pathlib import Path

OUT = Path(__file__).parent / "links.pdf"

# Object numbers are fixed so the cross-references below line up.
#  1 catalog | 2 pages | 3 page1 | 4 page2 | 5 page3
#  6 font    | 7 c1     | 8 c2    | 9 c3    | 10 link-annot
objects: list[bytes] = []


def add(body: bytes) -> None:
    objects.append(body)


def page(contents_obj: int, extra: bytes = b"") -> bytes:
    return (
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
        b"/Resources << /Font << /F1 6 0 R >> >> "
        b"/Contents " + str(contents_obj).encode() + b" 0 R" + extra + b" >>"
    )


def contents(label: str) -> bytes:
    stream = b"BT /F1 24 Tf 72 720 Td (" + label.encode() + b") Tj ET"
    return (
        b"<< /Length " + str(len(stream)).encode() + b" >>\nstream\n"
        + stream + b"\nendstream"
    )


add(b"<< /Type /Catalog /Pages 2 0 R >>")
add(b"<< /Type /Pages /Kids [3 0 R 4 0 R 5 0 R] /Count 3 >>")
add(page(7, extra=b" /Annots [10 0 R]"))  # page 1, with the link
add(page(8))  # page 2
add(page(9))  # page 3
add(b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>")
add(contents("Page 1 (link to page 3)"))
add(contents("Page 2"))
add(contents("Page 3"))
# Internal link on page 1 → page 3's object (5 0 R). /Dest by object ref.
add(
    b"<< /Type /Annot /Subtype /Link /Rect [72 700 360 724] "
    b"/Border [0 0 1] /Dest [5 0 R /Fit] >>"
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
