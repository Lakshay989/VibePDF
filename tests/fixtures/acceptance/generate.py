#!/usr/bin/env python3
"""Generate Phase 1 acceptance fixtures.

The roadmap (docs/05_ROADMAP.md §Phase 1) calls for three large
fixtures that are too big to keep in git:

    p1-spec.pdf       1000-page text-heavy PDF (Lorem ipsum)
    p1-encrypted.pdf  encrypted copy of tests/fixtures/basic/hello.pdf
                       (user password: "vibepdf")
    p1-large.pdf      ~500MB filler PDF (NFR-PERF-003 stress test)

This script regenerates any or all of them on demand.

USAGE
    python3 tests/fixtures/acceptance/generate.py all
    python3 tests/fixtures/acceptance/generate.py spec --pages 1000
    python3 tests/fixtures/acceptance/generate.py encrypted
    python3 tests/fixtures/acceptance/generate.py large --size-mb 500

The spec + large fixtures are built from scratch in pure Python
(same minimal PDF builder as tests/fixtures/basic/generate-hello.py).
The encrypted fixture requires pypdf:

    pip install -r tests/fixtures/acceptance/requirements.txt
"""

from __future__ import annotations

import argparse
import sys
import textwrap
from pathlib import Path

ACCEPTANCE_DIR = Path(__file__).resolve().parent
HELLO_PDF = ACCEPTANCE_DIR.parent / "basic" / "hello.pdf"

LOREM = (
    "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod "
    "tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim "
    "veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex "
    "ea commodo consequat. Duis aute irure dolor in reprehenderit in voluptate "
    "velit esse cillum dolore eu fugiat nulla pariatur. Excepteur sint occaecat "
    "cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id "
    "est laborum."
)

ENCRYPTED_USER_PASSWORD = "vibepdf"
ENCRYPTED_OWNER_PASSWORD = "vibepdf-owner"


# ----------------------------------------------------------------------------
# Minimal PDF builder (shared across spec + large generators)
# ----------------------------------------------------------------------------


def _escape_pdf_text(s: str) -> str:
    """Escape PDF string literal contents (between parens)."""
    return s.replace("\\", "\\\\").replace("(", "\\(").replace(")", "\\)")


def build_pdf(pages_content: list[bytes]) -> bytes:
    """
    Build a valid PDF 1.4 with N pages, each using the given content stream.

    Object layout (1-indexed, as PDFs expect):
        1: Catalog
        2: Pages
        3: Font (Helvetica)
        4: Page 1 dict
        5: Page 1 content stream
        6: Page 2 dict
        7: Page 2 content stream
        ...

    The serializer is hand-rolled; no external libraries.
    """
    n = len(pages_content)
    total_objects = 3 + 2 * n
    objects: list[bytes] = [b""] * total_objects

    page_dict_objnums = [4 + i * 2 for i in range(n)]
    kids = " ".join(f"{num} 0 R" for num in page_dict_objnums)

    objects[0] = b"<< /Type /Catalog /Pages 2 0 R >>"
    objects[1] = f"<< /Type /Pages /Kids [{kids}] /Count {n} >>".encode()
    objects[2] = b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>"

    for i, content in enumerate(pages_content):
        page_objnum = 4 + i * 2
        content_objnum = 5 + i * 2
        page_dict = (
            f"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
            f"/Resources << /Font << /F1 3 0 R >> >> "
            f"/Contents {content_objnum} 0 R >>"
        ).encode()
        stream = (
            b"<< /Length "
            + str(len(content)).encode()
            + b" >>\nstream\n"
            + content
            + b"\nendstream"
        )
        objects[page_objnum - 1] = page_dict
        objects[content_objnum - 1] = stream

    out = bytearray()
    out += b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n"
    offsets: list[int] = []
    for i, body in enumerate(objects, start=1):
        offsets.append(len(out))
        out += f"{i} 0 obj\n".encode() + body + b"\nendobj\n"
    xref_offset = len(out)
    out += b"xref\n"
    out += f"0 {total_objects + 1}\n".encode()
    out += b"0000000000 65535 f \n"
    for off in offsets:
        out += f"{off:010d} 00000 n \n".encode()
    out += b"trailer\n"
    out += f"<< /Size {total_objects + 1} /Root 1 0 R >>\n".encode()
    out += b"startxref\n"
    out += f"{xref_offset}\n".encode()
    out += b"%%EOF\n"
    return bytes(out)


# ----------------------------------------------------------------------------
# Content stream builders
# ----------------------------------------------------------------------------


