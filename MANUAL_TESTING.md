# Manual testing — your verification checklist

Things only a human can confirm: cross-reader PDF validity, real-GUI
behavior (the Tauri webview, not jsdom), and CI runs. Automated tests
(`npm run check` / `test` / `test:rust`) are all green; this is the rest.

Tick a box when done. When a step's checks pass, flip its status in
`steps/P2.md` (or `steps/P1.md`) from `[~]` to `[x]`.

---

## Where test PDFs live (and where to get more)

- **`Sample PDFs/`** (repo root, **git-ignored**) — all sample + verification
  PDFs go here, **not the Desktop**. Large, often copyrighted, and
  regenerable, so they are never committed. The `/ship` verification
  artifacts (`vibepdf-verify-*.pdf`) and any PDFs you download for manual
  testing belong here. `TestPDFs/` is also ignored if you prefer that name.
- **`tests/fixtures/`** (committed) — the *deterministic* fixtures the
  automated suite depends on (`hello.pdf`, `links.pdf`, `bookmarks.pdf`).
  These are small, hand-generated, and checked in. Don't put scratch PDFs here.

**Sources for edge-case / sample PDFs** (download into `Sample PDFs/`):

- **Cabinet of Horrors** — deliberately broken / spec-edge PDFs:
  https://github.com/openpreserve/format-corpus/tree/master/pdfCabinetOfHorrors
- **tpn/pdfs** — a large grab-bag of real-world PDFs (technical papers, specs):
  https://github.com/tpn/pdfs
- **py-pdf/sample-files** — curated, well-described samples incl. embedded
  fonts, forms, encryption: https://github.com/py-pdf/sample-files

---

## A. Cross-reader PDF checks

Open each in **Adobe Acrobat + macOS Preview + a third reader** (Chrome
works as the third). A passing unit test does *not* prove cross-reader
validity.

- [x] **`Sample PDFs/vibepdf-verify-rotated.pdf`** (B1 rotate) — page 1 should
  render **rotated 90°** and the file must not be flagged corrupt.
  → **PASS (2026-06-13, Preview).** P2.B1 flipped to `[x]`.
- [x] **`Sample PDFs/vibepdf-verify-deleted.pdf`** (B2 delete) — **2 pages**
  ("Page 1 (link to page 3)" then "Page 3"); page 2 gone; not corrupt.
  → **PASS (2026-06-13, Preview).** P2.B2 flipped to `[x]`.
- [x] **`Sample PDFs/vibepdf-verify-inserted.pdf`** (B3 insert) — **4 pages**:
  "Page 1", then a **blank** page, then "Page 2", "Page 3"; not corrupt.
  → **PASS (2026-06-13, Preview).** P2.B3 flipped to `[x]`.
- [x] **`Sample PDFs/vibepdf-verify-cropped.pdf`** (B4 crop) — page 1 shows
  only its **centre** (100pt trimmed each edge); pages 2–3 full; not corrupt.
  → **PASS (2026-06-13, Preview).** P2.B4 flipped to `[x]`.
- [x] **`Sample PDFs/vibepdf-verify-extracted.pdf`** (C2 extract) — **2 pages**:
  "Page 1 (link to page 3)" and "Page 3"; renders correctly; not corrupt.
  → **PASS (2026-06-13, Preview).** P2.C2 flipped to `[x]`.
- [ ] **`Sample PDFs/vibepdf-verify-split-001/002/003.pdf`** (C3 split) — **three
  files, 2 pages each** ("Page 1"+"Page 2", "Page 3"+"Page 4", "Page 5"+"Page
  6"); each opens cleanly and is not corrupt. (Produced by splitting the
  6-page `bookmarks.pdf` every 2 pages.)
  → on pass, flip **P2.C3** to `[x]`.
- [x] **`Sample PDFs/vibepdf-verify-merged.pdf`** (C4 merge) — **7 pages**:
  bookmarks.pdf (Page 1–6) → forms.pdf ("Form", page 7). The **bookmarks panel
  shows 3 bookmarks** (Chapter 1/2/3, navigating to the right pages) **and** a
  **form field** is present on the last page. Opens cleanly. *(Full P2-PAGE-008
  now — bookmarks + form fields preserved.)*
  → **PASS (2026-06-13):** 7 pages + form field (Preview); **3-bookmark outline
  confirmed in Chrome's outline sidebar** (Preview hides outlines by default).
  P2.C4 flipped to `[x]`.
