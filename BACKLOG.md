# Backlog — decide later

Non-blocking items deferred out of an in-flight change. Nothing here gates
the current roadmap phase. When one is picked up, move it into the relevant
`steps/P<n>.md` (or open a spec line) and delete it from here.

> This file is for *judgement calls and polish*, not tracked feature work.
> Feature work lives in `steps/`. The roadmap is in `docs/05_ROADMAP.md`.

## Decisions to make (need a human call)

- **✅ DECIDED — `lopdf` adopted (COS/dict layer) for structural edits.**
  pdfium-render's outline/link/destination API is read-only, blocking
  reference rewriting (P2-PAGE-002/003), form-field preservation+rename
  (P2-PAGE-005/008), and bookmark merge (P2-PAGE-008). **Resolved:** `lopdf`
  (MIT, pure Rust, `default-features = false`) now lives alongside PDFium as a
  byte-handoff COS layer (`src-tauri/src/pdf/cos.rs`); see `docs/03` "Structural
  edits — `lopdf`" and `docs/04` "Structural edits via lopdf". A capability
  spike (`tests/cos.rs`) proves outline read/write + form-field rename and that
  every lopdf output round-trips through PDFium (and Ghostscript). **Now-unblocked
  follow-up work** (each its own step):
  - ✅ **C1 (reorder)** — done (`reorder.rs`, `cos::reorder_pages`).
  - ✅ **C4 completion** — done (`cos::merge_documents`: merges `/Outlines` +
    `/AcroForm`, suffixes colliding `/T`). *Limit:* top-level field `/T` only —
    kid-field hierarchies (parent `/T` + `/Kids` partial names) are a follow-up.
  - ✅ **D1 completion** — done (`cos::register_inserted_form_fields` re-attaches
    inserted pages' terminal fields; undo via `RestoreDocEdit`). *Limit:*
    terminal fields only (kid hierarchies = follow-up); `RestoreDocEdit` holds
    a full-doc byte snapshot per insert in the undo stack — fine for infrequent
    inserts, but a memory cost on large docs (folds into the edit-perf item).
  - ✅ **B2/C3 dangling-ref cleanup** — done. `cos::prune_dangling_destinations`
    runs on the write path (`save_document`): removes `/Link` annotations that
    dangle (target page gone) or are destination-less, drops dangling top-level
    bookmarks (re-chained) + neutralizes nested ones, and `prune_objects()`
    GCs the orphans. *Remaining:* named-destination (`/Names /Dests`) cleanup;
    and the per-save lopdf load is unconditional — gate it on "a delete/split
    happened this session" if save latency on huge docs becomes an issue.
    *Note:* `FPDF_ImportPages` (insert/split) doesn't remap internal-link dests,
    so inserted internal links are pruned as dead — non-link annotations
    survive (the lopdf merge does preserve links).

- **Thumbnail appearance in dark mode — match vs. true-colour.**
  Currently the sidebar thumbnails invert with the page in dark mode
  (shared `DARK_PAGE_FILTER`, commit `13e139c`) so the sidebar matches the
  main view. The alternative many PDF tools use: keep thumbnails
  **true-colour** so pages stay recognisable at a glance regardless of
  theme. Current choice = "match". Revisit if it reads poorly in practice;
  flipping it is a one-line change to the `<img>` style in
  `src/panels/ThumbnailPanel.tsx`.

- **Add an EARS spec line for Save (`P2-SAVE-001`)?**
  P2.A1 (Save) shipped as infrastructure with no `P2-PAGE-*` id; it's
  governed by `NFR-PERF-004` + `docs/04` §"Saving and auto-save". A
  candidate EARS line was drafted in the P2.A1 plan. Decide whether to add
  it to `docs/02_PRODUCT_SPEC.md` (human-owned file) or leave Save as pure
  infrastructure.

## Deferred polish / tech debt

- **Embedded-font fixture for extract/merge (from P2.C2).** Tests use
  `links.pdf`, whose pages use *standard* Helvetica (not embedded), so they
  prove structure (page count, opens cleanly) but not embedded-glyph
  fidelity. Add a fixture with an embedded/subset font so `extract.rs` (and
  later `merge.rs`) can assert the spec's "no missing glyphs" directly. The
  new `bookmarks.pdf` (P2.C3) is in the same boat — standard Helvetica.

- **Nested page-tree reorder (from P2.C1).** `cos::reorder_pages` reorders the
  root `/Pages` `/Kids` and requires a **flat** tree (each Kid is a `/Page`
  leaf); it errors on a nested tree (intermediate `/Pages` nodes) rather than
  risk dropping inherited attributes (`/Resources`, `/MediaBox`, `/Rotate`).
  Most PDFs — and all PDFium-resaved ones — are flat, so this is rare. To
  support nested trees, flatten the tree before reordering: resolve each page's
  effective inherited attributes onto the page, re-parent all leaves to the
  root, then reorder. Add a nested fixture to test it.

- **Audit transient-document drops for lock-safety (from P2.D1).**
  `PdfDocument::drop` calls `FPDF_CloseDocument` *without* our process-global
  `PDFIUM_LOCK`, which can race another PDFium thread (the SIGABRT/SIGSEGV
  class the lock exists to prevent). The actor closes its own doc under the
  lock, and `insert_from` now closes its source under the lock — but the
  transient docs in `extract`/`split`/`merge` (`out`, and merge's N opened
  sources) and `save_document`'s verify-reopen doc still drop *unlocked*. It
  hasn't crashed in practice (single-threaded tests; real sessions haven't
  hit the window), but it's a latent race. Wrap each transient drop in a
  `pdfium_lock()` guard (or add a small `close_under_lock(doc)` helper) and
  use it everywhere a `PdfDocument` is dropped off the actor's main path.

