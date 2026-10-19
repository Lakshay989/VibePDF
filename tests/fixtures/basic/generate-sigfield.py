#!/usr/bin/env python3
"""Regenerate tests/fixtures/basic/sigfield.pdf.

A one-page document with an **empty signature field** named `Approval` — the
shape P6.A5b signs into rather than adding a field of its own. Documents that
arrive with a signature field waiting for you are the common case for
certificate signing: a contract, an approval form, anything routed for sign-off.

The field has a /Rect (it is a visible placeholder box) and no /V, which is what
"unsigned" means. A second field, `Countersign`, exists so tests can tell
"picked the right field" from "picked the only field".

No external deps. Run from anywhere:
    python3 tests/fixtures/basic/generate-sigfield.py
"""
from pathlib import Path

OUT = Path(__file__).parent / "sigfield.pdf"

CONTENT = b"""BT /F1 14 Tf 72 720 Td (Approval form) Tj ET
BT /F1 10 Tf 72 640 Td (Signed:) Tj ET
BT /F1 10 Tf 72 560 Td (Countersigned:) Tj ET
"""


def stream(dict_body: bytes, data: bytes) -> bytes:
    return (
        b"<< " + dict_body + b" /Length " + str(len(data)).encode() + b" >>\n"
        b"stream\n" + data + b"\nendstream"
    )


objects: list[bytes] = [
    b"<< /Type /Catalog /Pages 2 0 R /AcroForm 6 0 R >>",
    b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
    b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
    b"/Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R "
    b"/Annots [7 0 R 8 0 R] >>",
    b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
    stream(b"", CONTENT),
    # 6 AcroForm — two signature fields, neither signed
    b"<< /Fields [7 0 R 8 0 R] /SigFlags 3 /DA (/Helv 0 Tf 0 g) >>",
    # 7 the field under test: no /V, so unsigned
    b"<< /Type /Annot /Subtype /Widget /FT /Sig /T (Approval) "
    b"/Rect [130 630 330 665] /F 4 /P 3 0 R >>",
    # 8 a second one, so "the right field" and "the only field" differ
    b"<< /Type /Annot /Subtype /Widget /FT /Sig /T (Countersign) "
    b"/Rect [130 550 330 585] /F 4 /P 3 0 R >>",
]

out = bytearray(b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n")
offsets = [0]
for i, body in enumerate(objects, start=1):
    offsets.append(len(out))
    out += str(i).encode() + b" 0 obj\n" + body + b"\nendobj\n"

xref_at = len(out)
n = len(objects) + 1
out += b"xref\n0 " + str(n).encode() + b"\n0000000000 65535 f \n"
for off in offsets[1:]:
    out += f"{off:010d} 00000 n \n".encode()
out += (
    b"trailer\n<< /Size " + str(n).encode() + b" /Root 1 0 R "
    b"/ID [<0123456789ABCDEF0123456789ABCDEF> <0123456789ABCDEF0123456789ABCDEF>] >>\n"
    b"startxref\n" + str(xref_at).encode() + b"\n%%EOF\n"
)

OUT.write_bytes(bytes(out))
print(f"wrote {OUT} ({len(out)} bytes)")
