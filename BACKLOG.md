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

- **No rich text / underline (`/RC` + `/DS`)** — B3a is one uniform style per box.
  Mixed runs/colours, underline, and the XHTML-subset rich-content appearance are
  **B3b** (the deferred half of P3-ANN-003).
- **No auto-wrap** — the `/AP` honors explicit `\n` only; the editor's soft-wrap
  won't match if the user relies on it. Measure-and-wrap in the `/AP` is B3b.
- **No re-edit of a committed box** — once added, you can ⌘Z but not double-click
  to edit (needs a read-back like B2b). Lands with B3b / the D1 sidebar.
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

## Verification debt (in-app + cross-reader, deferred by the user)

The automated suites (`check` / `test` / `test:rust`) are green; these shipped
`[~]` features still want a human in-app + cross-reader pass. **2026-06-21 the user
said "push testing to backlog verify"** — keep shipping, surface these at
phase-close. Walk `MANUAL_TESTING.md` + the `Sample PDFs/vibepdf-verify-*.pdf`
artifacts and flip the passing `[~]`→`[x]` in `steps/P3.md`.
- **C1b₁** line/arrow, **C1b₂** polygon (placement eyeballed 2026-06-21;
  cross-reader pending), **C2** ink, **C3a** stamps, **C4a** measurements.
- **D1** sidebar UI, **D1d** select+delete, **D1e** free-text edit-in-place.
- **D2** reply threads, **E1** XFDF round-trip (draw → ⬆ Export → delete all →
  ⬇ Import → restored + reply still threaded; then open the `.xfdf` in Acrobat
  against the same base PDF — artifacts at `Sample PDFs/vibepdf-verify-xfdf.{pdf,xfdf}`).
- **E2** flatten (draw markup → ▦ Flatten → confirm → markup stays visible but
  leaves the sidebar + isn't selectable → ⌘Z restores → ⌘S, reopen → baked, not
  selectable; then cross-reader — artifact `Sample PDFs/vibepdf-verify-flatten.pdf`).
- **C4b** `/Measure` (Calibrate → measure → ⌘S → reopen → tool stays calibrated
  without re-calibrating; then open the saved PDF in Acrobat and confirm its
  measuring tool reports the same value live — artifact `…verify-measure.pdf`).
- The **per-edit refresh flash** is a separate *open bug* (reverted; below), not
  verification debt.

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
- **Image stamps (C3b)** — the other half of P3-ANN-006: custom stamps from an
  **image** (and image+text). Needs image XObject embedding (read file → `/Image`
  XObject → `Do` in the `/AP`), a bundled default stamp-image set in
  `src/assets/stamps/`, and aspect-aware placement.
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

- **The per-edit "refresh flash" (deferred — needs eyes on the pixels).** Every
  annotation add/remove/edit bumps the edit-epoch, and `PdfViewer`'s doc-load
  effect `setDoc(null)`s on that change — unmounting the whole page view, showing
  "Opening…", then rebuilding it. So each edit blanks the page (and jumps scroll).
  The fix is to keep the current doc mounted and **swap a freshly-loaded one in
  when ready** (doc-level double-buffer), paired with a canvas-level double-buffer
  in `PageSlot`. A first attempt skewed the page-geometry/scale timing — shapes
  rendered off-spot, ovals→circles — so it was **reverted** to the known-good
  path. Redo it with the dev app open so the geometry can be eyeballed while
  iterating; add a `PageVirtualizer`/`PdfViewer` render test if one is feasible.

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
