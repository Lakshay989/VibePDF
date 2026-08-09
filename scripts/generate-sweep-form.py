#!/usr/bin/env python3
"""Generate the Phase 5 manual-sweep forms into the git-ignored `Sample PDFs/`.

The automated suites cover the Rust; what they can't cover is the *app* — a dead
overlay, a missed epoch bump, a panel that never mounts all pass a green test
run. `steps/P5-SWEEP.md` is the checklist for that pass, and this script builds
the two files it works through:

  Sample PDFs/sweep/p5-sweep-form.pdf   one page, one field of every fillable
                                        kind, all blank — A1 through C2 in a
                                        single file so the sweep is one pass.
  Sample PDFs/sweep/p5-sweep-xfa.pdf    an XFA-carrying form for A5's degraded
                                        path (XFA and a real AcroForm can't
                                        coexist meaningfully, hence two files).

The output is deliberately NOT a committed fixture: the sweep edits and saves it
in place, and VibePDF writes next to the file it opened. This script is
committed so the assets can always be rebuilt; the assets themselves are not.

    python3 scripts/generate-sweep-form.py

No external dependencies — raw PDF 1.4 object graphs, the same hand-rolled shape
as `tests/fixtures/basic/generate-forms-*.py`.
"""

from pathlib import Path

OUT_DIR = Path(__file__).resolve().parent.parent / "Sample PDFs" / "sweep"