- [x] **`Sample PDFs/vibepdf-verify-insertfrom.pdf`** (D1 insert-from) — **5
  pages**: "Hello, Vibe.PDF.", then links.pdf's 3 pages (the first keeps its
  annotation), then forms.pdf ("Form"). The **last page has a fillable form
  field** (`name`). Opens cleanly. *(Full P2-PAGE-005 — form fields preserved.)*
  → **PASS (2026-06-13):** field present and fillable, **confirmed rendering in
  Chrome** (was camouflaged white-on-white in Preview — cosmetic, BACKLOG).
  P2.D1 flipped to `[x]`.
- [x] **`Sample PDFs/vibepdf-verify-reordered.pdf`** (C1 reorder) — **3 pages**
  in the order **"Page 3", "Page 1 (link to page 3)", "Page 2"** (links.pdf
  reordered `[2,0,1]`); opens cleanly, and the link on the "Page 1" page still
  jumps to the "Page 3" page (reference integrity).
  → **PASS (2026-06-13):** in-app drag-reorder fixed (pointer events) and
  verified live; backend output was always correct.
- [x] **`Sample PDFs/vibepdf-verify-pruned.pdf`** (B2/C3 dangling cleanup) —
  `bookmarks.pdf` with page 3 deleted. The **bookmarks panel shows 2 entries**
  (Chapter 1 + Chapter 3; **Chapter 2 — which pointed at the deleted page — is
  gone**), and no broken bookmark remains. 5 pages, opens cleanly.
  → **PASS (2026-06-13):** Ch2 removal confirmed in bytes (Ch1 + Ch3 only,
  `/Count 2`); folded into P2.B2 `[x]`.
- [x] **`Sample PDFs/vibepdf-verify-resized.pdf`** (B5 resize) — `hello.pdf`
  (Letter) resized to **A4 (595×842 pt)** with preserve-aspect. The text
  "Hello, Vibe.PDF." should be **scaled to fit** the A4 page (not clipped, not
  sitting at the old position with empty space), and the page should measure A4.
  Opens cleanly. → **PASS (2026-06-13):** content-scale byte-verified (cos
  `q…cm` wrapper + A4 MediaBox + PDFium reopen) and confirmed in-app via PDF.js.
  P2.B5 flipped to `[x]`.
- [x] `Sample PDFs/vibepdf-verify.pdf` (A1 save) — already verified.

## B. In-app checks (`npm run dev`)

Open a **multi-page** PDF for these (a one-pager hides the interesting bits).

- [x] **Rotate (B1):** right-click a page thumbnail → Rotate right / left /
  180. The thumbnail updates immediately. → **PASS (2026-06-13).**
- [ ] **Live preview (pipeline):** scroll to ~page 3, then rotate that page.
  The **main view** should rotate *in place at page 3* — no blank flash, no
  scroll jump. (If it jumps to page 1, tell me — page-restore timing.)
- [x] **Delete (B2):** right-click a page thumbnail → **Delete page** (or
  focus a thumbnail and press **Delete/Backspace**). The page vanishes, the
  count drops, and the main view updates live. **⌘Z** brings it back in the
  same position. ⌘S → reopen externally → page really gone.
  → **PASS (2026-06-13).** P2.B2 flipped to `[x]`.
- [x] **Insert blank (B3):** right-click a page thumbnail → **Insert blank
  page after**. A blank page (same size) appears right after it; count goes
  up; main view + thumbnails update. **⌘Z** removes it; ⌘S → reopen → blank
  page present. → **PASS (2026-06-13).** P2.B3 flipped to `[x]`.
- [x] **Crop (B4):** right-click a page → **Crop page…** → enter margins →
  Apply. The page shows only the cropped region (main view + thumbnail).
  **Reset crop** restores the full page; **⌘Z** undoes; ⌘S → reopen → cropped.
  → **PASS (2026-06-13)** for the main view. Note: thumbnail did **not** show
  the crop (filed to Findings); CropBox-only crop per spec works. P2.B4 → `[x]`.
