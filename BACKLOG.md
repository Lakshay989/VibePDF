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

- ✅ **DONE (P3.B1a) — the temporary "▭" toggle is removed** from `ZoomToolbar`
  (replaced by the markup toolbar). The `example-rect-tool` + the A1 drag
  lifecycle stay (for C1 shapes + their tests), just no longer wired to a toggle.

## From P3.B1a (text selection + markup preview)

- **Multi-page selection robustness.** `apply-markup` maps each selection line
  rect to a page via `elementsFromPoint(centre)`; a rect straddling the page gap,
  or an overlay intercepting the point, could mis-assign. Works for the common
  single-/adjacent-page case; harden (or use range-boundary containers) if
  cross-page markup misbehaves.
- **Squiggly is approximated** (`squigglePath`) as a fixed-amplitude zigzag, not
  a true PDF squiggly appearance. Fine for the preview; B1b's `/AP` should use a
  proper wavy appearance for cross-reader fidelity.
- **Highlight blend in dark mode.** Preview uses `mix-blend-mode: multiply`,
  which assumes a light page; with the dark-mode page invert filter the blend may
  read oddly. Revisit when annotations meet dark mode.
- **Text-layer cost on huge pages.** Every visible page builds a full PDF.js text
  layer (`getTextContent` + spans). Fine now (only visible pages); if a
  text-dense 1000-page doc stutters, gate or virtualize the text layer.

## From P3.B1b (persist text markup)

- **No optimistic preview.** Apply → IPC write → epoch reload → the canvas shows
  the highlight; there's a brief delay (the lopdf+PDFium round-trip). For instant
  feedback, optimistically add the markup to the store (overlay draws it) and
  clear it once the reload lands. The overlay's `MarkupShape` rendering is kept
  (inert in prod) precisely for this.
- **`/QuadPoints` corner order is verified visually, not by spec.** We emit
  `UL,UR,LL,LR`; since we own the `/AP`, rendering is correct regardless, but a
  reader that *regenerates* appearance from `/QuadPoints` (Acrobat with `/AP`
  stripped) depends on the order. Confirm against the de-facto Acrobat order.
- **Squiggly `/AP` is a fixed-step zigzag**, not a true PDF squiggly wave.
  Acceptable; refine for fidelity if it reads poorly.
- **The frontend store doesn't track committed markup** (the canvas renders it
  from the PDF). Editing/selecting/deleting existing annotations + the sidebar
  need a read path (`cos::read_text_markup` → store) — that's **D1**. Reopened
  files still *display* correctly (PDF.js renders their `/AP`); they just aren't
  editable yet. *(A document-wide `Clear markup` button exists; per-annotation
  delete is D1.)*

- **Overlapping highlights Multiply-blend into mixed colours** (yellow over blue
  → green). It's standard `/Highlight` behaviour (and how Acrobat looks), but
  confusing. Options if we want "newest wins": a Normal-blend translucent
  appearance, or de-duping/merging overlapping quads of the same colour.

- **Dark mode recolours the highlight.** The `/AP` is baked into the canvas,
  which the dark-mode CSS invert filter then recolours (yellow → blue-ish). Fix
  by exempting annotation marks from the invert, or compensating the colour —
  revisit when annotations meet dark mode (shared with the P3.A2 dark-mode item).

## From P3.B2a (sticky notes)

- ✅ **DONE 2026-06-15 (P3.B2b) — reopened files now show their notes in-app**,
  and an actor ⌘Z no longer leaves a ghost icon. `cos::read_text_notes` →
  `pdf_read_text_notes` → `useNotesSync` makes the overlay a *projection of the
  PDF*, re-synced on open and on every edit-epoch bump (incl. undo/redo).
- **Placing a note then cancelling leaves an empty note.** Placement persists
  immediately (empty `/Contents`); closing the popup without typing keeps it. Make
  a just-placed-then-cancelled note self-delete (track "pending until first save"),
  or only persist on first save. Two undo entries (place + first edit) collapse to
  one if we switch to persist-on-first-save.
- **Popup closes only via ✕ / Save / Esc** — no outside-click-to-dismiss. Add a
  document-level pointer listener (icons/popup already `stopPropagation`).
- **Author is a fixed `"VibePDF User"`.** No per-user identity yet; thread a real
  author through `ToolOptions`/settings when identity lands (also feeds D2 replies).
- **Note icon is a fixed 18px hit target** anchored at the click's lower-left;
  it doesn't visually match the reader's own icon glyph. Fine for now; revisit if
  placement feels off against Acrobat/Preview.

## From P3.B2b (read notes on open)

- **`createdAt` is `0` for read-back notes** — `read_text_notes` doesn't parse
  `/CreationDate`//`/M` into epoch ms. The overlay doesn't use it, but the D1
  sidebar (date column / sort) will; parse the `D:YYYYMMDD…` form (inverse of
  `pdf_date_now`) when D1 lands.
- **Foreign `/Contents`//`/T` aren't decoded** — only ASCII/UTF-8 round-trips;
  UTF-16BE (BOM) and PDFDocEncoding from other editors render as mojibake. Add a
  decoder if we want to faithfully show notes authored in Acrobat with non-ASCII.
- **Re-sync re-reads on *every* edit-epoch bump**, including markup edits that
  don't touch notes (each is a full `save_to_bytes` + lopdf parse). Cheap today;
  gate to notes-affecting epochs (or diff by `/NM` set) if it ever shows up.
- **Optimistic-placement vs re-sync race (narrow).** Placement deliberately
  doesn't bump the epoch, but an *unrelated* epoch bump landing between the
  optimistic store-add and the `addTextNote` resolving could momentarily drop the
  new icon; the next re-sync restores it. Single-user-synchronous, self-healing —
  revisit only if it's ever observed.

## From P3.B3a (free-text boxes)