- **Split by-size re-serializes per page — O(n²) (from P2.C3).**
  `split::groups_by_size` grows a chunk one page at a time and calls
  `save_to_bytes()` after each addition to measure size, because PDFium gives
  no size oracle. On a large document that is a lot of repeated serialization.
  Acceptable for a one-shot split today; optimize (incremental size estimate,
  or a coarse pass then refine) only if a big split feels slow. Also: the
  result is *approximate* (shared resources compress unpredictably), so a
  chunk can slightly exceed the target — documented behaviour, not a bug.

- **Drag-select crop overlay (from P2.B4).** Crop currently uses a margins
  dialog (trim N points from each edge). The richer UX is a draggable /
  resizable selection rectangle over the page in the main view, mapping
  screen → PDF points (with the y-flip). The backend (`pdf_crop_page`)
  already takes an arbitrary rectangle, so this is purely a viewer-overlay
  + coordinate-mapping job.

- **Actor's cached page count goes stale after edits (found in P2.B3).**
  `Message::GetPageCount` (and `pdf_open`'s `OpenedDocument.pageCount`)
  return the count captured at *open*; after an insert/delete it's wrong.
  `GetMetadata` re-reads live, so the fix is to have `GetPageCount` return
  `doc.pages().len()` (and/or refresh the cached metadata on each mutating
  message). Low impact today — the UI's page count comes from PDF.js, which
  reloads via the edit epoch — but a trap waiting for the next consumer.

- **D1 sidebar open/close animation** — the thumbnail panel snaps; a slide
  transition would feel better.
- **Dark-mode load flash** — a freshly opened page can flash light before
  the dark filter applies on first paint.
- **Thumbnail cache eviction** — `thumbnail-cache` (IndexedDB) grows
  unbounded; add an LRU/size cap.
- **IPC byte-size perf for renders** — `pdf_render_page` returns `Vec<u8>`
  as a JSON `number[]`; large pages would benefit from
  `tauri::ipc::Response` (raw bytes). Noted in `pdf::render::RenderedPage`.
- **Recovery: adopt the original path (P2.A2).** "Recover" currently opens
  the autosave file directly, so a subsequent ⌘S targets the autosave copy,
  not the user's original. The recovered document should adopt its
  `originalPath` (from the sidecar) so Save writes back to the real file.
  Needs a small `spawn` variant that loads bytes from file X while
  reporting path Y. Safe today (never clobbers the original); just a UX
  sharpening.
- **Autosave cleanup on hard-exit (P2.A2).** Graceful close discards the
  recovery copy; a `std::process::exit` that skips actor `Drop` could leave
  a stale copy, causing a spurious recovery offer after a *clean* quit.
  Moot until edits create copies (B-steps); revisit then.
- **Edit-preview re-parses the whole document per edit (except rotate).**
  **Rotate** now uses a fast path — a cosmetic PDF.js viewport rotation while
  PDFium holds the real `/Rotate`, no reload (`rotation-preview-store.ts`).
  But **delete / undo / redo** still reload PDF.js from the whole document
  (and remount the virtualizer — the no-blank in-place swap was reverted as
  unreliable). On a large doc (1300-page book) those blank + re-parse.
  Remaining optimizations: re-render only the affected page(s) on delete;
  reuse measured page dimensions across reloads; make undo/redo of a rotate
  also cosmetic (they currently reload); and the `tauri::ipc::Response`
  raw-bytes upgrade (also above). Do when a large PDF feels slow.
- **External-edit reload / file watching.** If the user edits an open PDF in
  another app (Preview, etc.), VibePDF doesn't notice — the preview is stale
  and re-opening just focuses the existing tab. Watch the file's mtime (or a
  Tauri fs watcher) and offer to reload when it changes on disk. Also: an
  explicit "Reload from disk" action.

## From the 2026-06-13 verification sweep