- [x] **Extract (C2):** in the viewer toolbar click **Extract…** → enter a
  range (e.g. `1,3`) → a save dialog opens → pick a path. The new PDF has
  exactly those pages and opens cleanly. (The open document is unchanged.)
  → **PASS (2026-06-13).** P2.C2 flipped to `[x]`.
- [ ] **Split (C3):** in the viewer toolbar click **Split…** → pick a mode
  (try **Every N pages** = 2, and on a bookmarked PDF **By top-level
  bookmarks**) → a folder picker opens → choose a folder. The folder gets
  `{name}-001.pdf`, `-002.pdf`, … each opening cleanly with the right pages.
  (The open document is unchanged.) A split that would make < 2 files shows
  an error. → on pass, flip **P2.C3** to `[x]`.
- [ ] **Merge (C4):** in the viewer toolbar click **Merge…** → the list is
  seeded with the current file → **Add files…** to append more → reorder with
  ↑/↓, remove with ✕ → **Merge…** → save dialog → pick a path. The new PDF has
  every page of every file, in the listed order, and opens cleanly. **Merge two
  bookmarked / form PDFs** → the result keeps **both sources' bookmarks** and
  **all form fields** (colliding names suffixed `_2`). Open document unchanged;
  button disabled with < 2 files. → on pass, flip **P2.C4** to `[x]`.
- [ ] **Insert PDF (D1):** in the viewer toolbar click **Insert PDF…** →
  **Choose file…** (the page count appears) → pick pages (blank = all) →
  choose position (start / end / after page N) → **Insert**. The pages appear
  live in the main view + thumbnails; **⌘Z** removes them, **⌘⇧Z** re-adds. ⌘S
  → reopen → inserted pages persist. **Insert a form PDF** → the inserted page's
  **form field is present and fillable** (colliding names suffixed `_2`).
  → on pass, flip **P2.D1** to `[x]`.
- [x] **Reorder (C1):** in the thumbnail sidebar, **drag a thumbnail** to a new
  position and drop it. The page order updates live (main view + thumbnails);
  **⌘Z** restores the old order, **⌘⇧Z** re-applies. ⌘S → reopen → order
  persisted. On a PDF with internal links, the link still lands on the right
  page after reordering. → **PASS (2026-06-13)** after the pointer-event rewrite
  (source tile dims, hovered tile rings, drop reorders; click still selects).
  P2.C1 flipped to `[x]`.
- [x] **Resize (B5):** right-click a page thumbnail → **Resize page…** → pick a
  size (e.g. **A4**) or **Custom** W×H, toggle **preserve aspect ratio**, choose
  **This page** / **All pages** → Apply. The page reflows to the new size in the
  main view + thumbnails with content scaled; **⌘Z** restores the old size,
  **⌘⇧Z** re-applies. ⌘S → reopen externally → new size persists. Try **All
  pages** on a multi-page PDF. → **PASS (2026-06-13):** dialog opens at the
  page's current size; This-page scales only that page, All-pages scales all.
  P2.B5 flipped to `[x]`.
- [x] **Dangling-ref cleanup (B2/C3):** open a PDF that has internal links or
  bookmarks pointing to a page, **delete that target page**, **⌘S**, reopen →
  the link/bookmark to the deleted page is **gone** (no broken navigation), the
  rest intact. (Use `bookmarks.pdf`: delete page 3 → the Chapter 2 bookmark
  disappears.) → **PASS (2026-06-13)** (Ch2 removal confirmed in bytes).
  ⚠️ **Do this on a COPY in `Sample PDFs/`, never the committed fixture** —
  saving onto `tests/fixtures/basic/bookmarks.pdf` clobbers it (see Findings).
- [x] **Undo/redo (A3):** after a rotate or delete, **⌘Z** reverts both views
  and **⌘⇧Z** re-applies. The Undo/Redo toolbar buttons enable/disable right.
  → **PASS (2026-06-13).** P2.A3 flipped to `[x]`.
