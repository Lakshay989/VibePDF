#!/usr/bin/env python3
"""Regenerate tests/fixtures/basic/forms.pdf.

A minimal PDF 1.4 with ONE US-Letter page carrying an AcroForm with a single
text field named "name" (a merged field/widget). This is the fixture for the
P2-PAGE-008 / P2.D1 form-field tests: the lopdf COS spike renames `/T (name)`
to `/T (name_2)` to prove the collision-resolution capability.

No external deps. Run from anywhere:
    python3 tests/fixtures/basic/generate-forms.py
"""
from pathlib import Path

OUT = Path(__file__).parent / "forms.pdf"

# Object numbers are fixed so the cross-references below line up.
#  1 catalog | 2 pages | 3 page | 4 field/widget | 5 acroform | 6 font | 7 contents
objects: list[bytes] = []


def add(body: bytes) -> None:
    objects.append(body)


stream = b"BT /F1 18 Tf 72 740 Td (Form) Tj ET"
contents = (
    b"<< /Length " + str(len(stream)).encode() + b" >>\nstream\n" + stream + b"\nendstream"
)

add(b"<< /Type /Catalog /Pages 2 0 R /AcroForm 5 0 R >>")
add(b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>")
add(
    b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
    b"/Resources << /Font << /F1 6 0 R >> >> "
    b"/Contents 7 0 R /Annots [4 0 R] >>"
)
# Terminal text field that is also its own widget annotation.
add(
    b"<< /Type /Annot /Subtype /Widget /FT /Tx /T (name) "
    b"/Rect [72 700 300 724] /DA (/F1 12 Tf 0 g) /P 3 0 R >>"
)
# AcroForm: one field, with default resources/appearance.
add(
    b"<< /Fields [4 0 R] /NeedAppearances true /DA (/F1 12 Tf 0 g) "
    b"/DR << /Font << /F1 6 0 R >> >> >>"
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
