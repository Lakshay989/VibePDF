#!/usr/bin/env python3
"""Build `report.pdf` — the fixture P7-OCR-004 (PDF → DOCX) is checked against.

A DOCX conversion has four things to preserve, and none of the existing
fixtures has more than one of them:

  * **headings**, which exist only as type that is larger than its neighbours —
    there is no heading flag in a PDF;
  * **bold and italic**, set here in the base-14 `Helvetica-Bold` and
    `Helvetica-Oblique` so the converter has real font names and flags to read;
  * **an image**, to land in `word/media/`;
  * **characters that break XML** (`&`, `<`, `>`, `"`), because the text goes
    straight into `document.xml` and an unescaped ampersand makes a file that
    every reader refuses to open. A fixture without them lets that ship.

Three heading sizes (20, 14, 11 pt) over 9 pt body text, so heading *levels*
can be told apart rather than merely detected. Hand-written PDF 1.4, no
dependencies, deterministic. ~2 KB.

Usage:  python3 tests/fixtures/basic/generate-report.py
"""

from __future__ import annotations

import zlib
from pathlib import Path

TARGET = Path(__file__).resolve().parent / "report.pdf"
WIDTH, HEIGHT = 612, 792

# (text, font, size) — font 'B' = Helvetica-Bold, 'I' = Oblique, 'R' = regular.
LINES: list[tuple[str, str, float]] = [
    ("Quarterly Report", "B", 20),
    ("Revenue & Costs", "B", 14),
    ("The quarter closed ahead of plan. Margin held at 31% despite", "R", 9),
    ("higher freight costs in March and April.", "R", 9),
    ("Risks", "B", 14),
    ("Supply is the main risk: lead times remain > 12 weeks, and the", "R", 9),
    ('single-source parts are still marked "critical" in the register.', "R", 9),
    # Blank entries are paragraph breaks: they advance y without drawing, so
    # the two sentences below are genuinely separate paragraphs in the page's
    # own geometry rather than two lines of one wrapped paragraph.
    ("", "R", 9),
    ("This sentence is emphasised.", "I", 9),
    ("", "R", 9),
    ("This sentence is strong.", "B", 9),
    ("Outlook", "B", 11),
    ("We expect <5% variance against the full-year forecast.", "R", 9),
]


def escape(text: str) -> bytes:
    """PDF string escaping — parentheses and backslashes only."""
    out = text.replace("\\", r"\\").replace("(", r"\(").replace(")", r"\)")
    return out.encode("latin-1")


def build_image() -> tuple[int, int, bytes]:
    """A 64x48 RGB gradient with a dark border — recognisable when it lands in Word."""
    w, h = 64, 48
    rows = bytearray()
    for y in range(h):
        for x in range(w):
            border = x < 2 or y < 2 or x >= w - 2 or y >= h - 2
            rows += bytes((0, 0, 0)) if border else bytes((x * 4 % 256, y * 5 % 256, 128))
    return w, h, zlib.compress(bytes(rows), 9)


def main() -> None:
    img_w, img_h, img_data = build_image()

    content = ["BT"]
    y = HEIGHT - 72
    for text, font, size in LINES:
        if not text:
            y -= size * 1.4      # a blank line: space, no ink
            continue
        content.append(f"/F{font} {size} Tf")
        content.append(f"1 0 0 1 72 {y:.0f} Tm")
        content.append(f"({escape(text).decode('latin-1')}) Tj")
        # Headings get air above them, which is also what makes the paragraph
        # breaker able to see them as separate blocks.
        y -= size * (2.2 if size > 9 else 1.4)
    content.append("ET")
    # The image, below the text, drawn at 128x96 pt (twice its pixel size).
    content.append(f"q 128 0 0 96 72 {y - 110:.0f} cm /Im0 Do Q")
    stream = "\n".join(content).encode("latin-1")

    objects: dict[int, bytes] = {
        1: b"<< /Type /Catalog /Pages 2 0 R >>",
        2: b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        3: (
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 %d %d] /Resources << /Font "
            b"<< /FR 5 0 R /FB 6 0 R /FI 7 0 R >> /XObject << /Im0 8 0 R >> >> "
            b"/Contents 4 0 R >>" % (WIDTH, HEIGHT)
        ),
        4: b"<< /Length %d >>\nstream\n" % len(stream) + stream + b"\nendstream",
        5: b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
        6: b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold >>",
        7: b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Oblique >>",
        8: (
            b"<< /Type /XObject /Subtype /Image /Width %d /Height %d /ColorSpace "
            b"/DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode /Length %d >>\nstream\n"
            % (img_w, img_h, len(img_data))
        ) + img_data + b"\nendstream",
    }

    out = bytearray(b"%PDF-1.4\n")
    offsets: dict[int, int] = {}
    for number in sorted(objects):
        offsets[number] = len(out)
        out += b"%d 0 obj\n" % number + objects[number] + b"\nendobj\n"
    xref_at = len(out)
    top = max(offsets) + 1
    out += b"xref\n0 %d\n0000000000 65535 f \n" % top
    for n in range(1, top):
        out += b"%010d 00000 n \n" % offsets[n]
    out += b"trailer\n<< /Size %d /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n" % (top, xref_at)

    TARGET.write_bytes(bytes(out))
    print(f"wrote {TARGET} — {len(out):,} bytes")


if __name__ == "__main__":
    main()