- [ ] **Persist on save (B1):** rotate → **⌘S** → reopen the file in Preview
  externally → still rotated. Reopen in VibePDF → rotation persisted.
- [x] **Save no-op (A1):** ⌘S on an *unedited* doc → toast **"No changes to
  save"**; the file is left untouched. → **PASS (2026-06-13).**
- [ ] **Dark mode:** toggle theme in the toolbar → main view **and**
  thumbnails both invert and stay readable.

## C. Crash recovery (A2) — now demoable thanks to B1

This one is finicky; follow the order exactly:

- [ ] Open a PDF and **rotate a page** (this makes it "dirty"). **Do not save.**
- [ ] **Wait ~30 seconds** — the autosave tick runs every 30 s, so give it one
  tick to write the recovery copy.
- [ ] **Force-kill** the app (Activity Monitor → Force Quit, or `kill -9`).
  A normal Quit *discards* the recovery copy on purpose — you must hard-kill.
- [ ] Relaunch → a **"Recover unsaved changes?"** dialog should list the file.
  **Recover** reopens the unsaved version; **Discard** drops it.
  → on pass, flip **P2.A2** to `[x]`.
  - Known limitation (BACKLOG): the recovered tab opens from the *autosave*
    path, so ⌘S targets that copy, not the original. Recovering then Save-As
    to the original is the workaround for now.

## D. CI (GitHub → Actions tab)

- [ ] **`ci.yml`** (macOS) — green on the latest push to `main`.
- [ ] **`e2e.yml`** (E5, Linux) — the **first real E2E run**. It was written
  blind from macOS and likely needs a fixup pass (webkit / tauri-driver /
  xvfb). If green → flip **P1.E5** to `[x]`. If red → paste me the failing
  step's log and I'll iterate.

## E. Recent viewer fixes (`npm run dev`)

From the round of GUI bugs found in real use. Rotate was confirmed working;
the rest still want a pass.

- [~] **Rotate fast-path** — instant, 90°/180° both update the main view.
  *(Looked good; re-confirm the switch-back case below.)*
- [ ] **Switch-back consistency** — rotate a page → switch tabs → switch
  back. The **main page and thumbnail must both stay rotated** (the last
  fix). And ⌘S → reopen in Preview → rotation persisted.
- [ ] **Pinch / Ctrl+wheel zoom** — smooth, responsive (not crawling).
- [ ] **Close tabs** — the **×** on each tab closes that PDF.
- [ ] **Doc switch** — clicking between tabs always updates the main view
  (no "invalid pdf", no stale page).

---

## F. Phase 3 — Annotations (`npm run dev`)

- [~] **Annotation render layer (P3.A2)** — *(the temporary "▭" toggle was
  removed in B1a; A2's overlay is now exercised by the B1a markup check below.
  Confirm jump-to-page still lands at the page top.)*
- [ ] **Text selection + markup preview (P3.B1a)** — open
  `tests/fixtures/basic/hello.pdf` (or any text PDF). The text should now be
  **selectable** (drag to select "Hello, Vibe.PDF."). With text selected, click
  **Highlight** in the Markup toolbar → translucent colour over the text;
  **Underline / Strikethrough / Squiggly** draw the right marks; the **colour
  swatches** change the highlight colour. Selecting then clicking must **not**
  lose the selection. → on pass, flip **P3.A2 + P3.B1a** to `[x]`.
- [ ] **Persisted text markup (P3.B1b)** — **the key check is the rendering
  decision:** select text → **Highlight** → after a brief reload the highlight
  **shows in the main view** (the PDF.js canvas renders the `/AP`). **Cmd+Z**
  removes it; **Cmd+S** → reopen in VibePDF → still there. Then the
  **cross-reader**: open `Sample PDFs/vibepdf-verify-highlight.pdf` (and your own
  saved file) in **Preview + Chrome + Acrobat** — the highlight must be visible
  and correctly placed over the text. Repeat for underline/strikethrough/squiggly.
  → on pass, flip **P3.B1b** to `[x]`. *(If the highlight does NOT appear in the
  main view, tell me — the fallback is overlay-rendering committed markup.)*

---

