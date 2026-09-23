#!/usr/bin/env python3
"""Build `noisy-scan.pdf` — a scan that compression can actually act on.

SPEC: P7-OCR-010. The existing `scan.pdf` is 5.9 KB of near-white page, and
Flate already squeezes it to almost nothing: recompressing it as JPEG makes it
*larger*, which is a useful test of the "never keep a bigger result" rule but
tells you nothing about whether compression works.

A real scan is large for one reason — **sensor noise**. Every pixel jitters by a
few levels, Flate cannot model that at all, and JPEG discards it almost for
free. That one property is the whole of PDF compression for scanned documents,
so the fixture has to have it. Measured 2026-09-23 on the output of this script:

    20 pages, 300 DPI grayscale, Flate    81.6 MB   (4 MB a page — real-scan scale)
    the same pages as JPEG q85, 300 DPI   16.7 MB   (20.4%)
    the same pages as JPEG q85, 200 DPI    7.5 MB   ( 9.1%)
    the same pages as JPEG q60, 150 DPI    1.2 MB   ( 1.5%)

That size is why the output is **gitignored** rather than committed. Run this
script to produce it; `src-tauri/tests/compress.rs` skips its ratio tests with a
printed instruction when it is absent, and its correctness tests run against the
small committed fixtures regardless.

Deterministic: fixed seed, fixed Ghostscript raster. Same version in, same bytes
out. Takes ~30 s and writes ~80 MB.

Usage:  python3 tests/fixtures/basic/generate-noisy-scan.py [--pages N]
"""

from __future__ import annotations

import argparse
import random
import shutil
import subprocess
import sys
import tempfile
import zlib
from pathlib import Path

HERE = Path(__file__).resolve().parent
SOURCE = HERE / "many-pages.pdf"     # 50 pages of real text to photograph
TARGET = HERE / "noisy-scan.pdf"
DPI = 300
SEED = 7
# Peak jitter per pixel. 14 levels out of 255 is a mid-range desktop scanner:
# invisible to a reader, fatal to Flate.
NOISE = 14


def read_pgm(path: Path) -> tuple[int, int, bytes]:
    """Ghostscript's pgmraw: 'P5', width, height, maxval, then the samples.

    The header is whitespace-separated and may carry '#' comments, so it is
    parsed token by token rather than by splitting on newlines.
    """
    data = path.read_bytes()
    if data[:2] != b"P5":
        raise SystemExit(f"{path} is not a binary PGM")
    values: list[int] = []
    i = 2
    while len(values) < 3:
        while data[i : i + 1].isspace():
            i += 1
        if data[i : i + 1] == b"#":
            while data[i : i + 1] != b"\n":
                i += 1
            continue
        j = i
        while not data[j : j + 1].isspace():
            j += 1
        values.append(int(data[i:j]))
        i = j
    return values[0], values[1], data[i + 1 :]


def add_noise(pixels: bytes, rng: random.Random) -> bytes:
    """Subtract a tiled block of random jitter from every sample.

    A per-pixel `randint` call is the obvious way to write this and takes about
    ten minutes on 168 million pixels. Tiling a 64 KB random block has the same
    effect on compressibility — the point is that neighbouring pixels stop
    correlating — and runs in a second.
    """
    block = bytes(rng.randint(0, NOISE) for _ in range(65536))
    noise = (block * (len(pixels) // 65536 + 1))[: len(pixels)]
    return bytes(max(0, a - b) for a, b in zip(pixels, noise))


def build(pages: int) -> None:
    gs = shutil.which("gs") or shutil.which("gswin64c")
    if gs is None:
        raise SystemExit("Ghostscript ('gs') is required; install it and re-run")
    if not SOURCE.is_file():
        raise SystemExit(f"missing {SOURCE}; run generate-many.py first")

    rng = random.Random(SEED)
    with tempfile.TemporaryDirectory() as tmp:
        tmpdir = Path(tmp)
        subprocess.run(
            [gs, "-q", "-dNOPAUSE", "-dBATCH", "-sDEVICE=pgmraw", f"-r{DPI}",
             "-dFirstPage=1", f"-dLastPage={pages}",
             f"-sOutputFile={tmpdir}/pg%03d.pgm", str(SOURCE)],
            check=True,
        )

        objects: list[tuple[int, bytes]] = []
        page_ids: list[int] = []
        for index, pgm in enumerate(sorted(tmpdir.glob("pg*.pgm"))):
            width, height, pixels = read_pgm(pgm)
            body = zlib.compress(add_noise(pixels, rng), 9)
            image_id, content_id, page_id = 4 + index * 3, 5 + index * 3, 6 + index * 3
            pt_w, pt_h = width * 72 // DPI, height * 72 // DPI
            objects.append((image_id,
                b"<< /Type /XObject /Subtype /Image /Width %d /Height %d "
                b"/ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /FlateDecode "
                b"/Length %d >>\nstream\n" % (width, height, len(body)) + body + b"\nendstream"))
            content = b"q %d 0 0 %d 0 0 cm /Im0 Do Q" % (pt_w, pt_h)
            objects.append((content_id,
                b"<< /Length %d >>\nstream\n" % len(content) + content + b"\nendstream"))
            objects.append((page_id,
                b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 %d %d] /Resources "
                b"<< /XObject << /Im0 %d 0 R >> >> /Contents %d 0 R >>"
                % (pt_w, pt_h, image_id, content_id)))
            page_ids.append(page_id)

    kids = b" ".join(b"%d 0 R" % i for i in page_ids)
    everything = sorted(
        [(1, b"<< /Type /Catalog /Pages 2 0 R >>"),
         (2, b"<< /Type /Pages /Kids [%s] /Count %d >>" % (kids, len(page_ids)))]
        + objects
    )

    out = bytearray(b"%PDF-1.4\n")
    offsets: dict[int, int] = {}
    for number, body in everything:
        offsets[number] = len(out)
        out += b"%d 0 obj\n" % number + body + b"\nendobj\n"
    xref_at = len(out)
    top = max(offsets) + 1
    out += b"xref\n0 %d\n0000000000 65535 f \n" % top
    for n in range(1, top):
        out += b"%010d 00000 n \n" % offsets.get(n, 0)
    out += b"trailer\n<< /Size %d /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n" % (top, xref_at)

    TARGET.write_bytes(bytes(out))
    print(f"wrote {TARGET} — {len(page_ids)} pages, {len(out):,} bytes "
          f"({len(out) // max(1, len(page_ids)):,} a page)")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pages", type=int, default=20, help="pages to scan (default 20)")
    args = parser.parse_args()
    if not 1 <= args.pages <= 50:
        sys.exit("--pages must be between 1 and 50 (many-pages.pdf has 50)")
    build(args.pages)