- ✅ **DONE 2026-06-22 (B3b, P3-ANN-003) — underline + auto-wrap + double-click
  re-edit** (underline `/AP` rule + private `/Underline` key; shared `wrap_lines`
  drives box height + drawn lines; per-box hit-zones → the sidebar's edit flow).
  Still deferred:
- **B3c — rich text (`/RC` + `/DS` mixed runs)** — per-run colour/bold/italic. The
  big remaining piece of P3-ANN-003: a runs-based editor (the current one is a
  plain textarea), `/RC` XHTML (de)serialize, and a multi-style `/AP` renderer.
  This is why P3-ANN-003 stays `[~]`.
- **Wrap uses estimated metrics** — `wrap_lines` measures `chars × size × avg-em`,
  not real AFM glyph widths, so wrap points differ slightly from other readers and
  the editor's CSS soft-wrap. Real metrics (or measuring base-14 widths) is exact.
  Mid-word breaks aren't done — a single over-long word overflows (clipped).
- **Underline persistence is a private `/Underline` key** — non-standard (readers
  ignore it but still show the `/AP` rule); B3c should standardize via `/DS`
  `text-decoration:underline`.
- **Base-14 / ASCII-WinAnsi only** — no font embedding, no non-Latin scripts; the
  three families (Helvetica/Times/Courier) render the Latin range. Embedding +
  subsetting is a separate, larger effort.
- **`/DA` carries no AcroForm `/DR`** — a reader that *regenerates* appearance
  (ignoring `/AP`) may fall back to a default font. `/AP` is the primary path;
  add the font to `/DR` if a target reader is found to regenerate.
- **Committing drops the tool to idle** — you re-pick **Text** for each box. Keep
  the tool armed for multiple boxes if it feels tedious in practice.

## From P3.D1 (annotation sidebar)

- **No per-annotation delete / edit from the sidebar** — D1 is read-only.
  Deleting/editing a specific annotation needs a **durable handle**; markup &
  free-text carry no `/NM` (only notes do). Next annotation-management step:
  enrich `add_text_markup`/`add_free_text` to write `/NM` (+ `/T` author + `/M`
  date), add `cos::delete_annotation`-style removal by `/NM` for all kinds, and
  wire a Delete (and re-edit) into the sidebar rows.
- **Markup & free-text show no author/date** — we don't write `/T`/`/M` on them,
  so the author/date columns are blank and those filters effectively only bite
  notes. Fixed by the same `/T`+`/M` enrichment above.
- **Selection handle is the lopdf object id** — stable within a load but not
  across a save/edit, so the sidebar selection (and its highlight) clears on the
  next edit. Fine for read-only navigation; the durable `/NM` handle above makes
  it survive.
- **Date filter is a single "modified after"** — no calendar range / "between".
  Add a range if users ask.
- **The list re-reads the whole PDF on every edit epoch** (serialize + lopdf
  parse), including edits that don't change annotations. Cheap today; gate to
  annotation-affecting epochs if it shows up on large files.

## From the 2026-06-18 verification sweep (annotation UX feedback)

- ✅ **DONE 2026-06-18 (P3.D1d, P3-ANN-012) — per-annotation select + delete.**
  Every annotation now carries a `/NM`; the sidebar row has a ✕ and Delete/Backspace
  removes the selected one (undoable). Shapes are now listed too (the kinds map was
  missing `/Square`+`/Circle`).
- ✅ **DONE 2026-06-18 (P3.D1e, P3-ANN-013) — edit a free-text box in place.**
  Sidebar **✎** reopens the box's editor pre-filled (text + style read back from
  `/DA`+`/BaseFont`); commit rewrites it in place preserving `/NM`. **Still
  deferred:** **shape** style re-edit (`update_shape` mirrors `update_free_text`);
  markup re-colour; in-canvas double-click-to-edit (sidebar pencil only);
  annotations saved *before* the `/NM` change are edit/delete-able only via the
  best-effort `obj:` fallback.
- ✅ **DONE 2026-06-18 — fuller colour palette incl. black/white.** Was 5 pastels;
  now 8 basics (black, red, amber, yellow, green, blue, purple, white) shared by
  markup stroke, free-text colour, and shape stroke/fill. A native
  `<input type="color">` for *arbitrary* colours is still a nice follow-up
  (skipped for now — markup's selection-preserving `onMouseDown preventDefault`
  fights the native picker, so it needs care).

## From P3.B3a (free-text boxes) — sweep follow-ups

- ✅ **DONE 2026-06-18 — oversized text no longer clipped vertically** (#3).
  `add_free_text` grows the box downward (top edge fixed) to fit `lines × leading +
  descender padding` at the chosen size; the editor textarea also grows to ≥ ~1.4
  line-heights so a big font is visible while typing. **Still open:** *horizontal*
  clipping of a single line wider than the box — needs text metrics or auto-wrap
  (the latter is B3b); for now drag a wider box.

## From P3.C1a / C1b₁ (shapes)

- ✅ **DONE 2026-06-20 (C1b₁) — line + arrow** (`/Line` + `/LE` open-arrow, drag).
- ✅ **DONE 2026-06-20 (C1b₂) — polygon** (`/Polygon`, multi-click add-vertex via a
  self-contained `PolygonLayer`). **Completes the C1 shapes track.** Deferred:
  **polyline (open)** — the cos `closed` flag + `/PolyLine` path are built and
  tested, but the UI exposes only Polygon because **P3-ANN-004 says "polygons", not
  "polylines"** (add the word to the spec to flip on a Polyline toggle); arrowhead
  **style** options (only end OpenArrow); editing a line/polygon's **geometry**
  (delete + redraw); snap-to-first-vertex / angle-snap / even-odd fill /
  self-intersection cleanup; a shared **multi-click lifecycle** in the tool
  framework (left until a third multi-click tool appears).
- **No select / delete / edit of a committed shape** — shared with markup &
  free-text; lands with the D1 read-back + per-annotation delete follow-up.
- **Axis-aligned only** — shapes are drawn to an axis-aligned `/Rect`; no rotation
  handle.
- **Solid border only** — no dashed (`/BS /D`) or cloud (`/BE`) borders, no
  rounded-rectangle corners.
- **Ellipse is a 4-Bézier approximation** (kappa) — visually exact but not a true
  conic; fine, note it if a reader complains.
- **The A2 annotation overlay's committed-shape `Shape` renderer is now unused**
  in production (committed shapes are canvas-drawn). Kept for the live-draft
  preview + a possible optimistic-render path; revisit if it bit-rots.

## From P3.C2 (freehand ink)

- ✅ **DONE 2026-06-20 (C2, P3-ANN-005) — freehand ink** (`/Ink`, drag) with
  frontend Catmull-Rom smoothing and a pressure-modulated **variable-width filled
  ribbon** `/AP`, via a self-contained `InkLayer`. Deferred:
- **One stroke == one `/Ink`** — no multi-stroke grouping. Adobe collects several
  strokes drawn before a commit into one annotation's `/InkList` (an array of
  sub-paths). We already write `/InkList` as a one-element array, so this is a
  capture-side change (debounce the commit, accumulate sub-paths) not a format one.
