#!/usr/bin/env python3
"""Regenerate tests/fixtures/basic/forms-choice.pdf.

A minimal PDF 1.4 with ONE US-Letter page carrying an AcroForm with:
  - a single-select combo box "fruit" (/Opt: Apple, Banana, and a labelled
    export/display pair [chy Cherry]); pre-selected /V (Apple), and
  - a multi-select list box "colors" (/Opt: Red, Green, Blue).

For the P5.A4 choice-fill tests + cross-reader demo. No external deps.
    python3 tests/fixtures/basic/generate-forms-choice.py
"""
from pathlib import Path

OUT = Path(__file__).parent / "forms-choice.pdf"

objects: list[bytes] = []


def add(body: bytes) -> None:
    objects.append(body)


page_stream = b"BT /F1 18 Tf 72 740 Td (Choice form) Tj ET"
contents = (
    b"<< /Length " + str(len(page_stream)).encode() + b" >>\nstream\n" + page_stream + b"\nendstream"
)

#  1 catalog | 2 pages | 3 page | 4 combo | 5 list | 6 acroform | 7 font | 8 contents
add(b"<< /Type /Catalog /Pages 2 0 R /AcroForm 6 0 R >>")
add(b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>")
add(
    b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
    b"/Resources << /Font << /F1 7 0 R >> >> "
    b"/Contents 8 0 R /Annots [4 0 R 5 0 R] >>"
)
add(
    b"<< /Type /Annot /Subtype /Widget /FT /Ch /Ff 131072 /T (fruit) "
    b"/Opt [(Apple) (Banana) [(chy) (Cherry)]] /V (Apple) "
    b"/Rect [72 700 250 724] /DA (/F1 12 Tf 0 g) /P 3 0 R >>"
)
add(
    b"<< /Type /Annot /Subtype /Widget /FT /Ch /Ff 2097152 /T (colors) "
    b"/Opt [(Red) (Green) (Blue)] "
    b"/Rect [72 600 250 684] /DA (/F1 12 Tf 0 g) /P 3 0 R >>"
)
add(
    b"<< /Fields [4 0 R 5 0 R] /DR << /Font << /F1 7 0 R >> >> >>"
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
