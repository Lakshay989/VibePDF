#!/usr/bin/env python3
"""Build `table.pdf` — the fixture P7-OCR-004's table clause is checked against.

Two pages, because generators draw a table's rules two different ways and a
detector has to survive both:

  * **page 1** draws each rule as its **own stroked line**, so every rule is a
    separate path object whose bounding box is thin in one axis;
  * **page 2** draws the **whole grid as one path** with many subpaths, so the
    object's bounding box is the size of the table and the rules can only be
    found by walking its segments.

Page 1 also carries a **decoy**: a lone horizontal rule under a heading, of the
kind that separates sections in ordinary documents. It has no verticals and
bounds nothing, and a detector that turns it into a table is worse than one
that finds no tables at all — so the fixture makes that failure visible.

Both pages carry body text above and below the table, which must still come out
as paragraphs, and whose text must appear exactly once.

Hand-written PDF 1.4, no dependencies, deterministic. ~3 KB.

Usage:  python3 tests/fixtures/basic/generate-table.py
"""

from __future__ import annotations

from pathlib import Path

TARGET = Path(__file__).resolve().parent / "table.pdf"
WIDTH, HEIGHT = 612, 792

# The grid: three columns, four rows (a header and three of data).
COLUMN_X = [72.0, 220.0, 360.0, 500.0]
ROW_Y = [640.0, 616.0, 592.0, 568.0, 544.0]     # top edge down to bottom edge

CELLS = [
    ["Region", "Revenue", "Change"],
    ["North", "412,000", "+6%"],
    ["South", "298,500", "-2%"],
    ["Overseas", "77,250", "+31%"],
]


def text(x: float, y: float, size: float, body: str, font: str = "FR") -> list[str]:
    escaped = body.replace("\\", r"\\").replace("(", r"\(").replace(")", r"\)")
    return [f"/{font} {size} Tf", f"1 0 0 1 {x:.1f} {y:.1f} Tm", f"({escaped}) Tj"]


def cell_text() -> list[str]:
    """The table's contents, one text object per cell."""
    out = ["BT"]
    for row, values in enumerate(CELLS):
        for column, value in enumerate(values):
            # 5 pt in from the cell's left edge, 7 pt up from its bottom.
            x = COLUMN_X[column] + 5
            y = ROW_Y[row + 1] + 7
            out += text(x, y, 9, value, "FB" if row == 0 else "FR")
    out.append("ET")
    return out


def separate_line_rules() -> list[str]:
    """Every rule as its own stroked path object."""
    out = ["0.75 w 0 G"]
    for y in ROW_Y:
        out.append(f"{COLUMN_X[0]:.1f} {y:.1f} m {COLUMN_X[-1]:.1f} {y:.1f} l S")
    for x in COLUMN_X:
        out.append(f"{x:.1f} {ROW_Y[0]:.1f} m {x:.1f} {ROW_Y[-1]:.1f} l S")
    return out


def single_path_rules() -> list[str]:
    """The whole grid as one path object with many subpaths, one stroke."""
    parts = ["0.75 w 0 G"]
    for y in ROW_Y:
        parts.append(f"{COLUMN_X[0]:.1f} {y:.1f} m {COLUMN_X[-1]:.1f} {y:.1f} l")
    for x in COLUMN_X:
        parts.append(f"{x:.1f} {ROW_Y[0]:.1f} m {x:.1f} {ROW_Y[-1]:.1f} l")
    parts.append("S")
    return parts


def transformed_rules() -> list[str]:
    """The grid drawn at half scale under a `cm`, so its path points are in the
    object's own space and only the matrix puts them on the page.

    Measured 2026-09-23: PDFium reports a segment's point *before* the object
    matrix is applied, while its bounding box is after. A detector that trusts
    the raw points mislocates every rule in a document that uses a transform —
    which is most of them — so one page here is built to catch exactly that.
    """
    parts = ["q 0.5 0 0 0.5 180 240 cm", "1.5 w 0 G"]
    for y in ROW_Y:
        parts.append(f"{COLUMN_X[0]:.1f} {y:.1f} m {COLUMN_X[-1]:.1f} {y:.1f} l")
    for x in COLUMN_X:
        parts.append(f"{x:.1f} {ROW_Y[0]:.1f} m {x:.1f} {ROW_Y[-1]:.1f} l")
    parts.append("S")
    parts.append("Q")
    return parts


def transformed_cell_text() -> list[str]:
    """The same cell text, under the same transform, so it lands in the cells."""
    out = ["q 0.5 0 0 0.5 180 240 cm", "BT"]
    for row, values in enumerate(CELLS):
        for column, value in enumerate(values):
            x = COLUMN_X[column] + 5
            y = ROW_Y[row + 1] + 7
            out += text(x, y, 18, value, "FB" if row == 0 else "FR")
    out += ["ET", "Q"]
    return out


def page_content(single_path: bool) -> bytes:
    out: list[str] = []
    out += ["BT"] + text(72, 720, 14, "Results by region", "FB") + ["ET"]
    if not single_path:
        # The decoy: a section rule under the heading. Two of these on a page
        # give two horizontal lines and no verticals — not a table.
        out.append("0.5 w 0 G")
        out.append(f"72 706 m 540 706 l S")
    out += ["BT"] + text(72, 676, 9, "The table below summarises the quarter.", "FR") + ["ET"]
    out += single_path_rules() if single_path else separate_line_rules()
    out += cell_text()
    out += ["BT"] + text(72, 516, 9, "Overseas growth came from two accounts.", "FR") + ["ET"]
    if not single_path:
        out.append("0.5 w 0 G")
        out.append(f"72 496 m 540 496 l S")
    return "\n".join(out).encode("latin-1")


def transformed_page() -> bytes:
    out: list[str] = []
    out += ["BT"] + text(72, 720, 14, "Results by region", "FB") + ["ET"]
    out += transformed_rules()
    out += transformed_cell_text()
    return "\n".join(out).encode("latin-1")


def main() -> None:
    page1, page2, page3 = page_content(False), page_content(True), transformed_page()
    objects: dict[int, bytes] = {
        1: b"<< /Type /Catalog /Pages 2 0 R >>",
        2: b"<< /Type /Pages /Kids [3 0 R 4 0 R 9 0 R] /Count 3 >>",
        3: (b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 %d %d] /Resources << /Font "
            b"<< /FR 7 0 R /FB 8 0 R >> >> /Contents 5 0 R >>" % (WIDTH, HEIGHT)),
        4: (b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 %d %d] /Resources << /Font "
            b"<< /FR 7 0 R /FB 8 0 R >> >> /Contents 6 0 R >>" % (WIDTH, HEIGHT)),
        5: b"<< /Length %d >>\nstream\n" % len(page1) + page1 + b"\nendstream",
        6: b"<< /Length %d >>\nstream\n" % len(page2) + page2 + b"\nendstream",
        7: b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
        8: b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold >>",
        9: (b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 %d %d] /Resources << /Font "
            b"<< /FR 7 0 R /FB 8 0 R >> >> /Contents 10 0 R >>" % (WIDTH, HEIGHT)),
        10: b"<< /Length %d >>\nstream\n" % len(page3) + page3 + b"\nendstream",
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
    print(f"wrote {TARGET} — {len(out):,} bytes, 3 pages")


if __name__ == "__main__":
    main()