- **No eraser / partial-stroke edit** — and no in-canvas select-and-reshape; ink
  is delete-and-redraw, shared with the markup/shape select/edit follow-up.
- **Bézier (`c`) `/AP` deferred** — the ribbon connects dense resampled points with
  straight `l` segments. Smooth enough at ~3pt spacing; revisit only if the dense
  `/InkList` + content stream proves too large for big scribbles (then emit
  Catmull-Rom-to-Bézier `c` ops and a sparser path).
- **Fixed pressure→width curve** — `ink_half_width` maps `[0,1] → [0.4,1.3]×` base
  width with no user control (no min/max-width or sensitivity setting), and no
  velocity-based width. A pen-settings panel would expose it.
- **Round/flat caps** — the ribbon ends are flat (square across the normal); no
  rounded pen cap. Cosmetic.

## Verification debt — IN-APP DONE 2026-06-25; cross-reader optional

**2026-06-25 the user walked every Phase-3 `[~]` feature in `npm run dev` and
confirmed them** (four issues found + fixed mid-sweep: scroll-jump, free-text
wrap/hard-break, filter-reset-on-delete, reply-button visibility). All Phase-3
steps are now `[x]` in `steps/P3.md` except **B3c** (rich text, `[ ]`, deferred
by design). The remaining **cross-reader pass** (opening the
`Sample PDFs/vibepdf-verify-*.pdf` artifacts in Acrobat/Preview) is **optional /
not blocking** — the artifacts are already CoreGraphics-validated (`sips`). Worth
a deeper look someday, especially:
- **C4b** — Acrobat's measuring tool re-measuring the saved line *live* (validates
  the `/Measure` `/X`/`/D`/`/A` chain against a real reader; risk #2 from the plan).
- **E1** — Acrobat *reading* our exported `.xfdf` against the same base PDF (we
  verified the in-app cross-document round-trip, not Acrobat ingestion).
- The **per-edit page-blank flash** is a separate *open bug* (below), not
  verification debt; the scroll-jump half of it was fixed 2026-06-25.

## From P3.D2 (reply threads)

- ✅ **DONE 2026-06-22 (D2, P3-ANN-009) — reply threads** (`/Text` linked via
  `/IRT`, threaded in the sidebar via `buildThreads`). Deferred:
