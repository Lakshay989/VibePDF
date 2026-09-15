#!/usr/bin/env python3
"""Regenerate tests/fixtures/basic/ccitt.pdf.

One page holding a 16x16 bilevel image compressed with /CCITTFaxDecode
(Group 3, one-dimensional: `/K 0`). PDF.js decodes CCITT through the same
WebAssembly module as JBIG2 (`jbig2.wasm`), so this is the fixture proving the
view can load that module; JBIG2 itself can't be encoded without a library.

The pattern is deliberately asymmetric, so a flipped, mirrored or inverted
decode fails the test instead of passing by coincidence:
  - rows 2-5:  black from column 1 to 10
  - rows 9-13: black from column 8 to 14
  - everything else white.
`BLACK` below is the source of truth the tests check decoded pixels against.

The encoder is a few lines because every row is at most white-black-white with
runs under 64 pixels, so only the ITU-T T.4 terminating codes are needed. No
external deps.

Run from anywhere:
    python3 tests/fixtures/basic/generate-ccitt.py
"""
from pathlib import Path

OUT = Path(__file__).parent / "ccitt.pdf"
SIZE = 16

# (first_row, last_row, first_col, last_col), inclusive.
BLACK = [(2, 5, 1, 10), (9, 13, 8, 14)]

# ITU-T T.4 terminating codes, run lengths 0-16.
WHITE = ["00110101", "000111", "0111", "1000", "1011", "1100", "1110", "1111",
         "10011", "10100", "00111", "01000", "001000", "000011", "110100",
         "110101", "101010"]
BLACK_CODES = ["0000110111", "010", "11", "10", "011", "0011", "0010", "00011",
               "000101", "000100", "0000100", "0000101", "0000111", "00000100",
               "00000111", "000011000", "0000010111"]


def is_black(row: int, col: int) -> bool:
    return any(r0 <= row <= r1 and c0 <= col <= c1 for r0, r1, c0, c1 in BLACK)


def encode_row(row: int) -> str:
    """Alternating runs, always starting with white (possibly of length 0)."""
    bits, col, colour = "", 0, False
    while col < SIZE:
        run = 0
        while col + run < SIZE and is_black(row, col + run) == colour:
            run += 1
        bits += (BLACK_CODES if colour else WHITE)[run]
        col += run
        colour = not colour
    return bits


bitstring = "".join(encode_row(r) for r in range(SIZE))
bitstring += "0" * (-len(bitstring) % 8)
data = int(bitstring, 2).to_bytes(len(bitstring) // 8, "big")

objects: list[bytes] = []


def add(body: bytes) -> int:
    objects.append(body)
    return len(objects)


catalog = b"<< /Type /Catalog /Pages 2 0 R >>"
pages = b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>"
page = (
    b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 160 160] "
    b"/Resources << /XObject << /Im1 4 0 R >> >> /Contents 5 0 R >>"
)
image = (
    b"<< /Type /XObject /Subtype /Image /Width 16 /Height 16 "
    b"/ColorSpace /DeviceGray /BitsPerComponent 1 /Filter /CCITTFaxDecode "
    b"/DecodeParms << /K 0 /Columns 16 /Rows 16 /EndOfBlock false >> "
    b"/Length " + str(len(data)).encode() + b" >>\nstream\n" + data + b"\nendstream"
)
stream = b"q 160 0 0 160 0 0 cm /Im1 Do Q"
contents = (
    b"<< /Length " + str(len(stream)).encode() + b" >>\nstream\n"
    + stream + b"\nendstream"
)

for obj in (catalog, pages, page, image, contents):
    add(obj)

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