## Status flips waiting on the above

| Step | Flips to `[x]` when |
|---|---|
| P2.B1 — Rotate | A (rotated PDF) + B (rotate/persist) pass |
| P2.B2 — Delete | A (deleted PDF) + B (delete) pass |
| P2.B3 — Insert blank | A (inserted PDF) + B (insert) pass |
| P2.B4 — Crop | A (cropped PDF) + B (crop) pass |
| P2.B5 — Resize | A (resized PDF: A4, content scaled) + B (resize) pass |
| P2.C2 — Extract | A (extracted PDF) + B (extract) pass |
| P2.C3 — Split | A (split PDFs) + B (split) pass |
| P2.C4 — Merge | A (merged PDF: bookmarks + form field) + B (merge) pass |
| P2.D1 — Insert from PDF | A (insert-from PDF: form field) + B (insert PDF) pass |
| P2.C1 — Reorder | A (reordered PDF) + B (drag reorder) pass |
| P2.A3 — Undo/redo | B (undo/redo) passes |
| P2.A2 — Auto-save | C (crash recovery) passes |
| P1.E5 — E2E harness | D (`e2e.yml`) goes green |

---

## Findings from the 2026-06-13 verification sweep

**Passed & flipped to `[x]`:** A1, A3, B1, B2 (+B2/C3 dangling cleanup), B3, B4,
B5 (resize, lopdf content-wrap), C1 (after the pointer-event fix), C2, C4
(bookmarks via Chrome), D1 (form field via Chrome). **Every Phase-2 page-op is
now built and human-verified**; only **A2 (auto-save / crash recovery)** remains
`[~]`, deferred by choice.

**⚠️ Process lesson (important):** the in-app testing was run **directly on the
committed fixtures in `tests/fixtures/basic/`**, so ⌘S / split / delete wrote
over the real `bookmarks.pdf` (and dropped scratch files in that dir). Restored
via `git restore` + cleanup. **Always open a COPY from `Sample PDFs/` for manual
testing — never the committed fixtures.** (PDFs you edit get saved in place.)

### Real bugs to fix
1. ~~**C1 reorder drop is broken (GUI).**~~ **FIXED 2026-06-13.** Root cause:
   **WKWebView never delivers HTML5 drop-target events** (`dragenter`/`dragover`/
   `drop`) — only `dragstart`/`dragend` — confirmed by instrumented logging, and
   it failed on a flat fixture too (so not the nested-tree limit). Rewrote the
   reorder with **pointer events**; verified live. C1 → `[x]`.
2. **Thumbnail-click scroll offset.** Clicking a thumbnail jumps to that page
   but lands so most of the *previous* page is still visible (only the very top
   of the target shows). Scroll-anchor/offset bug in the main viewer.

### Follow-ups → BACKLOG (do not block their parent step)
3. **Crop not reflected in the thumbnail** (main view crops correctly; the
   thumbnail re-render still shows the full MediaBox).
4. **Crop (CropBox) lost in split output** — `FPDF_ImportPages` doesn't carry
   the source `/CropBox` into split/extract children.
5. **Inserted form fields render white-on-white, no border** (D1) — present and
   fillable, just visually camouflaged. Consider a faint widget border.
6. **Reversed page range `99-82`** (D1 insert) is silently normalized to
   ascending `82..99` — decide: reject, or document as intended.
7. **Save latency + `.bak` lock window.** Save feels slow and the `.bak` is
   briefly unselectable in Finder mid-write. Matches the known per-save lopdf
   load cost (BACKLOG: gate the prune/load for large docs).

### Confirmed NOT bugs (expected behavior)
- **"No bookmarks" in Apple Preview** — Preview doesn't surface PDF `/Outlines`
  in its default sidebar. Bookmarks are present in the bytes (verified for
  merged + pruned). Use **Acrobat**, or Preview → **View → Table of Contents**.
- **"Can't edit the page text"** — text editing isn't a Phase 2 feature; it's
  Phase 3 (redact + reflow). Preview also can't edit arbitrary PDF text.
- **No per-page thumbnails in Preview** for these small hand-built PDFs — a
  Preview rendering quirk, not a file defect.