def build(objects: list[bytes]) -> bytes:
    """Serialise a 1-based object list into a PDF with a correct xref table."""
    out = bytearray(b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n")
    offsets: list[int] = []
    for i, body in enumerate(objects, start=1):
        offsets.append(len(out))
        out += f"{i} 0 obj\n".encode() + body + b"\nendobj\n"
    xref_offset = len(out)
    out += b"xref\n" + f"0 {len(objects) + 1}\n".encode()
    out += b"0000000000 65535 f \n"
    for off in offsets:
        out += f"{off:010d} 00000 n \n".encode()
    out += b"trailer\n" + f"<< /Size {len(objects) + 1} /Root 1 0 R >>\n".encode()
    out += b"startxref\n" + f"{xref_offset}\n".encode() + b"%%EOF\n"
    return bytes(out)


def stream(dict_body: bytes, data: bytes) -> bytes:
    """A stream object: `dict_body` gets its /Length filled in."""
    return dict_body + b" /Length " + str(len(data)).encode() + b" >>\nstream\n" + data + b"\nendstream"


def label(x: int, y: int, text: str, size: int = 10) -> bytes:
    return f"BT /F1 {size} Tf {x} {y} Td ({text}) Tj ET ".encode()


# ── the sweep form ──────────────────────────────────────────────────────────
#
# Object map (1-based, must stay in sync with the references below):
#   1 catalog        2 pages          3 page           4 acroform
#   5 font           6 contents       7 on-AP          8 off-AP
#   9 text-plain    10 text-maxlen   11 text-multiline
#  12 checkbox      13 radio group   14 radio Red     15 radio Green
#  16 combo         17 list

# Layout rule: every label sits ABOVE its widget with a clear gap, and no label
# shares an x-range with a widget rect. The first cut violated both for the radio
# group and the list box — the label was drawn *inside* the widget, which made
# the Red option unclickable (the widget was under the text) and the list look
# like it had garbled contents. Keep the bands below disjoint when editing.
page_text = (
    label(72, 750, "VibePDF - Phase 5 sweep form", 16)
    + label(72, 730, "Every fillable field kind, blank. Work through steps/P5-SWEEP.md.")
    + label(72, 700, "1. Full name (plain text)")
    + label(72, 648, "2. Code (max 5 chars)")
    + label(72, 596, "3. Notes (multi-line)")
    + label(72, 492, "4. Agree (checkbox)")
    + label(72, 440, "5. Colour (radio: Red / Green)")
    + label(118, 420, "Red")
    + label(118, 390, "Green")
    + label(72, 356, "6. Fruit (combo / dropdown)")
    + label(72, 300, "7. Tags (list) - hold Cmd to select more than one")
    + label(72, 190, "Empty space below - drag new fields here for B1 / B2.")
)

on_face = b"q 0 0 0 rg 3 3 12 12 re f Q"

sweep = [
    b"<< /Type /Catalog /Pages 2 0 R /AcroForm 4 0 R >>",
    b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
    b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
    b"/Resources << /Font << /F1 5 0 R >> >> /Contents 6 0 R "
    b"/Annots [9 0 R 10 0 R 11 0 R 12 0 R 14 0 R 15 0 R 16 0 R 17 0 R] >>",
    b"<< /Fields [9 0 R 10 0 R 11 0 R 12 0 R 13 0 R 16 0 R 17 0 R] "
    b"/DR << /Font << /F1 5 0 R >> >> /DA (/F1 0 Tf 0 g) >>",
    b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
    stream(b"<<", page_text),
    stream(b"<< /Type /XObject /Subtype /Form /BBox [0 0 18 18]", on_face),
    stream(b"<< /Type /XObject /Subtype /Form /BBox [0 0 18 18]", b""),
    # 9 — plain text
    b"<< /Type /Annot /Subtype /Widget /FT /Tx /T (fullName) /TU (Your full name) "
    b"/Rect [72 672 400 696] /DA (/F1 12 Tf 0 g) /P 3 0 R >>",
    # 10 — /MaxLen 5
    b"<< /Type /Annot /Subtype /Widget /FT /Tx /T (code) /MaxLen 5 "
    b"/Rect [72 620 200 644] /DA (/F1 12 Tf 0 g) /P 3 0 R >>",
    # 11 — multi-line (/Ff bit 13 = 4096)
    b"<< /Type /Annot /Subtype /Widget /FT /Tx /T (notes) /Ff 4096 "
    b"/Rect [72 520 400 592] /DA (/F1 11 Tf 0 g) /P 3 0 R >>",
    # 12 — checkbox, on-state /Yes
    b"<< /Type /Annot /Subtype /Widget /FT /Btn /T (agree) "
    b"/Rect [72 466 90 484] /AS /Off /AP << /N << /Yes 7 0 R /Off 8 0 R >> >> /P 3 0 R >>",
    # 13 — radio group (/Ff bit 16 = 32768)
    b"<< /FT /Btn /Ff 32768 /T (colour) /V /Off /Kids [14 0 R 15 0 R] >>",
    b"<< /Type /Annot /Subtype /Widget /Parent 13 0 R /Rect [92 414 110 432] "
    b"/AS /Off /AP << /N << /Red 7 0 R /Off 8 0 R >> >> /P 3 0 R >>",
    b"<< /Type /Annot /Subtype /Widget /Parent 13 0 R /Rect [92 384 110 402] "
    b"/AS /Off /AP << /N << /Green 7 0 R /Off 8 0 R >> >> /P 3 0 R >>",
    # 16 — combo (/Ff bit 18 = 131072)
    b"<< /Type /Annot /Subtype /Widget /FT /Ch /Ff 131072 /T (fruit) "
    b"/Opt [(Apple) (Banana) (Cherry)] /Rect [72 328 300 352] "
    b"/DA (/F1 12 Tf 0 g) /P 3 0 R >>",
    # 17 — list, multi-select (/Ff bit 22 = 2097152)
    b"<< /Type /Annot /Subtype /Widget /FT /Ch /Ff 2097152 /T (tags) "
    b"/Opt [(urgent) (review) (archive)] /Rect [72 220 300 296] "
    b"/DA (/F1 12 Tf 0 g) /P 3 0 R >>",
]

# ── the XFA form (A5's degraded path) ───────────────────────────────────────
#
# An /XFA entry on the AcroForm with no usable AcroForm fields — what a
# "XFA-only" form looks like to us. The app should refuse to edit it and offer
# the flatten-to-read-only conversion instead.

xfa_packet = (
    b'<?xml version="1.0" encoding="UTF-8"?>\n'
    b'<xdp:xdp xmlns:xdp="http://ns.adobe.com/xdp/">\n'
    b"  <template xmlns=\"http://www.xfa.org/schema/xfa-template/3.0/\">\n"
    b'    <subform name="sweep"><field name="xfaOnly"/></subform>\n'
    b"  </template>\n"
    b"</xdp:xdp>\n"
)

xfa_text = (
    label(72, 750, "VibePDF - Phase 5 sweep: XFA form", 16)
    + label(72, 720, "This form is XFA-only. VibePDF should say so and offer")
    + label(72, 704, "'Convert XFA to flat content (read-only)' - never a fill UI.")
)

xfa = [
    b"<< /Type /Catalog /Pages 2 0 R /AcroForm 4 0 R >>",
    b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
    b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
    b"/Resources << /Font << /F1 5 0 R >> >> /Contents 6 0 R >>",
    b"<< /Fields [] /XFA 7 0 R /DR << /Font << /F1 5 0 R >> >> >>",
    b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
    stream(b"<<", xfa_text),
    stream(b"<<", xfa_packet),
]

if __name__ == "__main__":
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    for name, objects in (("p5-sweep-form.pdf", sweep), ("p5-sweep-xfa.pdf", xfa)):
        data = build(objects)
        (OUT_DIR / name).write_bytes(data)
        print(f"wrote {OUT_DIR / name} ({len(data)} bytes)")
