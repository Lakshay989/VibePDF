# Phase 7 — verification pass

Everything the automated suite cannot reach: the six toolbar features that have
**never been clicked**, and the five file formats whose only real authority is
the program that opens them.

State when this was written: 905 Rust tests, 796 frontend tests, `npm run check`
clean. None of that tells you whether Word opens a `.docx`, or whether the
Compress dialog's buttons are wired to the right handlers.

**Tick a box when it passes. If something fails, write what you saw next to
it** — a failure with symptoms is worth more than a red X.

---

## 0. Setup

Regenerate every output file, so what you check matches what the code does now:

```bash
cd src-tauri && cargo test --test export_image --test export_docx --test export_xlsx --test compress --test ocr_to_searchable --test ocr_languages --test create_docx --test create_html --test create_pdfa -- --ignored --nocapture
```

Then start the app and leave it running:

```bash
npm run dev
```

> The big compress inputs need `tests/fixtures/basic/noisy-scan.pdf`, which is
> git-ignored. If the compress outputs are missing, run
> `python3 tests/fixtures/basic/generate-noisy-scan.py` first.

---

## Part 1 — In the app

**This is the part that has never been done.** Six features shipped with
passing tests and were never once operated by a person. The failure this
catches is not a wrong pixel; it is a button wired to nothing, a dialog that
never closes, or a handler that throws into a console nobody is reading.

Keep the devtools console open for all of Part 1. **A red error there is a
failure even if the file comes out right.**

### 1.1 Read text… (OCR) — the longest-standing gap

- [ ] Open `Sample PDFs/in/scanned/scan.pdf`. It is a picture of text: try to
      select a word with the cursor. **Nothing should be selectable.**
- [ ] Toolbar → **Read text…**. The dialog should list languages with English
      first, offer whole-document / this-page, and three quality switches.
- [ ] Click **Read text**. It takes ~4.5 s a page and the dialog disables
      itself while it runs.
- [ ] When it finishes the dialog stays open and reports a word count.
      **Expect ~27 words.** Zero words means OCR ran and found nothing.
- [ ] Close the dialog. Now try selecting that same word. **It should select.**
      The page must look *identical* — the text layer is invisible.
- [ ] Ctrl/Cmd+Z. The selectable text should disappear again.

*Fails if:* the dialog lists no languages (tessdata missing), the word count is
0, the page visibly changes, or selection still does nothing afterwards.

### 1.2 Read text… on the awkward scans

- [ ] `in/scanned/scan-rotated.pdf` — a page whose ink is sideways but displays
      upright. Run OCR. Words should still be found, and selecting a word
      should highlight **the word you clicked**, not one elsewhere on the page.
- [ ] `in/scanned/scan-skewed.pdf` — a page fed in crooked. Same check: the
      highlight must sit on the ink, not drift off it.
- [ ] `in/scanned/scan-cyrillic.pdf` — switch the language to **Russian**
      first. Expect ~7 words. English on this file should find little or
      nothing, which is the correct behaviour.

*Fails if:* highlights land away from the ink (a rotation or skew sign bug), or
Russian finds nothing (the language pack is not loading).

### 1.3 Export text…

- [ ] Open `Sample PDFs/in/tables/two-column.pdf`. Toolbar → **Export text…**,
      save anywhere.
- [ ] Open the `.txt`. **All five "Alpha" lines must come before the first
      "Beta" line.** Interleaved Alpha/Beta means reading order is broken.
