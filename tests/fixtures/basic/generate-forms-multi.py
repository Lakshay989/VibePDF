#!/usr/bin/env python3
"""Regenerate tests/fixtures/basic/forms-multi.pdf.

A minimal PDF 1.4 with ONE US-Letter page carrying an AcroForm with THREE text
fields, for the P5.A2 fill + tab-navigation tests/demo:
  - "first"  : plain text field
  - "last"   : text field with /MaxLen 5
  - "notes"  : multi-line text field (/Ff bit 13 = 4096)

Each is a merged field/widget (its own /Rect + /P). No external deps.
    python3 tests/fixtures/basic/generate-forms-multi.py
"""
from pathlib import Path

OUT = Path(__file__).parent / "forms-multi.pdf"

objects: list[bytes] = []


def add(body: bytes) -> None:
    objects.append(body)


stream = b"BT /F1 18 Tf 72 740 Td (Multi-field form) Tj ET"
contents = (
    b"<< /Length " + str(len(stream)).encode() + b" >>\nstream\n" + stream + b"\nendstream"
)

#  1 catalog | 2 pages | 3 page | 4 first | 5 last | 6 notes | 7 acroform | 8 font | 9 contents
add(b"<< /Type /Catalog /Pages 2 0 R /AcroForm 7 0 R >>")
add(b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>")
add(
    b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
    b"/Resources << /Font << /F1 8 0 R >> >> "
    b"/Contents 9 0 R /Annots [4 0 R 5 0 R 6 0 R] >>"
)
add(
    b"<< /Type /Annot /Subtype /Widget /FT /Tx /T (first) "
    b"/Rect [72 700 300 724] /DA (/F1 12 Tf 0 g) /P 3 0 R >>"
)
add(
    b"<< /Type /Annot /Subtype /Widget /FT /Tx /T (last) /MaxLen 5 "
    b"/Rect [72 660 300 684] /DA (/F1 12 Tf 0 g) /P 3 0 R >>"
)
add(
    b"<< /Type /Annot /Subtype /Widget /FT /Tx /T (notes) /Ff 4096 "
    b"/Rect [72 560 400 640] /DA (/F1 12 Tf 0 g) /P 3 0 R >>"
)
add(
    b"<< /Fields [4 0 R 5 0 R 6 0 R] /NeedAppearances true /DA (/F1 12 Tf 0 g) "
    b"/DR << /Font << /F1 8 0 R >> >> >>"
)
add(b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>")
add(contents)

out = bytearray()
out += b"%PDF-1.4\n"
out += b"%\xe2\xe3\xcf\xd3\n"
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
