#!/usr/bin/env python3
"""Regenerate tests/fixtures/basic/scan.pdf — a page that is a *picture* of
text, i.e. what a scanner produces and what OCR exists to read.

Built in two steps:
  1. lay out WORDS as real text in a temporary PDF (built here, no deps);
  2. rasterise that page to 8-bit grayscale at SCAN_DPI with Ghostscript and
     embed the raster as the only content of the committed fixture.

So the fixture contains no text objects at all — `pdftotext` on it yields
nothing, which is exactly the starting condition P7-OCR-001 describes. It is
rendered at 150 DPI on purpose: below the 300 DPI floor in P7-OCR-003, so the
upscale stage runs in the OCR smoke test.

The one external tool: `ghostscript` (`brew install ghostscript`,
`apt install ghostscript`). Rasterising text without a renderer is not a few
lines of Python. Output is byte-identical for a given Ghostscript version.

Run from anywhere:
    python3 tests/fixtures/basic/generate-scan.py
"""
import shutil
import subprocess
import tempfile
import zlib
from pathlib import Path

OUT = Path(__file__).parent / "scan.pdf"
SCAN_DPI = 150
PAGE_WIDTH, PAGE_HEIGHT = 612, 792  # US Letter, points

# What the tests assert OCR reads back: a heading plus body text, because size
# is what makes a preprocessing stage safe or harmful. A first version set
# everything at 28 pt, and at that size *any* pipeline passed — including a
# 3x3 median filter that destroyed 9 pt text (measured 2026-09-21: "brown" →
# "broval"). Body lines at 9 pt keep the fixture honest.
#
# The words themselves are ordinary and unambiguous: an OCR test that argues
# about "rn" versus "m" is testing Tesseract, not us.
LINES = [
    (24, "VibePDF scanned page fixture"),
    (9, "The quick brown fox jumps over the lazy dog"),
    (9, "Invoice number 4821 dated 14 March 2026"),
    (9, "Total due within thirty days of receipt"),
]


def build_text_pdf(path: Path) -> None:
    """A minimal PDF with LINES set in Helvetica, one line each."""
    drawn = []
    y = PAGE_HEIGHT - 120
    for size, text in LINES:
        drawn.append(f"BT /F1 {size} Tf 72 {y} Td ({text}) Tj ET")
        y -= size * 2.5
    stream = "\n".join(drawn).encode()

    objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        (
            f"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {PAGE_WIDTH} {PAGE_HEIGHT}] "
            "/Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>"
        ).encode(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
    ]
    objects.append(
        b"<< /Length " + str(len(stream)).encode() + b" >>\nstream\n" + stream + b"\nendstream"
    )
    path.write_bytes(serialise(objects, b"%PDF-1.4\n"))


def serialise(objects: list[bytes], header: bytes) -> bytes:
    out = bytearray(header)
    out += b"%\xe2\xe3\xcf\xd3\n"  # binary-file marker
    offsets = []
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
    return bytes(out)


def read_pgm(path: Path) -> tuple[int, int, bytes]:
    """Ghostscript's pgmraw output: 'P5', width, height, maxval, then bytes."""
    data = path.read_bytes()
    fields = []
    at = 0
    while len(fields) < 4:
        while data[at : at + 1].isspace():
            at += 1
        if data[at : at + 1] == b"#":  # comment line
            at = data.index(b"\n", at) + 1
            continue
        start = at
        while not data[at : at + 1].isspace():
            at += 1
        fields.append(data[start:at])
    assert fields[0] == b"P5", f"expected a raw PGM, got {fields[0]!r}"
    return int(fields[1]), int(fields[2]), data[at + 1 :]


gs = shutil.which("gs") or shutil.which("gswin64c")
if gs is None:
    raise SystemExit("ghostscript not found — install it (brew install ghostscript)")

with tempfile.TemporaryDirectory() as tmp:
    text_pdf = Path(tmp) / "text.pdf"
    raster = Path(tmp) / "page.pgm"
    build_text_pdf(text_pdf)
    subprocess.run(
        [gs, "-q", "-dNOPAUSE", "-dBATCH", "-sDEVICE=pgmraw", f"-r{SCAN_DPI}",
         f"-sOutputFile={raster}", str(text_pdf)],
        check=True,
        capture_output=True,
    )
    width, height, pixels = read_pgm(raster)

assert len(pixels) == width * height, f"{len(pixels)} bytes for {width}x{height}"
compressed = zlib.compress(pixels, 9)

image = (
    b"<< /Type /XObject /Subtype /Image "
    + f"/Width {width} /Height {height} ".encode()
    + b"/ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /FlateDecode "
    + b"/Length " + str(len(compressed)).encode() + b" >>\nstream\n"
    + compressed + b"\nendstream"
)
content = f"q {PAGE_WIDTH} 0 0 {PAGE_HEIGHT} 0 0 cm /Im1 Do Q".encode()

objects = [
    b"<< /Type /Catalog /Pages 2 0 R >>",
    b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
    f"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {PAGE_WIDTH} {PAGE_HEIGHT}] "
    "/Resources << /XObject << /Im1 4 0 R >> >> /Contents 5 0 R >>".encode(),
    image,
    b"<< /Length " + str(len(content)).encode() + b" >>\nstream\n" + content + b"\nendstream",
]

OUT.write_bytes(serialise(objects, b"%PDF-1.4\n"))
print(f"wrote {OUT} ({OUT.stat().st_size} bytes), {width}x{height} at {SCAN_DPI} DPI")