- [ ] Open a fresh copy of `in/scanned/scan.pdf` (one you have *not* OCR'd) and
      export its text. **Expect the "No text to export" message**, not an empty
      file written silently.

### 1.4 Export images…

- [ ] Open `in/normal/article-two-page.pdf`. Toolbar → **Export images…**.
- [ ] The dialog should offer PNG / JPG / TIFF / WebP, a DPI field, and show a
      live pixel estimate. **Type 1200 into the DPI field — the button must
      disable.** Type 300 — it must enable again. Clear the field entirely —
      it must disable and must not show "NaN".
- [ ] Pick **JPG**. A quality choice should appear. Pick PNG again — it should
      disappear.
- [ ] Export at 300 DPI into a new folder. Check the files are named
      `article-two-page-001.png`, `-002.png`.
- [ ] Choose **This page only** while on page 2 and export again. **The file
      must be named `-002.png`, not `-001.png`** — an image file *is* that page.

### 1.5 Compress…

- [ ] Open `Sample PDFs/in/scanned/noisy-scan.pdf` (~28 MB).
- [ ] Toolbar → **Compress…**. The dialog should show the current size and an
      estimate that **changes when you pick a different level**.
- [ ] Choose **Much smaller**, save a copy. Expect roughly 1.4 MB and a
      before/after line naming both sizes.
- [ ] **Check the original is untouched** — still ~28 MB on disk, still open in
      the app unchanged. Compression must never overwrite in place.
- [ ] Now compress `in/normal/invoice.pdf`, which has nothing to gain.
      **Expect the "already as small as we can make it" message**, not a fake
      saving.

### 1.6 Export to Word…

- [ ] Open `Sample PDFs/in/tables/report.pdf`. Toolbar → **Export to Word…**.
- [ ] Save, then open the `.docx` (Part 3 covers what to look for).

### 1.7 Export to Excel…

- [ ] Open `Sample PDFs/in/tables/table.pdf`. Toolbar → **Export to Excel…**.
- [ ] Save, then open the `.xlsx` (Part 3).
- [ ] Now try **Export to Excel…** on `in/tables/report.pdf`, which has no
      ruled table. **Expect "No tables found" and no file written** — check the
      folder: there must be nothing at the path you chose.

### 1.8 Create PDF… from a Word table (added with P7.C1b2)

The automated tests build their Word tables by hand. macOS `textutil`, the
outside writer used for plain text, writes tables as loose paragraphs, so no
table made by a real word processor has been through this path yet.

- [ ] In **Pages**, make a document with a table: a header row, three columns
      of different widths, about 40 rows (so it runs onto a second page), one
      cell holding a long sentence, one shaded cell and two cells merged.
      **File → Export To → Word…**, saved as
      `Sample PDFs/in/tables/pages-table.docx`.
- [ ] In the app: header → **Create PDF…** → add that file → **Create**.
- [ ] **Expect:** the columns at the widths Pages drew; the long sentence
      wrapped inside its own cell; the grid lines where Pages had them; the
      header row again at the top of page 2; the merged cells as one; the
      shading. Write down any warnings the dialog shows.

### 1.9 Create PDF… from a saved web page (added with P7.C1c)

The tests use hand-written pages, pages from macOS `textutil` and VibePDF's
own HTML export. None of them is a real website.

- [ ] In **Safari**, open a text-heavy article. **File → Save As…**, Format
      **Page Source**, into `Sample PDFs/in/html/`. Use Page Source, not Web
      Archive: a `.webarchive` isn't HTML.
- [ ] In the app: **Create PDF…** → add the `.html` → **Create**.
- [ ] **Expect:**
  - The article's text, in order, in its own fonts.
  - Headings larger than the body; lists with bullets or numbers.
  - No pictures (Safari's Page Source keeps them on the web), with a
    warning that says so.
  - A warning about page layout if the site uses columns or sidebars:
    the sidebar's text then follows the article instead of sitting
    beside it.
  - Nothing cut off at the right margin.

### 1.10 Create PDF… as PDF/A (added with P7.C1d)

veraPDF already checks the files the tests make; this checks the checkbox
and the result.

- [ ] **Create PDF…** → add a photo, a Word file and a web page → tick
      **PDF/A, for archiving** → **Create**.
- [ ] Run veraPDF on what you saved:

      verapdf --flavour 2u "path/to/what-you-saved.pdf" | grep -o 'isCompliant="[a-z]*"'

      **Expect `isCompliant="true"`.**
- [ ] Open it in **Preview**: it should look the same as a file made without
      the checkbox. Look at **Tools → Show Inspector**: the title is the file's
      name.

---

## Part 2 — PDFs in Preview

Preview is the reader that matters here: it is a genuinely independent
implementation, and it is strict about things PDFium tolerates.

```bash
open -a Preview "Sample PDFs/out/p7-ocr/vibepdf-verify-ocr.pdf" "Sample PDFs/out/p7-ocr/vibepdf-verify-ocr-cyrillic.pdf"
```

- [ ] **`vibepdf-verify-ocr.pdf`** — looks like a plain scan. Now
      **Edit → Select All, then copy, and paste somewhere**. You should get the
      page's words. This is the whole point of OCR: Preview reads the invisible
      layer we wrote, not the picture.
- [ ] Use **Preview's search** (Cmd+F) for a word you can see on the page.
      It should find and highlight it, **on the ink**.
- [ ] **`vibepdf-verify-ocr-cyrillic.pdf`** — same, with Russian text. Copy and
      paste: the Cyrillic must survive as Cyrillic, not as `?????`. This is the
      file that proves the text layer embeds a font rather than dropping the
      characters.

```bash
open -a Preview "Sample PDFs/out/p7-compress/00-original.pdf" "Sample PDFs/out/p7-compress/02-medium.pdf" "Sample PDFs/out/p7-compress/03-high.pdf"
```

- [ ] All three open, all 6 pages, same page size.
- [ ] Put `00-original` and `02-medium` side by side at **100% zoom**. The
      claim being tested is "no visible quality loss at 100%". Believe your own
      eyes over the claim.
- [ ] Now look at `03-high` (0.6% of the original). **Decide whether this is
      acceptable to ship as a preset.** If the text looks mushy to you, say so —
      the level is two numbers and easy to change.

```bash
open "Sample PDFs/out/p7-images"
```

- [ ] Quick Look each file (space bar). All **nine** should render as a page
      image — four formats of `hello.pdf`, two DPIs of `ccitt`, a colour page,
      and two numbered JPGs from a page selection.
- [ ] Select `hello-png-001.png` → **Tools → Show Inspector**. Dimensions
      should be **1275 × 1650** (US Letter at 150 DPI).
- [ ] `ccitt-72dpi.png` vs `ccitt-600dpi.png` — the second should be visibly
      sharper. Both should be a black-and-white pattern, not blank.
- [ ] `jpx-colour.png` — should be **four coloured quadrants**, not grey. A
      grey image here means a colour-channel bug.

```bash
open -e "Sample PDFs/out/p7-text/vibepdf-verify-text-two-column.txt" "Sample PDFs/out/p7-text/vibepdf-verify-text-article.txt"
```

- [ ] `two-column.txt` — all Alpha lines, then all Beta lines.
- [ ] `article.txt` — reads as prose, in order, with no obvious scrambling.

```bash
open -a Preview "Sample PDFs/out/p7-create/tables.pdf" "Sample PDFs/out/p7-create/table-roundtrip.pdf"
```

- [ ] **`tables.pdf`** (made from a hand-built Word file) — page 1: a grid
      with blue outer edges and red inside rules, a green cell with its text
      readable on top, a heading spanning two columns, and a cell merged down
      three rows with its word centred. The long table carries on to page 2
      with its **Item / Note** header repeated, and no line or rule runs into
      the bottom margin.
- [ ] **`table-roundtrip.pdf`** (`table.pdf` exported to Word, then back) —
      each table's four columns line up from row to row, with the figures in
      their own columns.

```bash
open -a Preview "Sample PDFs/out/p7-create/from-html-features.pdf" "Sample PDFs/out/p7-create/from-html-textutil-table.pdf" "Sample PDFs/out/p7-create/from-html-textutil-memo.pdf"
```

- [ ] **`from-html-features.pdf`** (from `html-features.html` beside it).
  - The blue Georgia heading.
  - The line of bold, italic, underlined, struck, `code`, H₂O and x².
  - A bulleted list with a numbered list inside it.
  - Preformatted text keeping its spaces.
  - A grey rule.
  - A yellow box with a gold border.
  - A grid table: blue header row, then a row spanning both columns. Its
    **Item / Note** header is repeated at the top of page 2.
- [ ] **`from-html-textutil-table.pdf`** (HTML written by macOS).
  - A three-column table with black borders.
  - "Drive shaft" on yellow.
  - "Fasteners, assorted" spanning two columns.
  - No cell text wrapped.
- [ ] **`pdfa-every-source.pdf`** and **`pdfa-html-features.pdf`** (PDF/A) —
      the same content as the files above, unchanged in look; the CMYK test
      image is still red.
- [ ] **`from-html-textutil-memo.pdf`** — the memo's heading, body, list
      lines, centred line, red and underlined words, and the Cyrillic line.

---

## Part 3 — Word and Excel files

`open` uses whatever app is your default for `.docx` / `.xlsx`. If Microsoft
Word or Excel ask for a subscription, use **Pages** and **Numbers** instead —
they are free and open both formats. Either is a genuine outside reader, which
is the point.

```bash
open "Sample PDFs/out/p7-docx/report.docx"
```

- [ ] **It opens at all.** "The file is corrupt" is the failure mode this
      format is famous for, and no test we have can see it.
- [ ] Turn on the navigation pane (Word) or the sidebar's Table of Contents
      (Pages). **Four headings should be listed**,
      at three levels: "Quarterly Report" (1), "Revenue & Costs" and "Risks"
      (2), "Outlook" (3). Headings that are merely *large text* will not appear
      here — that is exactly the distinction being checked.
- [ ] "This sentence is emphasised." should be **italic**, and "This sentence
      is strong." **bold** — selectable as such, not just visually slanted.
- [ ] The image should be present, bordered, about 128 × 96 pt.
- [ ] The ampersand in "Revenue & Costs" should be an ampersand, not `&amp;`.

```bash
open "Sample PDFs/out/p7-docx/table.docx"
```

- [ ] Three tables, each 4 columns × 4 rows, bold header row.
- [ ] **Click into a cell and press Tab.** It must move to the next cell. If
      the cursor runs through it as a line of text, detection produced
      text-that-looks-like-a-table, which no automated check here can tell
      apart.
- [ ] The prose above and below each table should be ordinary paragraphs, and
      each sentence should appear **once**.

```bash
open "Sample PDFs/out/p7-xlsx/table.xlsx"
```

- [ ] Three sheets: `Page 1 Table 1`, `Page 2 Table 1`, `Page 3 Table 1`.
- [ ] Column D (**Units**) should **right-align** and show a sum in the status
      bar when selected — these are real numbers.
- [ ] Column B (**Revenue**, `412,000`) should **left-align** — it is text on
      purpose, because a comma means a thousands separator in one locale and a
      decimal point in another and a PDF carries no locale to decide with.
      **If you would rather have these parsed as numbers, say so** — it is a
      contained change, and this is the decision to revisit.

---

## Part 4 — Optional

- [ ] Any `out/p7-ocr/*.pdf` in **Firefox** — search should find the OCR'd text.
      Optional: Preview in Part 2 is the check that counts.

Acrobat is not part of this pass: it is installed but paywalled, so it is not a
usable reader. Signature validation, which is what it would have been for, is
already covered by the pyHanko checker built in P6:

```bash
bash "Sample PDFs/tools/check-signature.sh" "Sample PDFs/out/p6-security/vibepdf-verify-signed.pdf"
```

---

## When you are done

Tell me which boxes failed and what you saw. Passing everything means Phase 7's
acceptance demo is genuinely demonstrated rather than merely tested, and the
`[ ]` markers in `steps/P7.md` can be flipped — **that is your call to make, not
mine.**
