#!/usr/bin/env python3
"""Regenerate the scanned-page fixtures — pages that are a *picture* of text,
i.e. what a scanner produces and what OCR exists to read.

Three files, one family, because P7-OCR-001's hard part is putting the
invisible text where the ink is, and only variants prove that:

  scan.pdf          upright
  scan-rotated.pdf  the raster turned on its side on a landscape page, with
                    /Rotate 270 turning it back — what a scanner produces when
                    the sheet went in rotated: sideways ink, upright display
  scan-skewed.pdf   the text drawn 3 degrees off level before rasterising,
                    as a page fed crooked through a scanner comes out

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
import math
import shutil
import subprocess
import tempfile
import zlib
from pathlib import Path

HERE = Path(__file__).parent
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


def build_text_pdf(path: Path, skew_degrees: float = 0.0) -> None:
    """A minimal PDF with LINES set in Helvetica, one line each.

    `skew_degrees` tilts every line through the text matrix, which after
    rasterising is indistinguishable from a page scanned crooked.
    """
    cos, sin = math.cos(math.radians(skew_degrees)), math.sin(math.radians(skew_degrees))
    drawn = []
    y = PAGE_HEIGHT - 120
    for size, text in LINES:
        drawn.append(
            f"BT /F1 {size} Tf {cos:.6f} {sin:.6f} {-sin:.6f} {cos:.6f} 72 {y} Tm ({text}) Tj ET"
        )
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


def rasterise(skew_degrees: float) -> tuple[int, int, bytes]:
    with tempfile.TemporaryDirectory() as tmp:
        text_pdf = Path(tmp) / "text.pdf"
        raster = Path(tmp) / "page.pgm"
        build_text_pdf(text_pdf, skew_degrees)
        subprocess.run(
            [gs, "-q", "-dNOPAUSE", "-dBATCH", "-sDEVICE=pgmraw", f"-r{SCAN_DPI}",
             f"-sOutputFile={raster}", str(text_pdf)],
            check=True,
            capture_output=True,
        )
        return read_pgm(raster)


def turn_clockwise(width: int, height: int, pixels: bytes) -> tuple[int, int, bytes]:
    """Rotate the raster 90 degrees clockwise: row y becomes column (h-1-y)."""
    out = bytearray(len(pixels))
    for y in range(height):
        row = pixels[y * width : (y + 1) * width]
        col = height - 1 - y
        for x, value in enumerate(row):
            out[x * height + col] = value
    return height, width, bytes(out)


def write_scan(name: str, skew_degrees: float = 0.0, rotate: int = 0) -> None:
    width, height, pixels = rasterise(skew_degrees)
    assert len(pixels) == width * height, f"{len(pixels)} bytes for {width}x{height}"
    page_width, page_height = PAGE_WIDTH, PAGE_HEIGHT
    if rotate:
        # Sideways ink on a landscape page; /Rotate brings it back upright for
        # the reader, so OCR sees upright text and the text layer has to be
        # written in the page's own (sideways) space.
        width, height, pixels = turn_clockwise(width, height, pixels)
        page_width, page_height = PAGE_HEIGHT, PAGE_WIDTH
    compressed = zlib.compress(pixels, 9)

    image = (
        b"<< /Type /XObject /Subtype /Image "
        + f"/Width {width} /Height {height} ".encode()
        + b"/ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /FlateDecode "
        + b"/Length " + str(len(compressed)).encode() + b" >>\nstream\n"
        + compressed + b"\nendstream"
    )
    content = f"q {page_width} 0 0 {page_height} 0 0 cm /Im1 Do Q".encode()
    rotate_entry = f"/Rotate {rotate} " if rotate else ""

    objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        (
            f"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {page_width} {page_height}] "
            f"{rotate_entry}"
            "/Resources << /XObject << /Im1 4 0 R >> >> /Contents 5 0 R >>"
        ).encode(),
        image,
        b"<< /Length " + str(len(content)).encode() + b" >>\nstream\n" + content + b"\nendstream",
    ]

    out = HERE / name
    out.write_bytes(serialise(objects, b"%PDF-1.4\n"))
    print(f"wrote {out} ({out.stat().st_size} bytes), {width}x{height} at {SCAN_DPI} DPI")


write_scan("scan.pdf")
write_scan("scan-rotated.pdf", rotate=270)
write_scan("scan-skewed.pdf", skew_degrees=3.0)