- **Right-click → Reply** context menu (the spec's example) — shipped an inline
  **Reply button** instead (more discoverable, consistent with the row's ✎/✕). A
  context menu could complement it.
- **Editing a reply's text** — no edit affordance (delete + re-reply for now;
  `update_text_note` already updates a `/Text` by `/NM`, so it's a small add).
- **Reply state / status** — Acrobat's `/State` + `/StateModel` (Accepted /
  Rejected / Cancelled / Completed review marks); no collapse/expand or unread.
- **Author identity** — replies use the fixed `"VibePDF User"` (no accounts);
  shared with notes.
- **Arbitrary nesting** — threads render one indent level (flat under the root,
  Acrobat-style); deep visual nesting isn't shown even though `/IRT` can chain.

## From P3.E1 (XFDF import / export)

- ✅ **DONE 2026-06-22 (E1, P3-ANN-010) — XFDF round-trip** (export reads raw
  dicts → XML; import reuses the `add_*` writers + patches `/NM`/`/Contents`/`/T`
  + wires `/IRT`; hand-rolled XFDF parser, no new dep). Deferred:
- **E1b — FDF** (import + export). The spec names XFDF *and* FDF; we shipped XFDF
  only (it's "preferred" and the modern interchange). FDF is COS syntax, so export
  is cheap via lopdf; import needs an FDF reader. This is why E1 lands `[~]` as
  honestly spec-partial, not `[x]`.
- **Free-text font-family / bold-italic fidelity** — import parses size + colour
  from the `/DA`, but defaults family to Helvetica regular (the XFDF
  `<defaultappearance>` font ref isn't mapped back to a base-14 family). Geometry +
  text + size + colour round-trip; the typeface may not.
- **`<contents-richtext>`** — Acrobat-authored rich-text free-text is dropped to
  plain `<contents>` on import (we never emit it).
- **Import is O(N · docsize)** — each annotation is added via a separate
  `add_*` load→save, then a patch load→save (2–3 full re-serializations per
  annotation). Fine for normal docs/counts (same per-annotation cost as drawing
  them); could batch into a single Document pass if a huge doc + many annots ever
  bites.
- **Multi-gesture ink** — a foreign `<ink>` with several `<gesture>` sub-paths
  imports only the **first** gesture (our `add_ink` is one stroke per annot; our
  own export always emits one). 4-component (CMYK) `/C` colours export as no
  colour (RGB + gray only).
- **Importing onto a different base PDF** clamps out-of-range pages and drops
  orphan replies (parent absent) rather than remapping — logged, not fatal.

## From P3.E2 (flatten annotations)

- ✅ **DONE 2026-06-22 (E2, P3-ANN-011) — flatten** (COS transform: register each
  annotation's `/AP` form under page `/Resources /XObject`, append a `Do` fragment
  to `/Contents`, drop the annot, prune; undoable in-session via snapshot). Deferred:
- **PDFium-native `FPDFPage_Flatten`** — the considered alternative; rejected for
  the live-handle rule + unsafe FFI. If our COS flatten ever mis-handles an exotic
  foreign appearance, the native path is the fallback (it also flattens form
  fields, which we'll need in P5).
- **Flattening a subset / by type / selection** — spec says *all*; we flatten the
  whole document. A "flatten selected" or "flatten by kind" is a natural follow-up.
- **Baking note icons** — `/AP`-less notes + replies are kept live (no appearance
  to bake). Acrobat's full Flatten stamps a note glyph; we chose not to (would lose
  the thread text + needs an icon generator). Could add a "bake note icons" option.
- **No flatten progress UI** — synchronous; a huge doc with thousands of
  annotations would block briefly (each is just a resource + content append, so
  it's cheap, but unbounded).

## From P3.C4a (measurement tools)

- ✅ **DONE 2026-06-21 (C4a, P3-ANN-007) — distance/perimeter/area + calibration**
  (`/IT` dimension annotations + a generated `/AP` label, self-contained
  `MeasureLayer`). Deferred:
- ✅ **DONE 2026-06-22 (C4b, P3-ANN-007) — `/Measure` dict + persisted calibration**
  (rectilinear `/Measure` with `/X`/`/D`/`/A` `NumberFormat`s, `/X /C` = scale;
  `read_measure_calibration` re-seeds the tool on reopen). Deferred:
- **Angle measurement** (`/T` number format) — no angle tool. **Anisotropic scale**
  (`/X` ≠ `/Y`) — single uniform scale only. **Page-level `/VP` viewport scale** —
  we attach `/Measure` to the annotation (Acrobat's measure-markup convention); a
  `/VP` fallback is the move if a reader ignores annotation-level `/Measure`.
- **UTF-16 unit labels** — `/Measure` unit strings stay ASCII (`sq ft`); the `²` is
  only in the on-screen `/AP` label. **XFDF round-trip of `/Measure`** — imported
  measures get a default `pt` scale (E1's XFDF doesn't carry the calibration).
- **Editing a measurement's calibration** in place (add-time only).
- **Self-intersecting area is undefined** — shoelace assumes a simple ring; a
  figure-eight reports a meaningless (partially cancelled) area. No guard.
- **No vertex editing / ortho / angle-snap**, no per-segment labels on a
  perimeter, no unit conversion between systems (m↔ft).
- **A shared `useVertexGesture` is now FIVE copies overdue** — note, polygon, ink,
  stamp, measure each hand-roll the click/multi-click gesture. Extract the
  primitive (vertices + rubber-band + Enter/Esc/dbl-click/close-first +
  auto-finish-at-N) before C5/the next such tool; migrate the existing layers.

## From P3.C3a (stamps)

- ✅ **DONE 2026-06-21 (C3a, P3-ANN-006) — stamp library + custom text stamps**
  (`/Stamp` + generated `/AP`, click-to-place via a self-contained `StampLayer`).
  Deferred:
- ✅ **DONE 2026-06-22 (C3b, P3-ANN-006) — image stamps** (PNG → Image XObject via
  the `png` decoder, alpha → `/SMask`, aspect-correct placement, optional overlaid
  label; `pdf/image_xobject.rs`). Deferred:
- **JPEG + other formats** — PNG only (the stamp format; transparency). JPEG embeds
  dep-free via `/DCTDecode` + an SOF header parse (DeviceRGB/Gray; CMYK rejected) —
  a quick follow-up. GIF/BMP/TIFF/WebP not planned.
- **The bundled default stamp-image set** (`src/assets/stamps/`) — needs asset/
  resource plumbing to reach the backend; the C3a text library covers built-ins, so
  this is polish.
- **Flate-compressing the embedded image** — uncompressed for v1 (a 500×500 RGBA ≈
  1 MB). `lopdf`'s `Stream::compress` (flate2-backed) is the follow-up.
- **CMYK / 16-bit images** — rejected with a typed error (16-bit would need a strip
  or a wider XObject).
- **No resize / move / rotate of a placed stamp** — fixed default size, dropped
  centred on the click; shared with the markup/shape select-and-edit follow-up.
- **No persistent custom-stamp manager** — a typed custom label isn't saved for
  reuse; re-type each time.
- **Fixed-width centring estimate** — the label is centred with a single average
  glyph-em (0.62, Helvetica-Bold); very wide/narrow strings sit slightly off-centre.
  A real metrics table (or measuring with the base-14 widths) would be exact.
- **A shared click / multi-click tool lifecycle is now overdue.** Four annotation
  overlays own their own gesture outside `stepTool` (note, polygon, ink, stamp).
  The "rule of three" is well past — a small framework primitive for click-to-place
  + multi-click (vertices) would dedupe `NoteLayer`/`PolygonLayer`/`InkLayer`/
  `StampLayer` boilerplate. Do it before the next such tool (measure, C4).

## From the P3.C2 verification sweep

- **The per-edit "refresh flash" — scroll-jump FIXED 2026-06-25; the page-blank
  flash remains.** Every annotation add/remove/edit bumps the edit-epoch, and
  `PdfViewer`'s doc-load effect `setDoc(null)`s on that change — unmounting the
  page view, showing "Opening…", then rebuilding it. **Fixed the scroll-jump**:
  `PdfViewer` now captures the exact `scrollTop` (via a new
  `PageVirtualizerHandle.getScrollTop`) before the reload and passes it as
  `initialScrollTop`, which the virtualizer restores after re-measure (page
  heights + scale are unchanged across an annotation edit, so the px offset maps
  back exactly) — replacing the old jump-to-page-*top* restore. The remaining
  **blank/flash** (the page briefly disappears during the reload) still wants the
  doc-level double-buffer (keep the old doc mounted, swap a freshly-loaded one in
  when ready); a first attempt at that skewed page-geometry timing (shapes
  off-spot, ovals→circles) and was reverted — redo it with the dev app open.

## From the 2026-06-25 verification sweep (Phase-3 in-app pass)

User walked the `[~]` features in `npm run dev`. Most passed (C1a/b, C2, C3a/b,
C4a/b, B3b). Four issues found + fixed this session:
- ✅ **Draw scrolled to the page top** — the exact-scroll restore above.
- ✅ **Free-text never wrapped** — `font_avg_em` under-estimated width, so wrapped
  lines ran past the `/AP`'s clipped right edge (looked un-wrapped). Bumped the
  ems (0.6 / 0.62 bold) to bias wide so lines wrap inside the box. *Estimate, not
  AFM metrics — exact widths still a follow-up.*
- ✅ **Filter reset to the full list on delete** — the `AnnotationPanel` was gated
  on `&& doc`, so each edit reload (`setDoc(null)`→value) unmounted + remounted it,
  wiping its `filter`/search/composer state. Removed the `doc` gate (the panel
  reads via the actor, not PDF.js); the `documentId` key still remounts on a real
  switch.
- ✅ **Reply affordance unclear (D2)** — the `💬 Reply` button was faint gray text;
  made it a visible bordered blue button with a count ("💬 2 replies").
- ⏳ **Add a note/comment from the sidebar** — the user expected an Acrobat-style
  "add comment" in the panel; today notes are placed via the page Note tool and
  the sidebar only *replies* to existing annotations. A "new note" affordance in
  the sidebar needs a placement decision (where on the page?) — deferred as a
  small feature.

## From P4.A1 (text-run extraction)

- ✅ **DONE 2026-06-25 (A1) — read-only text-run extraction** (live PDFium,
  `pdf/text_extract.rs`). Carry-forwards for the rest of the text engine:
- **Run granularity is author-dependent** — one PDFium text object may be a glyph,
  a word, or a whole line (whatever a `Tj`/`TJ` emitted). B1's editor may want to
  **merge** adjacent same-style runs on a line into one editable field; not A1's job.
- **`fontName` is not the `/Font` resource key** — PDFium gives a (subset-stripped)
  font *name*, but a byte-level edit (A3 reflow) needs the page's `/Font` resource
  + encoding to re-emit glyphs. Mapping name→resource (or going through PDFium's
  own text-set API) is an A3/B1 problem.
- **Loose AABB for rotated/skewed text** — `bbox` is axis-aligned (over-covers);
  the `transform` matrix is shipped so B1 can compute the true oriented box. Fine
  for coarse hit-testing now.
- **Per-page, whole-page payload** — a text-heavy page returns every run in one
  `Vec`. Fine for click-to-edit (one visible page on demand); cap/stream if a
  pathological page ever bites.
- **CMYK/pattern fills → approximated** — `fill_color` normalizes to `#rrggbb`; a
  pattern/shading text fill (rare) falls back to black.

## From P4.A2 (font fallback resolver)

- ✅ **DONE 2026-06-26 (A2) — font resolver + once-per-doc banner** (`pdf/font_resolver.rs`,
  `FontFallbackBanner.tsx`). Resolver + warning are gated; the re-flow *action* and the
  in-app banner eyeball are deferred. Carry-forwards:
- **Re-flow action is a no-op until B1** — the banner's "Re-flow affected text" button
  is rendered **disabled** (the spec's *offer*, not the *action*). Wire it to the real
  reflow (A3) when B1 lands, and flip P4.A2 `[~]` → `[x]` after an in-app eyeball.
- **System-font check is a file-stem heuristic** — `load_system_fonts` matches on the
  normalized font-file *stem*, not the parsed `name`-table family. So a face whose file
  is named oddly may be **falsely warned** (deliberate bias: warn-when-unsure). A precise
  check needs a font-parsing crate (`ttf-parser`/`fontdb`) we declined — revisit only if
  false warnings become noisy.
- **No user-chosen substitute** — the fallback face is auto-picked (serif→Times,
  mono→Courier, else Helvetica). A per-font "use this instead" picker is a later polish.
- **Per-document warning, not per-run** — one banner names every missing font on open.
  B1 may additionally want an *inline* per-run confirm at edit time ("this run → Helvetica").
- **Arial≡Helvetica metrics** — both map to the base-14 Helvetica widths; we don't ship
  a true Arial. Fine for layout (same metrics), a caveat for pixel-exact diffs.
- **Open-time scan cost** — `collect_document_fonts` walks every page's objects under the
  PDFium lock once on open. Cheap for normal docs; if a huge doc ever stalls, make the
  report fetch lazier (it's already off the critical render path, fetched async).

## From P4.A3 (text editing) — shipped EDIT-only; redact half BLOCKED

- ✅ **DONE 2026-06-26 (A3) — in-place text edit** (`pdf/reflow.rs::replace_text_run`).
  Edits a run's text via PDFium `set_text` on a throwaway doc, `Manual`-staged + one
  `regenerate_content`, swap-the-live-doc with a `RestoreDocEdit` inverse. Read-for-B1
  infra; no actor/IPC/UI yet.
- ⚠️ **`FPDFPage_RemoveObject` SIGSEGVs in our bundled PDFium** (diagnosed to the FFI call:
  stderr markers enter, never return; reproduced with 1 and 2 page loads). We **route around
  it** rather than fix it:
    - **P4-EDIT-004 (delete text)** — ✅ **DONE 2026-06-26 (B3)** via lopdf content-stream
      splice (`reflow.rs::delete_text_run`), not PDFium. No longer blocked.
    - **P6-SEC-010 (true redaction)** — unblocked: will **reuse `delete_text_run`** for the
      text-removal clause (region selection + image removal still to build).
    - **Fallback-font recreate** — still deferred (needs *remove + create* of an object,
      which the lopdf splice doesn't do — editing a non-embedded run keeps the font ref;
      A2 warns). Revisit if/when a newer PDFium binary is evaluated (touches the bundled
      `libpdfium.dylib`; needs a human OK per CLAUDE.md).

## From P4.B3 (delete text — lopdf content-stream surgery)

- ✅ **DONE 2026-06-26 (B3)** — `delete_text_run` splices a run's `Tj`/`TJ` out of the page
  content stream and **verifies by re-extraction**; wired as the Edit Text **Delete** button.
  Pending the in-app eyeball.
- **`'` / `"` operators rejected** — these advance the line as they show, so removing them
  would shift following text. Rare; errors cleanly. Support them (convert to a move-only op)
  if a real document ever needs it.
- **XObject-embedded text rejected** — glyphs inside a Form XObject have no show operator in
  the *page* stream, so the ordinal won't resolve → clean error. Deleting them would mean
  editing the XObject stream (and it may be shared) — out of scope.
- **Rewritten stream is uncompressed** — `change_page_content` writes one plain stream (no
  `/FlateDecode`). Valid, slightly larger; re-compress later if size matters.
- **No neighbour reflow** — the deleted run leaves its gap (or the line closes up if runs
  shared a cursor); true reflow needs the line model (shared with the A3/B1 carry-forward).
- **Two PDFium loads per delete** (before + after verify) — fine for interactive delete;
  revisit only if batch/region redaction makes it hot.

## From P4.B2 (add text box — page content)

- ✅ **DONE 2026-06-26 (B2)** — `add_text_box` appends a `Tj` fragment to the page content
  stream (not an annotation); the **Add Text** tool. Pending the in-app eyeball. The added
  text is real content → editable/deletable via B1/B3 for free.
- **Base-14 fonts only** — Helvetica/Times/Courier (the families that render without
  embedding). A custom/embedded-font picker is a separate feature (font embedding).
- **No on-page auto-grow** — text wraps within the drawn box (free-text's `wrap_lines`); the
  box doesn't expand on the page if the text overflows (unlike the free-text *annotation*,
  which grows its `/Rect`). Overflow clips at the box edge.
- **Rewritten content is an extra uncompressed stream** — appended as a plain `/Contents`
  entry (no `/FlateDecode`). Valid, slightly larger; re-compress later if size matters.
- **No edit-as-a-unit** — once committed it's ordinary content text; there's no "edit this box"
  affordance, you edit/delete the run(s) via B1/B3. Intentional, but a power user might expect
  to reselect the whole box.
- **Subset embedded fonts may tofu** — `set_text` with characters outside an embedded
  *subset* font's glyph set renders missing glyphs. This is the lossiness A2 warns about;
  detecting per-glyph coverage is deferred.
- **Rotated/skewed runs** — `set_text` preserves the matrix (good), so rotation is fine
  for *edit*; only the (deferred) recreate path would need to re-apply a matrix.

## From P4.C2 (edit existing image)

- ✅ **DONE 2026-06-27 (C2)** — move/resize/rotate (PDFium `reset_matrix`) + delete (lopdf
  splice). RISK #1 resolved: `reset_matrix` mutate-in-place works (no SIGSEGV). Pending the
  in-app eyeball.
- ✅ **FIXED — `ETq` content-stream merge.** `append_page_content` (C1/B2) lacked a leading
  separator; lopdf concatenates `/Contents` array streams without inserting the spec-required
  whitespace, so `…ET`+`q` fused into a bogus token and broke multi-image delete. Prepended `\n`.
  (PDFium masks this on read, which is why it surfaced only via the lopdf delete path.)
- ✅ **DONE 2026-06-28 (C2b) — replace** (`replace_image`): embed the new PNG/JPEG, overwrite the
  referenced `XObject` in place (name/`cm`/`Do` untouched). The "original data preserved unless
  replaced" clause is now satisfied (every other op leaves pixels alone; replace is the explicit
  swap). **Aspect re-fit not done** — the new image fills the old box; if its aspect differs it
  stretches (resize via the handles). A re-fit-on-replace pass is the noted follow-up. **Single
  instance of a shared XObject** still can't be replaced independently (would need a fresh name +
  content rewrite) — rare for VibePDF-added images.
- **Resizing a rotated image resets rotation** — `rectToMatrix` produces an axis-aligned matrix,
  so a resize after a rotate squares it up. Acceptable; a rotation-aware resize is later polish.
- **90° rotation only** — no free-angle drag handle (the matrix supports any angle; the handle +
  snapping UX is deferred).
- **Best on identifiable images** — delete relies on finding the image's `Do`; an image inside a
  Form XObject (or shared across pages) won't match → the verify errors rather than corrupt.
- **Selection doesn't persist across the edit** — after move/resize the epoch reload re-extracts;
  the box re-derives from the new geometry. Fine, but rapid successive edits each round-trip.

## From P4.D1 (background)

- ✅ **DONE 2026-06-29 (D1a, P4-EDIT-008)** — colour fill + image background behind page content on a
  range, with opacity. New `background.rs` (always prepends; image cover-fit + clipped to the
  MediaBox) + `BackgroundDialog`. Verified via Apple PDFKit render (corner pixel = the fill colour).
  Pending the in-app eyeball (`[~]`).
- ✅ **DONE 2026-06-29 (D1b, P4-EDIT-008)** — PDF-page background. `import_page_as_form` renumbers the
  source above the dest (`renumber_objects_with`), copies **only** the page's resource object closure
  (BFS, not the whole source), wraps the page content in a `/Form` XObject (`BBox` = source
  `MediaBox`), drawn contain-fit + centred. Verified via Apple PDFKit (imported page's text renders
  behind the host page's). Pending the in-app eyeball (`[~]`).
- **Source page `/Rotate` ignored** — a rotated source page imports unrotated (Form XObjects don't
  carry page rotation). Bake-rotation-into-the-matrix is the follow-up.
- **Contain-fit only** for the PDF-page source (whole page visible, centred) — no cover/stretch/
  position. Source `/CropBox` not honoured (uses `/MediaBox`).
- **Encrypted source PDFs** error cleanly (lopdf can't load) — no decrypt-on-import.
- **Image is cover-fit only** (fills + crops); no contain/stretch/tile toggle, no position control.
- **Solid colour only** — no gradients/patterns.
- **Can't edit/remove an applied background** — page content; removal is in-session undo. A
  "remove background" pass (strip our `q…Q` by marker) is possible later.
- **Consolidation:** `parsePageRange` now lives in `src/tools/page-range.ts` and `page_media_box` in
  `cos.rs` (`pub(crate)`) — shared by watermark + background (and Track D's remaining features).

## From P4.HF (FABLE_REVIEW hardening)

- ✅ **DONE 2026-07-06 (P4.HF)** — review items **3.1** (decorations compensate `/Rotate`),
  **3.4** (placement vs. CropBox; colour fill stays MediaBox), **3.7** (`/Contents` ref→array),
  **3.3** (encrypted docs were **unsaveable** — verify now uses the open password; save works,
  encryption preserved, pinned by test). Fixtures `rotated.pdf` + `cropped.pdf` added.
- ✅ **DONE 2026-07-06 (P4.HF2, review 3.13)** — decorations tagged
  `/VibePDF << /Kind (…) /Id (uuid) >> BDC … EMC`; splice-proof test shows removal is a
  mechanical operator drop. **Unlocks a future `remove_decoration(kind/id)` feature** (find the
  tagged range in `get_and_decode_page_content`, drain, re-encode — the `delete_image` pattern).
  Note: PDFium compresses content streams on save, so the tag isn't raw-`grep`-able in saved
  files — find it at the operator layer.
- ✅ **DONE 2026-07-06 (P4.HF3, review 3.2 stage-1 + 3.5)** — text writers WinAnsi-transcode
  Latin-1/CP1252 + `/WinAnsiEncoding` font, and **reject** non-WinAnsi text with a typed
  character-naming error (all 7 text entries); failed canvas-tool writes now show **toasts**
  (`toast-store` + `Toasts` + `report-error`) instead of `console.warn`.
- ✅ **STARTED 2026-07-12 (P4.HF5, review 3.2 stage-2)** — true Unicode via **PDFium font
  embedding** (no new dep): `font_embed::embed_runs` loads a system TTF (`load_true_type_from_bytes`)
  and places PDFium text objects → `/Type0` + `/CIDFontType2` + `/ToUnicode` + `/FontFile2`.
  `ensure_winansi` → branch (`winansi_fits`); WinAnsi keeps base-14, non-WinAnsi embeds (falls back
  to HF3 reject when no covering font). Tracer wired into **header/footer**. Committed fixture
  `tests/fixtures/fonts/NotoSansCoptic-Regular.ttf` (28 KB, OFL).
- ✅ **DONE 2026-07-12 (P4.HF6)** — **Font subsetting (was the top follow-up).**
  `font_embed::subset_font` subsets the face to the used glyphs before embedding, via `subsetter`
  0.1 + `ttf-parser` 0.25 (MIT/Apache, zero-dep). Cyrillic footer **15 MB → 60 KB**. PDFium's
  native `FPDF_SUBSET_NEW_FONTS` flag was unreachable through pdfium-render 0.9.1 (a future
  pdfium-render bump could switch to it and drop these deps).
- ✅ **DONE 2026-07-12 (P4.HF7)** — **Watermark** converted (second text writer). `EmbedRun` gained
  `opacity` (→ PDFium fill alpha) + `behind` (→ `insert_object_at_index(0)`); `cos::compose` bakes
  the rotate-about-centre transform into the run matrix. Cyrillic/Greek watermarks render, ~56 KB.
- ✅ **DONE 2026-07-15 (P4.HF8)** — **Text box** converted (third, last page-content writer).
  `EmbedRun.underline: Option<f32>` (PDFium path rule); `add_text_box_embedded` reuses the base-14
  `wrap_lines` layout → one run per line. Multi-line Cyrillic + underline render, ~48 KB.
- ✅ **DONE 2026-07-15 (P4.HF9)** — **Free-text `/AP`** converted (the annotation class). New
  `font_embed_cid::build_cid_font` hand-builds a Type0/CIDFontType2 in lopdf (subsetter + ttf-parser
  → /FontFile2 + /FontDescriptor + /W + /ToUnicode, Identity-H). `free_text_appearance` (add +
  update) branches on `winansi_fits`; /Contents keeps plain text for re-edit. ~24 KB, searchable.
- ✅ **DONE 2026-07-15 (P4.HF10)** — **Stamp + image-stamp labels** converted (the last text
  writers). `show: Fn(&str)->String` closure on the two stamp content builders; `add_stamp` /
  `add_image_stamp` branch on `winansi_fits(label.to_uppercase())` → `build_cid_font`. **Stage-2
  writer surface complete: all 7 rendered-text entries embed Unicode.** The `ensure_winansi`
  char-naming test graduated to a direct unit test (no writer rejects unconditionally now).
- **⏭️ TODO — 3.2 stage-2 residual polish (each its own small ship):**
  1. **CID-path unification** — the hand-built-CID `/AP` path is strictly more capable than the
     PDFium page-object path (real metrics, marked-content tags, no PDFium round-trip). Consider
     retiring HF5–HF8's PDFium path onto `build_cid_font`, which would also fix their HF2-tag gap
     and 3.10 metrics in one move. Larger refactor; design first.
  3. ~~**`/FontFile2` compression** (FlateDecode)~~ **DONE FOR FREE** — investigated P4.HF11:
     lopdf's `save_to` already FlateDecode-compresses every stream on save (a 332 KB subset stores
     as 20 KB). No helper needed; see FABLE_REVIEW §3.12.
  3. **Per-glyph coverage in `covering_font_bytes`** — today it returns a broad face without
     verifying the glyphs exist; an exotic script shows `.notdef`. Needs a coverage probe.
  4. **HF2 marked-content tag on embedded runs** — PDFium objects aren't wrapped in `/VibePDF`
     BDC/EMC, so an embedded decoration isn't splice-removable (blocks "remove watermark/footer").
  5. **Exact embedded-font metrics** for centre/right alignment (shared with 3.10; today uses the
     base-14 `font_avg_em` estimate).
- **Note (3.2):** `/WinAnsiEncoding` renders `'`/`` ` `` straight instead of curly — pre-HF3
  free-text/stamps with apostrophes change appearance slightly (arguably more correct).
- **Note (3.5):** toasts cover *user-action write* failures; passive read/sync failures still
  `console.warn` only. Re-editing a pre-HF3 annotation that stored non-WinAnsi (mojibake) text now
  errors on save — rare; the message explains.
- ✅ **DONE 2026-07-11 (P4.HF4, review 3.14)** — `background.rs::collect_refs` (walks the
  **untrusted source PDF**'s resource graph during D1b import) is now iterative (two worklists:
  `pending` ids + `inline` stack), so a crafted deep container chain can't overflow the actor
  thread's stack. Pinned by `background::tests` (100k-link chain + a reference cycle). Discovered:
  lopdf `get_object` collapses bare `M 0 R` chains, so the overflow shape is *container* links.
- **Still open from FABLE_REVIEW** (each its own ship): **3.10** AFM glyph metrics, **3.15**
  assorted. (**3.6** undo memory → byte-budgeted P4.HF13; **3.8** CSP → strict policy P4.HF14;
  **3.9** Windows path + CI leg → P4.HF15; **3.11** dirty-flag → fixed P4.HF12; **3.12** stream
  compression → non-issue, P4.HF11.)
  - **Note (P4.HF15):** the Windows CI leg runs `check` + frontend tests only. The Windows **Rust
    PDF suite** is deferred — needs a `fetch-pdfium.sh` Windows branch (`pdfium-win-x64` → `pdfium.dll`)
    and a per-platform skip of the macOS-arm64 `render_compare` golden. That's also where the
    review's other Windows risks (`std::fs::rename`-onto-existing semantics, PDFium load) would
    finally get exercised.
  - **Note (P4.HF14):** the CSP includes `'wasm-unsafe-eval'` for PDF.js v5's WASM decoders, but
    they aren't wired up (`wasmUrl` unset; `.wasm` files not in `public/pdfjs/`). Wiring
    OpenJPEG/JBIG2/QuickJS so scanned-PDF images decode is a separate follow-up; the CSP already
    permits it. The CSP also still needs a **manual in-app smoke test** per webview platform
    (ties into the §3.9 Windows/Linux CI leg).
- **Note:** annotation `/AP` writers (shapes, notes, free-text) still place in page space —
  viewers rotate annotations themselves, so they were *not* part of the 3.1 bug; re-check only
  if a reader renders them oddly on rotated pages.

## From P4.D3 (header/footer)

- ✅ **DONE 2026-07-01 (D3, P4-EDIT-010)** — left/centre/right header/footer text over a page range,
  font/size/colour/margin, with `{n}`/`{total}`/`{date}` placeholders substituted per page. `{date}`
  value supplied by the frontend (no Rust date dep). New `header_footer.rs` + `HeaderFooterDialog`.
  Verified via Apple PDFKit (per-page "Page N of 50" + date). Pending the in-app eyeball (`[~]`).
- **`{n}` is the absolute page number.** Start-number offset and `1/N` / `Page 1 of N` presets /
  roman (`i`,`I`) / alpha (`a`,`A`) numbering are **P4.D4 (page numbers)**, not D3.
- **Right/centre alignment uses an estimated glyph width** (`font_avg_em`), so it can be a few points
  off with proportional fonts — inside the margin, acceptable. A real glyph-metrics table would fix
  this here *and* in watermark centring (shared follow-up).
- **One `{date}` format** (whatever the frontend sends, default `YYYY-MM-DD`) — no in-dialog format
  picker/locale control.
- **Single line per position** — no multi-line headers, odd/even (mirrored) headers, or a background
  box behind the header text.
- **Consolidation:** `escape_pdf_string` now lives in `cos.rs` (`pub(crate)`), shared by watermark +
  header/footer (and D4).

## From P4.D2 (watermark)

- ✅ **DONE 2026-06-29 (D2, P4-EDIT-009)** — text/image watermark over a page range, on top of or
  behind content, with opacity (`/ExtGState`) + rotation (`cm` about the page centre). New
  `watermark.rs` module + `WatermarkDialog`. 50 pages in ~0.12 s (budget 2 s). Pending the in-app
  eyeball (`[~]`).
- **Single centred mark, not tiled.** No repeating/grid watermark across the page — one centred
  instance. A tiled mode (cover the page) is a follow-up.
- **No live preview.** The watermark is apply-then-render (epoch bump), like the other
  document-wide ops; you don't see it until it's applied (then undo if wrong).
- **Can't edit/remove an applied watermark as an object.** It's page content, so removal is
  in-session undo; a saved file's watermark isn't separately selectable. A "remove watermark" pass
  (find + strip our `q…Q` fragment by marker) is possible later.
- **Base-14 text + PNG/JPEG image only** (matches `embed_image`); no embedded fonts, no
  GIF/BMP/TIFF.
- **Image fit is ~70% of page, centred.** No control over watermark image scale/position yet.
- **`many-pages.pdf`** (50pp) added as a shared Track-D fixture — reuse for D3/D4/D5.

## From P4.C3 (hyperlinks)

- ✅ **DONE 2026-06-28 (C3, P4-EDIT-007)** — `cos::add_link` writes a `/Link` annotation over a
  dragged region: URL / `mailto:` email (`/A /URI`), internal page (`/Dest [pageRef /Fit]`, the
  form the P2 reorder/delete fixups resolve), named destination (`/Dest (name)`). `(value)` escaped
  by `string_literal`. The **Add Link** tool. Pending the in-app eyeball (`[~]`).
- ✅ **DONE 2026-06-28 (C3b, P4-EDIT-007b)** — link appearance: **box** (default) / **underline** /
  **invisible**, in a chosen colour. Visible styles carry a generated `/AP` (Form XObject, 1pt
  stroke) so they render in every reader, not just on hover; invisible stays `/Border [0 0 0]`,
  no `/AP`. Popover gained a Style select + colour picker. Verified box/underline/invisible via
  Apple PDFKit.
- **Single colour, fully opaque, static appearance.** No opacity, no dashed/variable-width borders,
  no rollover/down (`/AP` `/R`,`/D`) states. Add if demand appears.
- **Can't re-style an existing link** — appearance is set at creation; changing it needs the
  deferred link-edit UI.
- **Add only — no link-edit UI.** You can't move an existing link's rect or retarget it from the
  UI; delete works generically (annotation delete / dangling-dest prune). A dedicated link-edit
  overlay (select an existing `/Link`, drag its box, change the target) is deferred.
- **Region rect, not a text-snapped quad.** The link box is whatever you drag; it doesn't snap to
  the glyph quads of a text selection. "Hyperlink the selected *text*" (precise quads) is later
  polish — the spec allows either text or region.
- **No auto-linking.** We don't scan page text for bare URLs and convert them to links in bulk.
  Separate feature if demand appears.
- **References, doesn't create, named destinations.** `named` points at an existing entry in
  `/Names/Dests`; C3 doesn't author the destination itself.
- **URL kind requires a scheme** (`https://…`). A bare `example.com` is rejected in the popover so
  the stored URI is unambiguous to readers; no auto-`https://` prepend.

## From P4.C1 (add image — page content)

- ✅ **DONE 2026-06-27 (C1)** — `add_image` embeds a PNG/JPEG as a content-stream Image XObject
  (`q <cm> /Img Do Q`); the **Add Image** tool. Pending the in-app eyeball. Real content → C2
  edits it.
- **PNG + JPEG only** — GIF/BMP/TIFF/WebP (the other formats P4-EDIT-005 lists) need a raster
  decoder we don't bundle; they error cleanly. Add via a decoder crate if real demand appears.
- **No rotation** — deferred to **C2** (move/resize/**rotate**); C1 is place + aspect-fit. The
  `cm` matrix already supports rotation, so C2 just supplies the angle.
- **CMYK / Adobe-marker JPEGs** — `embed_jpeg` maps 4 components → `DeviceCMYK` but doesn't
  honour an APP14 transform / inverted-`/Decode`; an Adobe CMYK JPEG may render inverted. Rare;
  revisit if it bites (add `/Decode [1 0 1 0 1 0 1 0]` when the APP14 transform says so).
- **No down-sampling / recompression** — the image embeds at its source resolution (PNG raw
  uncompressed; JPEG verbatim). A huge photo bloats the PDF. A "fit to N dpi" pass is a later
  optimization (shared with the stamp's same caveat).
- **Aspect-fit, not free-resize** — the image fits (centred) inside the drawn box; you can't
  stretch or set an exact size at add time. Post-placement resize is **C2**.

## From P4.B1 (click-to-edit text)

- ✅ **DONE 2026-06-26 (B1) — Edit Text tool** (`TextEditLayer` + `ReplaceTextRun`).
  The consumer that surfaces A1+A2+A3. Pending the in-app eyeball that flips A1/A2/A3 → `[x]`.
- **Run granularity is whatever PDFium emits** — a "run" may be a word, a line, or a glyph
  (A1 caveat). B1 edits exactly that unit; merging adjacent same-style runs into one
  editable field is a later nicety, not wired.
- **No re-wrap / neighbour shift on long edits** — a much longer replacement is placed at
  the same origin and may visually overrun; neighbours don't move. Needs the line model
  (shared with the A3 reflow carry-forward).
- **No style-change picker** — B1 preserves the run's existing font/size/colour; changing
  *style* while editing (bold a word, recolour) is future work.
- **Editor preview font is approximate** — `cssFamilyForFont` buckets serif/mono/sans for
  the on-screen editor only; the saved file keeps the real font (`set_text`). Cosmetic.
- **Rotated-text editor geometry is best-effort** — the editor box is axis-aligned over a
  rotated run's loose AABB; the *edit* still applies correctly (matrix preserved).
- **Per-page run fetch on tool activate** — `extractTextRuns` runs for the visible page
  when Edit Text is on (and on each epoch). Fine for normal pages; a pathological page
  could be cached if it ever bites.

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