- **Thumbnail doesn't reflect a crop (from P2.B4).** The main view crops
  correctly (CropBox), but the sidebar thumbnail still renders the full
  MediaBox. The thumbnail re-render needs to honour `/CropBox` (PDF.js renders
  the crop box by default — likely the cache isn't invalidated, or the
  thumbnail render forces MediaBox). Cosmetic; doesn't affect saved output.

- **Crop (CropBox) not carried into split/extract children (from P2.B4/C3).**
  `FPDF_ImportPages` copies page content but drops the source page's
  `/CropBox`, so a cropped page split into its own file shows uncropped. To
  preserve it, re-apply the source `/CropBox` onto each imported child (either
  via the lopdf COS pass or by reading the box before import and setting it
  after). Confirm whether `/Rotate` survives import too while here.

- **Inserted form widgets render with no visible border (from P2.D1).** A
  white text field on a white page is invisible until focused. The field is
  present and fillable (correct per spec). Consider giving inserted/registered
  widgets a faint `/MK /BC` border (or a light `/BG`) so they're discoverable.

- **Reversed insert range `99-82` is silently sorted to `82..99` (from P2.D1).**
  Decide: reject a descending range with an error, or keep normalizing and
  document it. Today it just inserts ascending.

## From P2.B5 (resize)

- **Resize doesn't re-scale annotations (from P2.B5).** `cos::resize_pages`
  scales page *content* (wraps the content stream in `q <matrix> cm … Q`) but
  leaves `/Annots` `/Rect`s at their original coordinates, so an annotation on a
  resized page ends up mis-placed/wrong-sized. Most resize targets are plain
  content pages, so this was scoped out. To fix: apply the same affine to each
  annotation's `/Rect` (and `/QuadPoints`, appearance-stream `/BBox`/`/Matrix`)
  in the cos pass. Shares the "annotation geometry" concern with the crop-in-split
  item above.

- **Resize drops `/CropBox` (and Bleed/Trim/Art) (from P2.B5).** To avoid a
  stale crop window over the new geometry, resize removes those boxes so they
  default to the new `/MediaBox`. If a user cropped *then* resized, the crop is
  lost. Acceptable for now; revisit if resize-after-crop is a real workflow
  (scale the CropBox by the same matrix instead of dropping it).

- **Resize orientation isn't auto-matched (from P2.B5).** Presets are portrait;
  applying e.g. A4 to a landscape page forces portrait (preserve-aspect centres
  the content so nothing is lost). A "match source orientation" toggle, plus
  mm/inch input units in the dialog, are easy follow-ups.

## From P3.A2 (annotation render layer)

- **Draft re-renders the overlay on every `pointermove` (from P3.A2).** The live
  draft is written to `useAnnotationStore` on each move, so the active page's
  overlay re-renders per event. Fine for one rect; if a future tool (ink) emits
  hundreds of points or a page has many annotations, keep the in-progress draft
  in local component state and commit to the store only on `pointerup`.

- **No deselect-on-empty-click (from P3.A2).** When idle the overlay is
  `pointer-events:none`, so clicking empty page space can't clear the selection
  (only clicking another shape moves it). Add a transparent hit area or an
  Escape/clear affordance when selection editing lands (with the move/resize
  handles, which are also deferred).

- **The temporary "▭" toolbar toggle + `example-rect-tool` registration are A2
  scaffolding** (`ZoomToolbar.tsx`) — remove when the real annotation tool
  palette ships in B1/C1.

## Real bugs (fix soon — these aren't polish)

- ✅ **DONE 2026-06-13 — C1 reorder no longer dead in the GUI.** Root cause was
  **WKWebView dropping all HTML5 drag-and-drop drop-target events**
  (`dragenter`/`dragover`/`drop` never fire; only `dragstart`/`dragend`),
  confirmed by instrumentation — not the nested-tree limit (it failed on a flat
  fixture). Fixed by reimplementing the reorder with **pointer events** in
  `ThumbnailPanel.tsx`. Recorded as a webview gotcha in `docs/04`. *(The
  nested-tree reorder limit above is still real but separate — `cos::reorder_pages`
  remains flat-tree-only; a nested PDF will still no-op, and surfacing that as a
  toast instead of a silent `console.warn` is a worthwhile follow-up.)*

- ✅ **DONE 2026-06-13 — thumbnail/outline jump now lands at the page top.**
  Clicking a thumbnail scrolled ~20% into the target page. Cause: the
  `PageVirtualizer` scroller was `position: static`, so the page slots'
  `offsetParent` was a positioned ancestor *above* the scroller (it included the
  toolbar), inflating `el.offsetTop` by a constant ≈ the toolbar height.
  `scrollTo(offsetTop - 8)` therefore over-scrolled. Fixed by making the scroller
  `relative` (it becomes the offset context) — corrects all jump paths
  (scrollToPage / scrollByPages / reload-restore) **and** the skewed current-page
  tracking, in one line.
