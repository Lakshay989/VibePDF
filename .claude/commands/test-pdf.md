---
description: Run the standard PDF regression suite against a file and report what breaks.
---

Run the PDF correctness suite against the file: **$ARGUMENTS** (default: all fixtures in `tests/fixtures/acceptance/` if no path given).

The goal is to catch the most common and most dangerous failure mode in this project: **a PDF that passes our internal tests but is broken when opened by other software.**

## 1. Structural validity

For the target file (or each fixture):
- Open it with `pdfium-render`. Does it load without error?
- Re-save it (no-op edit + save). Does the round-trip produce a valid file?
- Run `qpdf --check` on the output if qpdf is available. Report warnings.
- Extract text with `pdftotext` if available. Does the text match expectations?

## 2. Cross-viewer reminder

You cannot open several independent readers yourself. After the structural checks, explicitly remind the human:

> "Structural checks complete. These do NOT prove cross-viewer compatibility. Please open the output in: (1) a mainstream PDF reader, (2) a platform viewer, (3) a third independent reader. Confirm it renders identically in all three."

## 3. Operation regression

If a specific operation was recently changed (check `git log` / `git diff`), apply that operation to the target and verify:
- Page count is correct after the operation
- Internal references (bookmarks, links, named destinations) still resolve
- Annotations survive (if not the thing being changed)
- Form fields survive (if not the thing being changed)
- File size is reasonable (didn't balloon 10x)
- Metadata is intact (unless the operation cleans metadata by design)

## 4. Performance check

For the target file:
- Time to open and render first page
- Memory used (RSS) with the doc open
- Compare against the budgets in `docs/06_CONVENTIONS.md`

Flag any budget exceeded.

## 5. Output

A report:
- Per-fixture pass/fail on structural validity
- Any operation regressions found
- Any performance budget violations
- The cross-viewer reminder (always)
- A list of any fixtures that are missing for the operation under test, with a suggestion of what to add to `tests/fixtures/`
