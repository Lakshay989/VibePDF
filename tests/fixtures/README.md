# Test fixtures — index

Small, hand-generated, **committed** PDFs the automated suites depend on. Every
one is deterministic: a `generate-*.py` beside it rebuilds it byte-for-byte with
zero external dependencies. Provenance for anything not generated here is in
[`PROVENANCE.md`](PROVENANCE.md).

⚠️ **Never open these in the app for manual testing.** VibePDF saves in place, so
a manual edit clobbers the committed fixture. Copy into `Sample PDFs/scratch/`
first — see [`Sample PDFs/README.md`](../../Sample%20PDFs/README.md).

## Why the paths look flat

Test files reference these by literal relative path — `hello.pdf` alone appears
in **62** of them. The layout is kept flat on purpose: a tidier tree would be a
rename across the entire suite for no behavioural gain. This index is the
findability fix instead.

## `basic/` — the working set

| Fixture | Pages | What makes it interesting | Mainly used by |
|---|---|---|---|
| `hello.pdf` | 1 | The baseline. One US-Letter page, "Hello, VibePDF." in Helvetica. Almost every write test starts here. | ~62 files |
| `many-pages.pdf` | 50 | 50 pages, each "Page N". The perf fixture — watermark <2 s acceptance (P4-EDIT-009). | Track D, perf |
| `links.pdf` | 3 | Page 1 has a `/Link` whose `/Dest` targets page 3's **object**, not its index — so deleting a page must not dangle it. | P2-PAGE-003 |
| `bookmarks.pdf` | 6 | Top-level outline of 3 bookmarks → pages 1, 3, 5. | P2-PAGE-007 split-by-bookmark |
| `annots.pdf` | 1 | A `/Square` markup annotation. No page destination, so it survives page import *and* the dangling-reference sweep. | annotation round-trips |
| `rotated.pdf` | 4 | `/Rotate` 0 / 90 / 180 / 270, one per page. | P4.HF hardening — decorations must respect rotation |
| `cropped.pdf` | 1 | `/CropBox` strictly inside `/MediaBox` (print bleed). Viewers show only the CropBox. | P4.HF hardening — decorations must respect crop |
| `sample.jpg` | — | Not a PDF. The image-embed source. | image add / replace |

### Form fixtures (Phase 5)

| Fixture | Holds | Mainly used by |
|---|---|---|
| `forms.pdf` | One text field `name`, merged field/widget. The original COS spike fixture. | P2-PAGE-008 / P2.D1 |
| `forms-multi.pdf` | Three text fields: plain, `/MaxLen 5`, multi-line. | P5.A2 fill + tab navigation |
| `forms-buttons.pdf` | Checkbox `agree` (on-state `/Yes`) + radio group `color` (`/Red`, `/Green`), each with real `/AP` on/off appearances. | P5.A3 |
| `forms-choice.pdf` | Combo `fruit` (incl. a labelled `[chy Cherry]` export/display pair) + a multi-select list. | P5.A4 |
| `forms-xfa.pdf` | XFA-only: empty `/Fields` + an `/XFA` XDP packet. The "no AcroForm fallback" case. | P5.A5 degraded path, P5.C2 flatten |

## `acceptance/` — phase acceptance demos

Bigger, closer to real documents; several are **generated on demand and
git-ignored** (`tests/fixtures/acceptance/*.pdf` is in `.gitignore`). Rebuild:

```bash
python3 tests/fixtures/acceptance/generate.py     # deps: acceptance/requirements.txt
```

- `p1-encrypted.pdf` — password-protected, for the Phase 1 open flow.
- **Missing:** `p5-irs-w9.pdf`. Phase 5's acceptance demo wants a real IRS W-9;
  drop one in `Sample PDFs/` and it can be wired in.

## `fonts/`

`NotoSansCoptic-Regular.ttf` — a face with no WinAnsi coverage, so it forces the
embedded-CID path in the font tests. `OFL-NOTICE.txt` is its licence; keep them
together.

## `golden/`

`hello-p0-72dpi.png` — the reference raster for the render-comparison test. If a
render change is *intended*, regenerate it deliberately; never to make a test
pass.

## Adding a fixture

1. Write `basic/generate-<name>.py` — no dependencies, deterministic output.
2. Run it, commit **both** the script and the PDF.
3. Add a row to the table above, and a line to `PROVENANCE.md` if it came from
   anywhere but the script.