def lorem_page_content(page_number: int, total_pages: int) -> bytes:
    """Build a single page of wrapped Lorem ipsum + a footer page number."""
    paragraphs = [LOREM] * 3
    wrapped: list[str] = []
    for p in paragraphs:
        wrapped.extend(textwrap.wrap(p, width=80))
        wrapped.append("")  # blank line between paragraphs

    cmds: list[bytes] = []
    cmds.append(b"BT /F1 11 Tf")
    y = 740
    for line in wrapped:
        if y < 80:
            break
        escaped = _escape_pdf_text(line)
        cmds.append(f"1 0 0 1 72 {y} Tm ({escaped}) Tj".encode())
        y -= 14
    cmds.append(b"ET")

    # Footer with page number
    footer = f"Page {page_number} of {total_pages}"
    cmds.append(b"BT /F1 9 Tf")
    cmds.append(f"1 0 0 1 72 40 Tm ({_escape_pdf_text(footer)}) Tj".encode())
    cmds.append(b"ET")
    return b"\n".join(cmds)


def filler_page_content(target_bytes: int) -> bytes:
    """Build a content stream of about `target_bytes` bytes of valid draw ops."""
    base = b"BT /F1 10 Tf 72 720 Td (Filler line for VibePDF perf test.) Tj ET\n"
    reps = max(1, target_bytes // len(base))
    return base * reps


# ----------------------------------------------------------------------------
# Generators
# ----------------------------------------------------------------------------


def generate_spec(pages: int) -> Path:
    out = ACCEPTANCE_DIR / "p1-spec.pdf"
    contents = [lorem_page_content(i + 1, pages) for i in range(pages)]
    out.write_bytes(build_pdf(contents))
    return out


def generate_large(target_size_mb: int) -> Path:
    out = ACCEPTANCE_DIR / "p1-large.pdf"
    target_bytes = target_size_mb * 1024 * 1024
    # Each page contributes its content stream (≈ per_page_bytes) plus a
    # small overhead (page dict, xref entry). The header/catalog/font are
    # fixed cost ~200 B. We aim for the right *total* by dividing into a
    # sensible number of pages × per-page content size.
    pages = max(50, target_size_mb)            # ~1 MB / page floor
    per_page_bytes = target_bytes // pages
    contents = [filler_page_content(per_page_bytes) for _ in range(pages)]
    out.write_bytes(build_pdf(contents))
    return out


def generate_encrypted() -> Path:
    """Encrypt tests/fixtures/basic/hello.pdf. Requires pypdf."""
    try:
        import pypdf  # type: ignore[import-not-found]
    except ImportError:
        print(
            "error: pypdf is required for the encrypted fixture.\n"
            "  pip install -r tests/fixtures/acceptance/requirements.txt",
            file=sys.stderr,
        )
        sys.exit(2)

    if not HELLO_PDF.exists():
        print(
            f"error: missing {HELLO_PDF}\n"
            "  regenerate with: python3 tests/fixtures/basic/generate-hello.py",
            file=sys.stderr,
        )
        sys.exit(2)

    reader = pypdf.PdfReader(str(HELLO_PDF))
    writer = pypdf.PdfWriter()
    for page in reader.pages:
        writer.add_page(page)
    writer.encrypt(
        user_password=ENCRYPTED_USER_PASSWORD,
        owner_password=ENCRYPTED_OWNER_PASSWORD,
    )
    out = ACCEPTANCE_DIR / "p1-encrypted.pdf"
    with out.open("wb") as f:
        writer.write(f)
    return out


# ----------------------------------------------------------------------------
# CLI
# ----------------------------------------------------------------------------


def fmt_size(n: int) -> str:
    if n >= 1024 * 1024:
        return f"{n / (1024 * 1024):.1f} MB"
    if n >= 1024:
        return f"{n / 1024:.1f} KB"
    return f"{n} B"


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description=(
            "Generate Phase 1 acceptance fixtures. "
            "See the file docstring for what each fixture is for."
        ),
    )
    sub = parser.add_subparsers(dest="cmd", required=True)

    sp_spec = sub.add_parser("spec", help="1000-page Lorem ipsum PDF")
    sp_spec.add_argument("--pages", type=int, default=1000)

    sub.add_parser("encrypted", help="Encrypted copy of hello.pdf")

    sp_large = sub.add_parser("large", help="Filler PDF sized to --size-mb")
    sp_large.add_argument("--size-mb", type=int, default=500)

    sp_all = sub.add_parser("all", help="Generate all three (smaller defaults)")
    sp_all.add_argument(
        "--quick",
        action="store_true",
        help="Use small sizes (5-page spec, 5MB large). Useful for CI smoke.",
    )

    args = parser.parse_args(argv)

    outputs: list[Path] = []
    if args.cmd == "spec":
        outputs.append(generate_spec(args.pages))
    elif args.cmd == "encrypted":
        outputs.append(generate_encrypted())
    elif args.cmd == "large":
        outputs.append(generate_large(args.size_mb))
    elif args.cmd == "all":
        if args.quick:
            outputs.append(generate_spec(5))
            outputs.append(generate_large(5))
        else:
            outputs.append(generate_spec(1000))
            outputs.append(generate_large(500))
        outputs.append(generate_encrypted())

    for p in outputs:
        print(f"  {p.name}: {fmt_size(p.stat().st_size)}  →  {p}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
