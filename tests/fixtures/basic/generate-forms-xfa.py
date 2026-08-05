#!/usr/bin/env python3
"""Regenerate tests/fixtures/basic/forms-xfa.pdf.

A minimal PDF 1.4 that is XFA-only: its AcroForm has an empty /Fields array and
an /XFA stream (a tiny XDP packet), plus a line of static page text. This is the
"no AcroForm fallback" case P5.A5 detects and offers to flatten. No external deps.
    python3 tests/fixtures/basic/generate-forms-xfa.py
"""
from pathlib import Path

OUT = Path(__file__).parent / "forms-xfa.pdf"

objects: list[bytes] = []


def add(body: bytes) -> None:
    objects.append(body)


page_stream = b"BT /F1 18 Tf 72 740 Td (Static XFA content) Tj ET"
contents = (
    b"<< /Length " + str(len(page_stream)).encode() + b" >>\nstream\n" + page_stream + b"\nendstream"
)
xfa_xml = (
    b'<xdp:xdp xmlns:xdp="http://ns.adobe.com/xdp/">'
    b"<template xmlns=\"http://www.xfa.org/schema/xfa-template/3.0/\"/>"
    b"</xdp:xdp>"
)
xfa_stream = b"<< /Length " + str(len(xfa_xml)).encode() + b" >>\nstream\n" + xfa_xml + b"\nendstream"

#  1 catalog | 2 pages | 3 page | 4 acroform | 5 xfa | 6 contents | 7 font
add(b"<< /Type /Catalog /Pages 2 0 R /AcroForm 4 0 R >>")
add(b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>")
add(
    b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
    b"/Resources << /Font << /F1 7 0 R >> >> /Contents 6 0 R >>"
)
add(b"<< /Fields [] /XFA 5 0 R /NeedAppearances false >>")
add(xfa_stream)
add(contents)
add(b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>")

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
