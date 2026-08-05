#!/usr/bin/env python3
"""Regenerate tests/fixtures/basic/forms-buttons.pdf.

A minimal PDF 1.4 with ONE US-Letter page carrying an AcroForm with:
  - a checkbox field "agree" (on-state /Yes), and
  - a radio group "color" with two options /Red and /Green.

Every widget declares real /AP /N on+off Form-XObject appearances (a blue square
for "on", empty for "off"), so a viewer draws the check/dot from /AS. For the
P5.A3 fill tests + cross-reader demo. No external deps.
    python3 tests/fixtures/basic/generate-forms-buttons.py
"""
from pathlib import Path

OUT = Path(__file__).parent / "forms-buttons.pdf"

objects: list[bytes] = []


def add(body: bytes) -> None:
    objects.append(body)


page_stream = b"BT /F1 18 Tf 72 740 Td (Buttons form) Tj ET"
contents = (
    b"<< /Length " + str(len(page_stream)).encode() + b" >>\nstream\n" + page_stream + b"\nendstream"
)
on_stream = b"q 0 0 1 rg 3 3 12 12 re f Q"
on_ap = (
    b"<< /Type /XObject /Subtype /Form /BBox [0 0 18 18] /Length "
    + str(len(on_stream)).encode()
    + b" >>\nstream\n"
    + on_stream
    + b"\nendstream"
)
off_ap = b"<< /Type /XObject /Subtype /Form /BBox [0 0 18 18] /Length 0 >>\nstream\n\nendstream"

#  1 catalog | 2 pages | 3 page | 4 checkbox | 5 radio group | 6 radio Red |
#  7 radio Green | 8 acroform | 9 font | 10 contents | 11 on-AP | 12 off-AP
add(b"<< /Type /Catalog /Pages 2 0 R /AcroForm 8 0 R >>")
add(b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>")
add(
    b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
    b"/Resources << /Font << /F1 9 0 R >> >> "
    b"/Contents 10 0 R /Annots [4 0 R 6 0 R 7 0 R] >>"
)
add(
    b"<< /Type /Annot /Subtype /Widget /FT /Btn /T (agree) "
    b"/Rect [72 700 90 718] /AS /Off "
    b"/AP << /N << /Yes 11 0 R /Off 12 0 R >> >> /P 3 0 R >>"
)
add(
    b"<< /FT /Btn /Ff 32768 /T (color) /V /Off /Kids [6 0 R 7 0 R] >>"
)
add(
    b"<< /Type /Annot /Subtype /Widget /Parent 5 0 R "
    b"/Rect [72 660 90 678] /AS /Off "
    b"/AP << /N << /Red 11 0 R /Off 12 0 R >> >> /P 3 0 R >>"
)
add(
    b"<< /Type /Annot /Subtype /Widget /Parent 5 0 R "
    b"/Rect [72 630 90 648] /AS /Off "
    b"/AP << /N << /Green 11 0 R /Off 12 0 R >> >> /P 3 0 R >>"
)
add(
    b"<< /Fields [4 0 R 5 0 R] /DR << /Font << /F1 9 0 R >> >> >>"
)
add(b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>")
add(contents)
add(on_ap)
add(off_ap)

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
