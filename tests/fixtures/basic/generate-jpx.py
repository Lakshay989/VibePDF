#!/usr/bin/env python3
"""Regenerate tests/fixtures/basic/jpx.pdf.

One page holding a 16x16 RGB image compressed with /JPXDecode (JPEG 2000).
PDF.js decodes JPX through `openjpeg.wasm`, so this is the fixture proving the
view can load that module.

The one external tool: `opj_compress` from OpenJPEG (`brew install openjpeg`,
`apt install libopenjp2-tools`). A JPEG 2000 encoder is not a few lines of
Python, unlike the other generators here. It runs lossless (OpenJPEG's default
reversible 5/3 wavelet), so decoding must return `pixel()` exactly — the tests
compare against it. Output is byte-identical for a given OpenJPEG version; the
codestream carries OpenJPEG's version in a comment marker.

The pattern is four quadrants — red top-left, green top-right, blue
bottom-left, white bottom-right — so a flip, a mirror or a channel swap fails
the test instead of passing by coincidence.

Run from anywhere:
    python3 tests/fixtures/basic/generate-jpx.py
"""
import shutil
import subprocess
import tempfile
from pathlib import Path

OUT = Path(__file__).parent / "jpx.pdf"
SIZE = 16


def pixel(row: int, col: int) -> tuple[int, int, int]:
    top, left = row < SIZE // 2, col < SIZE // 2
    if top:
        return (255, 0, 0) if left else (0, 255, 0)
    return (0, 0, 255) if left else (255, 255, 255)


encoder = shutil.which("opj_compress")
if encoder is None:
    raise SystemExit("opj_compress not found — install OpenJPEG (brew install openjpeg)")

with tempfile.TemporaryDirectory() as tmp:
    ppm = Path(tmp) / "pattern.ppm"
    j2k = Path(tmp) / "pattern.j2k"
    rgb = bytes(v for r in range(SIZE) for c in range(SIZE) for v in pixel(r, c))
    # OpenJPEG's PNM reader wants the header on separate lines, and its default
    # of 6 resolution levels needs at least 32 pixels; 3 suits a 16x16 image.
    ppm.write_bytes(f"P6\n{SIZE} {SIZE}\n255\n".encode() + rgb)
    subprocess.run([encoder, "-i", str(ppm), "-o", str(j2k), "-n", "3"], check=True, capture_output=True)
    data = j2k.read_bytes()

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
    b"/ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /JPXDecode "
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
out += b"%PDF-1.5\n"
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
