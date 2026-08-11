# 04 — Architecture

The whole project is organized around one hard line: **the Rust backend owns the PDF.** The frontend reads a derived, serializable view of the document and dispatches intent. It does not own the bytes.

This is the boundary that keeps the editor correct.

---

## Top-level layout

```
vibepdf/
├── src/                          # Frontend (React + TS)
│   ├── app/                      # App shell, routing, layout
│   ├── view/                     # Rendering (PDF.js wrapper)
│   ├── tools/                    # Edit tools (one folder per tool)
│   ├── panels/                   # Sidebars: thumbnails, outline, annotations
│   ├── ipc/                      # Typed wrappers around Tauri invoke()
│   ├── state/                    # Zustand stores
│   ├── styles/                   # Tailwind config + globals
│   └── main.tsx
│
├── src-tauri/                    # Rust backend
│   ├── src/
│   │   ├── main.rs               # Tauri setup, capability config
│   │   ├── commands/             # Tauri commands (the IPC surface)
│   │   ├── pdf/                  # Core PDF engine (PDFium wrapper)
│   │   │   ├── document.rs       # Open, save, metadata
│   │   │   ├── cos.rs            # lopdf object-model layer (structural edits) + annotation add_* incl. add_link (P4.C3)
│   │   │   ├── page.rs           # Page operations
│   │   │   ├── text.rs           # Text editing
│   │   │   ├── annotation.rs     # Annotations
│   │   │   ├── xfdf.rs           # XFDF annotation import/export (P3.E1)
│   │   │   ├── flatten.rs        # Flatten annotations into page content (P3.E2)
│   │   │   ├── image_xobject.rs  # PNG → Image XObject + /SMask (P3.C3b)
│   │   │   ├── text_extract.rs   # Text-run extraction + doc font scan (live PDFium read, P4.A1/A2)
│   │   │   ├── font_resolver.rs  # Font fallback: base-14/system check + substitute (pure, P4.A2)
│   │   │   ├── reflow.rs         # Text-run edit (PDFium set_text, P4.A3/B1) + delete (lopdf splice, P4.B3)
│   │   │   ├── image_extract.rs  # Locate page images (live PDFium read, P4.C2)
│   │   │   ├── image_edit.rs     # Image transform (PDFium reset_matrix) + delete + replace (lopdf) (P4.C2/C2b)
│   │   │   ├── watermark.rs      # Text/image watermark over selected pages, on-top/behind (P4.D2)
│   │   │   ├── background.rs     # Colour/image/PDF-page background behind page content (P4.D1)
│   │   │   ├── header_footer.rs  # Header/footer text with {n}/{total}/{date} placeholders (P4.D3)
│   │   │   ├── form.rs           # Form fields
│   │   │   ├── form_data.rs      # Form-data export: FDF/XFDF/JSON/CSV (P5.C1)
│   │   │   ├── form_import.rs    # Form-data import + match/mismatch report (P5.C2)
│   │   │   ├── form_flatten.rs   # Synthesize field appearances, bake, drop AcroForm (P5.C2)
│   │   │   ├── render.rs         # Rasterization (for thumbnails, export)
│   │   │   └── actor.rs          # Single-threaded document actor
│   │   ├── ocr/                  # Tesseract pipeline
│   │   ├── security/             # Crypto, signatures, redaction
│   │   │   ├── encrypt.rs
│   │   │   ├── sign.rs
│   │   │   └── redact.rs
│   │   ├── ai/                   # Local AI integration
│   │   │   ├── ollama.rs
│   │   │   ├── ner.rs
│   │   │   └── embed.rs
│   │   ├── convert/              # Format conversions
│   │   └── settings/             # Persisted settings
│   │       ├── recents.rs        # Last 20 opened files (P1-VIEW-012)
│   │       ├── session.rs        # Restored tabs/scroll (P1-VIEW-011)
│   │       └── signatures.rs     # Signature library: index.json + PNG blobs (P6.A1)
│   ├── Cargo.toml
│   ├── tauri.conf.json
│   └── capabilities/             # Tauri 2 capability files
│
├── tests/
│   ├── fixtures/                 # Sample PDFs
│   │   ├── basic/                # Simple docs for smoke tests
│   │   ├── edge-cases/           # Encrypted, large, malformed, XFA, etc.
│   │   ├── conformance/          # W3C PDF conformance suite (subset)
│   │   └── acceptance/           # Per-phase acceptance documents
│   ├── integration/              # Rust integration tests
│   └── e2e/                      # WebdriverIO + tauri-driver (Linux/Windows; no macOS)
│
├── docs/                         # The specs you're reading
├── .claude/                      # Claude Code config
├── CLAUDE.md
└── package.json
```

Claude does NOT add new top-level directories. If a need arises, update this doc first.

---

## The document actor

Every open PDF lives behind an actor.

```
┌─────────────────────┐                ┌──────────────────────┐
│  Frontend (React)   │   invoke()     │  Tauri command       │
│  - dispatches       │  ───────────►  │  layer (commands/)   │
│    intents          │                │  - validates input   │
│  - renders view     │                │  - finds actor by    │
│  - never holds      │   event        │    document id       │
│    PDF bytes        │  ◄───────────  │  - sends message     │
└─────────────────────┘                └──────────┬───────────┘
                                                  │ mpsc::Sender
                                                  ▼
                                       ┌──────────────────────┐
                                       │  Document actor      │
                                       │  (one per open doc)  │
                                       │  - owns PdfDocument  │
                                       │  - serializes ops    │
                                       │  - emits events on   │
                                       │    state change      │
                                       └──────────┬───────────┘
                                                  │
                                                  ▼
                                       ┌──────────────────────┐
                                       │  PDFium (single-     │
                                       │  threaded API)       │
                                       └──────────────────────┘
```

**Why an actor:** PDFium's API is not thread-safe per document. The actor pattern serializes operations cheaply without making us hold a `Mutex<PdfDocument>` across the IPC boundary.

**Why *also* a process-global lock:** PDFium is not thread-safe *across* documents either — two actor threads operating on their own documents still race on PDFium's global subsystems (render cache, page-state, document load/close) and SIGABRT/SIGSEGV. So the actor handles per-document ordering + the async boundary, and a single `pdf::document::PDFIUM_LOCK` serializes **every** PDFium FFI span (load, save, metadata, page lookup, rotate, render, and `FPDF_CloseDocument` on drop) across the whole process. Hold it around the minimal FFI span and never across a call that re-locks (the `Mutex` is not reentrant). Integration tests run single-threaded for the same reason — they open/drop their own documents, which can't take the crate-private lock.

**Identity:** Each opened document gets a `DocumentId` (UUID). All commands take a `DocumentId` and the dispatcher routes the message to that actor's channel.

**Lifetime:** Actors are dropped when the document is closed. If the actor panics, the document is reported as crashed; the user gets a recovery offer from auto-save state.

### Structural edits via `lopdf` (COS layer)

Some edits are dictionary-level surgery PDFium's API can't do: rewriting the
`/Outlines` (bookmarks), the `/AcroForm` (form fields), the page tree, and
indirect references — needed by reorder (P2-PAGE-002), delete ref-cleanup
(P2-PAGE-003), insert-from form fields (P2-PAGE-005), and merge
bookmarks/form-fields (P2-PAGE-008). These go through `pdf::cos` (the `lopdf`
object model), **not** PDFium. See `docs/03` "Structural edits — `lopdf`" for
the why.

**Page resize (P2-PAGE-010) also lives here, for a different reason.** Scaling a
page's content is something PDFium *can* nominally do (`FPDFPage_TransFormWithClip`),
but pdfium-render's wrapper forces a `reload_in_place()` (a documented PDFium
workaround, issue #93) that leaves the document in a state that **SIGSEGVs at
teardown** — reproducibly, in our tests. So `cos::resize_pages` does it at the
COS level instead: wrap each page's content stream with `q <scale-matrix> cm …
Q` (push state, concat a scale matrix, …, pop) and set the new `/MediaBox`. No
PDFium content API, no `reload_in_place`. Annotations aren't re-scaled (their
`/Rect`s keep their coordinates) — a documented limitation in `BACKLOG.md`.

**Authoring annotations also lives here (P3.B1b).** `cos::add_text_markup` builds
a text-markup annotation dict (`/QuadPoints`, `/C`) **plus a generated `/AP`
appearance stream** and appends it to the page `/Annots` — `PDFium` can read and
preserve annotations but can't *author* a coloured one (no colour setter), and
the `/AP` is what makes the markup render in every reader. The committed
annotation is drawn in the main view by **PDF.js** (write → epoch reload → the
canvas renders the `/AP`), consistent with every other edit; the annotation
overlay stays draft/selection-only.

**Two rendering paths, chosen per annotation type.** Markup is *canvas-rendered*
(above). Sticky notes (P3.B2a, `cos::add_text_note`) are different: a `/Text`
annotation deliberately carries **no `/AP`** — by convention a reader draws the
note icon itself from `/Name`, so shipping an `/AP` would double-draw. So notes
are *overlay-rendered*: a dedicated HTML layer (`src/view/note-layer.tsx`,
mounted on top of the SVG `annotation-layer`) paints the icon and its editable
popup from the annotation store, and the store id doubles as the annotation's
`/NM` so later edit/delete edits (`update_text_note` / `delete_annotation`) find
the right one. Rule of thumb: **if we author an `/AP`, the canvas renders it; if
we don't, an overlay must.** Note edits use the shared `cos_edit` helper in
`pdf/annotation.rs` (snapshot → cos transform → reload; inverse `RestoreDocEdit`).

**Free-text (P3.B3a)** sits on the *canvas* side of that split: `cos::add_free_text`
writes a `/FreeText` box whose generated `/AP` *draws the text* (PDF text
operators — `BT`/`Tf`/`Td`/`Tj`/`ET` — over a self-contained base-14 `/Font`
resource), so the PDF.js canvas renders it like markup (write → `bumpEpoch`
reload). The twist is input: text needs an editor, so `src/view/free-text-layer.tsx`
is a *transient* overlay — it holds no committed boxes, only the live drag-preview
and a positioned `<textarea>`; on commit it persists through the actor and the
canvas takes over. So the overlay catalogue is now: notes (persistent overlay,
no `/AP`), free-text (transient editor overlay, canvas-rendered `/AP`), markup
(no overlay, canvas-rendered `/AP`).

**B3b** rounds out the box: **underline** (a stroked rule drawn under each line in
the `/AP`, *outside* `BT/ET`; persisted in a private `/Underline` key so re-edit
recovers it — readers ignore the key but still show the rule), **auto word-wrap**
(a single `wrap_lines` — width estimated as `chars × size × per-family-em` —
feeds *both* the box's grow-to-fit height and the drawn `/AP` lines, so they can't
disagree), and **double-click re-edit** (the otherwise-`pointer-events:none`
overlay renders a transparent hit-zone per committed box, read from
`read_annotations` on the edit epoch; a double-click posts the same
`annotation-edit-store` request the sidebar ✎ uses, reusing the D1e read-back →
editor flow). Rich text (`/RC` + `/DS` mixed-style runs — a runs editor + a
multi-style `/AP`) is **deferred to B3c**.

**Lines + arrows (P3.C1b₁)** are the first *points-based* shape: `cos::add_line`
writes a `/Line` (`/L [x1 y1 x2 y2]`, `/C`, `/BS /W`, `/NM`, plus `/LE [/None
/OpenArrow]` for an arrow) with a generated `/AP` that strokes the segment and an
arrowhead V; the `/BBox` is padded for the head + stroke width. The drag tools
(`line-tools.ts`) and a `LineShape` draft renderer plug into the same
`annotation-layer` lifecycle the rect/ellipse tools use.

**Polygons (P3.C1b₂)** are the first tool whose gesture is *not* a drag —
multi-click (click each vertex, double-click to finish). Rather than extend the
generic `stepTool` lifecycle with a multi-click mode (a new framework pattern),
the polygon follows the note/free-text precedent: a **self-contained overlay**
(`src/view/polygon-layer.tsx`) owns the gesture (vertices in local state,
rubber-band preview, Enter/Esc) and commits via `cos::add_polygon` (a `/Polygon`
or `/PolyLine` with a generated `/AP`). Generalizing into a shared multi-click
lifecycle is deferred until a third such tool exists (the rule of three).

**Freehand ink (P3.C2)** is the second non-`stepTool` gesture. It *is* a drag,
but — unlike the rect/line tools, which only need a start and an end — it captures
the whole sampled path plus per-sample pressure, so it too gets a **self-contained
overlay** (`src/view/ink-layer.tsx`) that owns pointer-capture and accumulates the
stroke. **Smoothing lives on the frontend** (`src/tools/ink/ink.ts`: `simplify`
drops jitter, then `catmullRomResample` interpolates an even, dense Catmull-Rom
spline); the smoothed path is persisted by `cos::add_ink` as an `/Ink` (`/InkList`,
`/C`, `/CA`, `/BS /W`) with a generated `/AP`. The `/AP` is a **variable-width
filled ribbon** (the centreline offset by ±`ink_half_width(pressure)` along each
averaged normal, filled non-zero) — pressure modulation that renders in every
viewer because it's a fill, and that degrades to a constant-width band when the
device reports a uniform pressure (a mouse's constant `0.5`). The `/BBox` pads by
the *max* half-width in the stroke so a hard press isn't clipped.

**Stamps (P3.C3a)** are a click-to-place tool. A toolbar palette
(`src/tools/stamp/StampPalette.tsx`) arms a stamp — a built-in from the library
(`stamps.ts`) or a custom text label — into a one-slot `stamp-store`; the
per-page **self-contained overlay** `src/view/stamp-layer.tsx` reads it and, on a
click, drops it via `cos::add_stamp`. The result is a `/Stamp` whose generated
`/AP` draws a coloured border + the bold uppercase label, centred and auto-fit to
the box (the label width is *estimated* from an average Helvetica-Bold glyph em —
exact metrics aren't needed for one centred line). `/Name` is informational; the
`/AP` is what renders.

**C3b adds image stamps.** `StampSpec` is a `text | image` union; the palette's
**Image…** picker arms an image stamp from a chosen PNG path, and the layer
branches to `cos::add_image_stamp`. A new **`pdf/image_xobject.rs`** decodes the
PNG with the `png` crate's *decoder* (already in the tree — the crate we added for
the render *encoder* ships both; **no new dependency**) and builds a `/Subtype
/Image` XObject; an alpha channel is split off into a grayscale **`/SMask`** so a
transparent signature/logo composites correctly. `add_image_stamp` derives an
**aspect-correct** rect from the click + a default height (clamped to the
`MediaBox`, never stretched — the frontend can't know the pixels' ratio, the
backend can) and builds an `/AP` that paints the image with `Do` plus an optional
overlaid label (the "combination" stamp). It reuses the C3a `/Stamp` dict, so it
reads back as kind `"stamp"` and inherits list/delete. Image data is uncompressed
for v1; the path goes to the backend (read there, like merge/insert-from). **PNG
only** — JPEG and the bundled default *image* set are deferred (BACKLOG).

**Measurements (P3.C4a)** are distance / perimeter / area tools. The maths is
pure + frontend (`src/tools/measure/measure.ts`): a *calibration* (units per
point, captured by drawing a reference and typing its real size) turns point
geometry into real-world values — area scaling by the **square** of the scale. A
self-contained `MeasureLayer` reuses the polygon multi-click (distance
auto-finishes at two clicks); a `calibrating` flag in `measure-store` switches
the same gesture between capturing a reference and persisting a measurement.
`cos::add_measure` writes a `/Line` / `/PolyLine` / `/Polygon` carrying a
dimension **`/IT`**, the value in `/Contents`, and an `/AP` (the geometry plus the
value label) — so it isn't a new subtype, just a shape with an intent, and
`read_annotations` keys off `/IT` to surface it as `"measure"`. **Five** overlays
now own a gesture outside `stepTool` (note, polygon, ink, stamp, measure) — a
shared click/multi-click lifecycle is well overdue (BACKLOG).

**C4b** completes the interop half: `add_measure` also attaches a rectilinear
**`/Measure`** dict (§12.9 — `/X`/`/D`/`/A` `NumberFormat`s; `/X /C` *is* the
calibration, units per point), so Acrobat & co. re-measure *live* against the raw
geometry rather than trusting our baked `/Contents` label. The `/Measure` dict is
also how the calibration **persists**: `read_measure_calibration` reads it back,
and `use-calibration-sync` re-seeds `measure-store` on reopen (guarded so it never
clobbers an in-session calibration). Unit labels in `/Measure` stay ASCII (`sq ft`)
to dodge PDF string-encoding pitfalls; the `²` lives only in the `/AP`.

**Shapes (P3.C1a)** are canvas-rendered like markup: `cos::add_shape` writes a
`/Square` or `/Circle` (`/C` stroke, `/IC` fill, `/CA`, `/BS /W`) with a generated
`/AP` (a path painted `S`/`f`/`B`; the ellipse is the standard four-Bézier kappa
approximation). C1a also *completes* the A2 overlay's promise: the
`annotation-layer` was built with a draft/preview lifecycle that committed to the
store as a placeholder — it now **registers the shape tools and persists** the
committed draft through the actor (`addShape` → `bumpEpoch` → canvas), keeping
only the in-progress draft in the store. Line/arrow/polygon are C1b.

**Reading annotations back (P3.D1).** `cos::read_annotations` is the one read that
spans *all* kinds: it walks every page's `/Annots`, whitelists the subtypes we
author (markup, note, free-text, shapes, line/arrow, polygon/polyline, ink,
stamp, measure — the last detected by a dimension `/IT` on a line/poly), and
returns a flat `AnnotationInfo` list (kind, page, `/Rect`, contents,
author, `/M`-as-epoch, plus `inReplyTo` for D2). A read-only `ReadAnnotations`
actor query feeds `pdf_read_annotations`; the `AnnotationPanel` sidebar re-reads
on `[documentId, edit-epoch]` (the same projection cadence as the note overlay),
so the list tracks edits/undo. Selection is a cross-cutting store
(`annotation-selection-store`) carrying the clicked annotation's `/Rect`, which a
per-page `SelectionHighlightLayer` draws as a dashed box — giving "select" visible
feedback for canvas-drawn kinds that have no overlay of their own.

**Reply threads (P3.D2)** add no new annotation type — a reply is a `/Text` linked
to its parent by **`/IRT`** (per the spec). `cos::add_reply` reuses the note write
+ the shared `resolve_handle` (so the parent is the same `/NM`/`obj:` id the
sidebar uses, and the existing delete works on a reply); `read_annotations`
dereferences each `/IRT` to the parent's handle (`inReplyTo`), and `read_text_notes`
skips `/IRT` annotations so a reply isn't drawn as a stray page icon. Threading is
pure + frontend: `buildThreads` walks `/IRT` chains to a root and nests replies
flat (orphan- and cycle-safe); the sidebar renders the root row + nested replies +
an inline composer, and filters operate on roots only.

**XFDF interchange (P3.E1, P3-ANN-010)** lives in **`pdf/xfdf.rs`** — the one place
that maps between annotation dicts and the XFDF (XML) sidecar Acrobat uses to ship
markup separately from the PDF. **Export** (`annotations_to_xfdf`, a read-only
`ExportAnnotations` actor query that writes the file like `extract_pages` writes
`dest`) walks the raw dicts and emits one element per `/Subtype` with full geometry
+ style (the thin `AnnotationInfo` read model can't round-trip colour/quadpoints/ink,
so this reads dicts directly). **Import** (`import_xfdf`, an undoable `ImportXfdfEdit`)
**reuses the canonical `cos::add_*` writers** rather than rebuilding dicts — an
imported highlight runs the same `/AP`/`/BBox` code as a drawn one — then patches
back the original `/NM`, `/Contents`, `/T`, and dates (finding the new annotation by
object-id set-difference, exact because lopdf preserves ids across load→save).
Preserving `/NM` is what lets reply threads survive: a two-pass import adds
non-replies first, then replies whose parent name now exists (fixed-point for
reply-to-a-reply), re-wiring `/IRT`. The whole import is one undoable edit. The XML
is parsed by a **hand-rolled** subset reader (no parser dependency — see
`docs/03`/CLAUDE.md's dependency stance): lenient separators on input, strict commas
on output, clean failure on malformed input. Commands: `pdf_export_annotations` /
`pdf_import_annotations`; the frontend `src/ipc/interchange.ts` wrappers drive the
`AnnotationPanel` header's ⬆/⬇ actions through the native save/open dialogs. **FDF
(the spec's other half) is deferred to E1b.**

**Flattening (P3.E2, P3-ANN-011)** lives in **`pdf/flatten.rs`** — a COS transform
(not PDFium's native `FPDFPage_Flatten`, which would need a shared live handle +
unsafe FFI, against the cos-on-bytes rule). It *replays* each annotation's existing
`/AP` appearance form into the page: register the form under the page's
`/Resources /XObject` (lopdf `add_xobject`), append a self-contained
`q <BBox→Rect cm> /<name> Do Q` fragment to a new content stream spliced onto
`/Contents` (the resize edit's append-don't-decode pattern), then drop the
annotation from `/Annots`; `prune_objects` GCs the orphaned annot dicts while the
forms survive (the page resources now reference them). The placement matrix is the
appearance algorithm of §12.5.5 — the identity for our own annotations
(`BBox == Rect`, no `/Matrix`). A guard clones inherited `/Resources` down before
`add_xobject` so an empty auto-created dict can't shadow them. It's a `FlattenEdit`
via `cos_edit`, so the inverse is a pre-flatten snapshot: **undoable in-session,
gone once saved + reopened** — exactly the spec's wording. `/AP`-less notes/replies
have no appearance to bake and are **kept live**. Command `pdf_flatten_annotations`;
the `AnnotationPanel` ▦ action is gated behind an inline confirm.

**Form flatten (P5.C2, P5-FORM-010)** reuses that machinery through a predicate:
`flatten.rs` exposes `flatten_annots_where(doc, keep)`, and **`pdf/form_flatten.rs`**
passes "keep everything that isn't a `/Widget`" so page markup survives. The half
that *isn't* reuse is the appearance pass in front of it, and it exists because of
how filling works: the P5.A2/A4 writers set `/V`, **delete the stale `/AP`**, and
flip `/NeedAppearances true` so the viewer regenerates the look — right for an
interactive form, fatal for flatten, since a naive widget-bake would find no
appearance and silently drop every typed value. So `form_flatten` first synthesizes
an `/AP /N` for each valued text/choice widget from `/V` + `/DA` (font size,
colour, and `/DR`-resolved base font parsed out of the `/DA` fragment; size `0`
auto-fits the box), routing through `cos::free_text_appearance` so non-WinAnsi
values take the same embedded-CID branch free text does. Buttons need none of it —
their `/AP /N` is pre-baked per state and picked by `/AS`. After the bake, every
remaining widget (hidden, or appearance-less) is swept from `/Annots` and the
catalog's `/AcroForm` is removed, taking `/XFA` with it. Same in-session-only undo
contract. Command `pdf_flatten_form`, behind the field panel's confirm.

**Form-data interchange (P5.C1/C2)** is a matched pair: `pdf/form_data.rs` walks
the `AcroForm` `/Fields` tree and serialises name/type/value as FDF, XFDF, JSON or
CSV; `pdf/form_import.rs` parses the same four back (FDF via lopdf after swapping
`%FDF-` → `%PDF-`, XFDF hand-rolled like `xfdf.rs`, JSON via serde, CSV RFC-4180)
and matches on the **fully-qualified** name. P5-FORM-009's "reported, not silently
coerced" is why import returns an `ImportReport` rather than a count, and why it
can't be a plain `Edit` (which only hands back an inverse): `import_into` opens the
`form_apply` chassis up so the actor records the snapshot inverse itself and
replies with the report alongside the history state.

**Phase 4 — content editing — adds a third read pattern.** P3's reads went through
the cos byte path (`save_to_bytes` → lopdf); but *text* lives in encoded content
streams that lopdf can't cheaply turn into positioned, styled runs — that's
PDFium's job. So **`pdf/text_extract.rs` (P4.A1)** reads the **live `PdfDocument`**
directly under the shared `PDFium` lock, exactly like `render.rs` renders it (no
serialize round-trip, no `unsafe` — the pdfium-render *high-level* API). It walks a
page's text page-objects (each ≈ one show operator) and emits a `TextRun {text,
bbox, font, size, colour, transform}` over `pdf_extract_text_runs`; the frontend
will hit-test a click to a run for **click-to-edit** (P4.B1). This is the read half
of the text engine — A2 (font fallback) + A3 (redact-and-reflow, the lossy *write*
half) build on it. So the COS layer now has **three** read shapes: lopdf byte-reads
(annotations), a render-to-pixels read, and this live-PDFium structured-text read.

**Font fallback (P4.A2)** is the honesty gate for that write half. `text_extract.rs`
gains `collect_document_fonts` (a lighter sibling of the run walk — distinct
`(name, embedded)` only, same live-`PDFium`-under-lock path), and **`pdf/font_resolver.rs`**
turns those into a `FontReport` over the read-only `pdf_read_font_report` query.
The resolver itself is **pure** — `resolve_font` classifies each font as embedded /
base-14 / system-installed / fallback against an injected `SystemFontIndex`, so it's
fully unit-testable. The *only* side effect is `load_system_fonts`: a one-time,
`OnceLock`-cached std::fs scan of the OS font dirs (offline, no network, **no new
dependency**). "Installed on the system" is a normalized file-stem heuristic — precise
family parsing would need a font crate we don't take — so the bias is deliberately to
**warn when unsure** rather than silently substitute (the roadmap's hard rule). The
frontend raises a once-per-document `FontFallbackBanner` (`use-font-report.ts` keyed
on document id, not edit epoch); its "re-flow" affordance is present-but-disabled
until B1 makes re-flow real.

**Text editing (P4.A3) makes PDFium a *writer* — between byte snapshots.** Until now
every write went through lopdf (the COS path); A3 is the first to use PDFium's own
object-mutation API, because re-emitting glyphs is PDFium's job, not lopdf's.
`pdf/reflow.rs::replace_text_run` edits a run's text in place with `FPDFText_SetText`,
preserving font/size/colour/matrix. Two hard-won rules shape it: (1) **never mutate
the live document** — PDFium content mutation can SIGSEGV at teardown (the same reason
`resize_pages` lives in lopdf), so we mutate a *throwaway* doc loaded from the input
bytes, serialize, and have `ReplaceTextRunEdit` swap the live doc to the result (inverse
= a `RestoreDocEdit` byte snapshot, identical to the COS edits' undo); and (2) **stage
under `Manual` content regeneration and `regenerate_content()` exactly once** — a bare
`set_text` mutates the object handle but doesn't flag the page, so the change is lost on
save otherwise. **The *redact* half (delete) came via lopdf, not PDFium:** `FPDFPage_RemoveObject`
**SIGSEGVs in our bundled PDFium**, so `delete_text_run` (P4.B3) removes a run at the COS
level — decode the page content with `get_and_decode_page_content`, splice out the run's
`Tj`/`TJ` operator, `change_page_content` to write it back. The trap is that A1's `run_index`
counts PDFium *text objects* while lopdf counts *show operators*; they align on normal pages,
but to never mis-delete we **verify by re-extraction** (post-delete runs == pre-delete minus
the target, else error) — which doubles as P6-SEC-010(c)'s "confirm the text is gone." `'`/`"`
and `XObject`-embedded text are rejected. (Recreating a run in a substitute font — the other
removal use — is still deferred; see `BACKLOG.md`.) So PDFium appears in the write path for
edits and lopdf for deletes, but both only ever between byte snapshots — the document of
record is still the bytes.

**Adding text (P4.B2)** is also lopdf, and reuses the free-text drawing. `cos::add_text_box`
registers a base-14 font on the page under a collision-free `Fvibe…` name (cloning a shared
or inherited `/Resources` so it never mutates another page's), then appends the *same*
`q BT … Tj … ET … Q` fragment free-text emits for its `/AP` — but into the **page content
stream** rather than an annotation appearance (the spec's "content stream, not an
annotation"). The payoff of real content: the added text is immediately **editable by B1 and
deletable by B3** — no bespoke edit path. `TextBoxEdit` wraps it in the usual `cos_edit`
(snapshot + swap + `RestoreDocEdit` inverse), so undo/redo come for free.

**Adding an image (P4.C1)** is the same recipe with an Image `XObject` instead of a font, and
opens **Track C**. `cos::add_image` embeds via `image_xobject::embed_image` (PNG → raw samples +
`/SMask` reusing P3.C3b's `embed_png`; JPEG → embedded verbatim as a `/DCTDecode` stream, dims
read from the `SOF` header — no pixel decode), registers it under a collision-free `/XObject`
name, and appends `q <cm> /Img Do Q` to the page content. The Resource-registration and
content-append helpers (`register_page_resource`, `append_page_content`) were **generalized out
of B2** and are now shared by add-text and add-image. Only PNG/JPEG are accepted — the other
formats the spec lists need a raster decoder we don't bundle (clean error; BACKLOG). Like the
added text, the image is real content, so **C2** (edit/move/resize/rotate/replace/delete) will
operate on it uniformly.

**Editing an image (P4.C2)** finally exercises a PDFium *content mutation* A3 left uncertain.
`image_extract.rs` locates images (A1-style live-PDFium read → `ImageInfo {index, bbox,
matrix}`). Move/resize/rotate are all *one* new placement matrix the frontend computes, applied
by `image_edit::transform_image` via PDFium **`reset_matrix`** — and the key finding is that
`reset_matrix` is a *mutate-in-place* FFI like `FPDFText_SetText` (it **works**), unlike
`FPDFPage_RemoveObject` (which crashes). So transform follows the throwaway-doc + `Manual`-regen
pattern; **delete** still goes through a lopdf `Do`-splice (`delete_image`, B3-style, verified by
re-extraction). C2 also flushed out a latent bug in C1/B2's `append_page_content`: appended
content streams lacked a leading separator, so a page ending `…ET` fused with the appended `q`
into `ETq` when lopdf re-decoded the array (PDFium had hidden it by inserting the spec-required
whitespace) — corrupting multi-image delete. Fixed by prepending `\n`. **Replace** (P4.C2b,
`replace_image`) embeds the new image and overwrites the `XObject` the selected image references
*in place* — the resource name, `cm`, and `Do` are untouched, so only the pixels change and no
`/Resources` edit (and no copy-on-write) is needed.

**Hyperlinks (P4.C3)** close Track C and reuse the *annotation* chassis rather than the
content-stream one: `cos::add_link` builds a `/Link` annotation dict (the same `add_*` +
`append_annotation` shape as the sticky note), and `annotation::AddLinkEdit` runs it through
`cos_edit` (snapshot → transform → reload; inverse `RestoreDocEdit`). The target is one of
four shapes: a URL or `mailto:` email → `/A << /S /URI >>`; an internal page → `/Dest [pageRef
/Fit]` (the **array-with-page-ref** form, so the existing `dest_target_page` /
`prune_dangling_destinations` reorder-and-delete fixups apply unchanged); a named destination →
`/Dest (name)`. Links are invisible hot-zones — `/Border [0 0 0]`, no `/AP`. The `(value)`
string is escaped by lopdf's `Object::string_literal`, so parens/backslashes in a URL can't
break the file. The frontend `LinkLayer` drags a rect, then a popover collects the target;
`tools/link/target.ts` (pure) validates it and converts the user's 1-based page number to the
0-based index the command takes. The primitive deliberately lives in `cos.rs` (every other
annotation `add_*` does), **not** a separate `link.rs`. **Appearance (P4-EDIT-007b)** threads a
`style` (`box` default / `underline` / `invisible`) + `color` through the same chain:
`apply_link_appearance` either leaves a borderless hot-zone (invisible — byte-identical to C3) or
attaches a generated `/AP` Form XObject (BBox == Rect, identity matrix, a 1pt stroke — the same
scaffold as the markup `/AP`) plus `/C` + `/BS` for readers that ignore `/AP`. Drawing a real
appearance — not just `/Border` — is what makes the box/underline render in every reader (a
borderless link only shows the reader's own hover affordance).

**Watermark (P4.D2)** opens **Track D — page decoration**, and is the first feature with its own
`pdf/watermark.rs` module rather than living in `cos.rs`: Track D's later features (background,
header/footer, page numbers, Bates) all share the same "draw content onto selected pages with
placement / opacity" machinery, so it earns a home. It reuses cos's content-writer helpers — now
`pub(crate)`: `register_page_resource` (copy-on-write `/Resources`), `append_page_content` and a new
`prepend_page_content`, `base_font`, `font_avg_em`, `parse_hex_color`. A watermark is **page
content**, not an annotation: per selected page it registers an opacity `/ExtGState` and a font (or
the shared image `XObject`, embedded **once**), then writes a `q … Q` fragment that rotates about the
page centre (a `cm` matrix) and draws the centred text/image. *On top* appends the stream; *behind*
prepends it (so existing content paints over the mark). `WatermarkEdit` is self-contained like
`image_edit` (snapshot → reload, inverse `RestoreDocEdit`), not routed through `annotation.rs`'s
`cos_edit`. The frontend is a **document-wide dialog** (`WatermarkDialog`, like Split/Merge), not a
canvas layer; `tools/watermark/parsePageRange` turns "all" / "1-3,5" into 0-based indices. 50 pages
in ~0.1 s — it's pure lopdf object-adds + one save, no rasterization.

**Background (P4.D1a)** is the watermark's simpler sibling and the first reuse of Track D's shared
machinery: always full-page, always **behind** (one `prepend_page_content`). Colour paints a filled
`MediaBox` rect; an image is embedded once and drawn **cover-fit with a clip** (`re W n` then a
cover-scaled `cm` — fills the page, crops overflow, no distortion). To share with watermark, this
ship promoted `page_media_box` from `watermark.rs` into `cos.rs` (`pub(crate)`), and moved the
frontend `parsePageRange` into a shared `tools/page-range.ts` (both decoration dialogs use it).

**A page from another PDF as a background (P4.D1b)** is the one genuinely new capability: a page
can't be referenced directly, so `import_page_as_form` converts it into a **Form XObject**. It loads
the source, `renumber_objects_with(dest.max_id + 1)` (the `cos::merge_documents` move) so the
source's ids can't collide, takes the chosen page's `MediaBox` + effective `/Resources` (walking the
`/Parent` chain) + decoded content, then copies **only the transitive object closure of those
resources** into the dest (a BFS over references — *not* the whole source doc, so the file doesn't
bloat) and wraps the content in a `/Form` XObject (`BBox` = source `MediaBox`). Each target page
references that one Form, drawn **contain-fit** + centred so the whole source page stays visible.
Limitation: the source page's `/Rotate` is ignored (Form XObjects don't carry page rotation).

**Header / footer (P4.D3)** is watermark's text path, positioned in the margin: `header_footer.rs`
draws left/centre/right text as **appended** page content (it overlays). Per page, each non-empty
position's template has its `{n}` (absolute 1-based page) / `{total}` / `{date}` placeholders
substituted (pure `substitute`, unit-tested), then a `BT … Tf x y Td (…) Tj ET` at the aligned
`x` (`font_avg_em` width estimate) and the header/footer baseline `y`. The `{date}` **value** comes
from the frontend (its locale-formatted today) so Rust needs no date dependency — offline-first for
free. This ship promoted `escape_pdf_string` to `cos.rs` `pub(crate)` (both text writers use it),
mirroring `page_media_box` in D1a. Start-offset / roman / alpha numbering is **D4**, not D3.

**Hardening (P4.HF)** made every Track-D writer rotation- and crop-aware: writers lay content out in
**visual space** — the displayed `CropBox` (`cos::page_effective_box`, CropBox∩MediaBox) after the
inheritable `/Rotate` (`cos::page_rotation`) — and prepend the compensating `cm` from
`cos::visual_transform`, so a footer lands at the *visible* bottom of a `/Rotate 90` page and a
watermark centres on what the user actually sees. (The background colour fill deliberately still
covers the full MediaBox — bleed-safe.) Two companion fixes: `/Contents` held as an indirect
reference **to an array** is now dereferenced and flattened by the content appenders
(`existing_contents`), and `save_document` threads the document's open password into the round-trip
verification — before this, **encrypted documents could not be saved at all** (PDFium preserves
encryption on save; the verify re-opened the temp file with no password and failed). Save now
succeeds and the copy stays encrypted, pinned by `tests/hardening.rs`.

**Decoration identity (P4.HF2).** Every Track-D fragment is wrapped in a marked-content block —
`/VibePDF << /Kind (watermark|background|header-footer) /Id (uuid) >> BDC … EMC`
(`cos::wrap_decoration`) — the content-stream analogue of `/NM` on annotations. Marked content is
semantically inert to renderers, but it makes a future "remove / re-stamp this decoration" a
mechanical operator splice (find the tagged `BDC…EMC` range, drain, re-encode — the same machinery
`delete_image` uses), proven end-to-end by `hardening.rs::decoration_tag_is_operator_spliceable`.
Two caveats: PDFium **compresses** content streams on save, so the tag is found at the operator
layer (`get_and_decode_page_content`), not by raw byte search; and PDFium's `Manual` content
regeneration (the text-edit path) may drop tags on a page it regenerates. D4/D5 inherit tagging
through the shared writers and must pass their own `/Kind`.

**Text correctness + error surface (P4.HF3).** The built-in base-14 fonts render only the
`WinAnsiEncoding` (CP1252) range, and previously any text outside ASCII silently corrupted — even
Latin-1 the fonts *can* draw. Three coordinated changes in `cos.rs` fix it: `base14_font_dict`
(the one builder for every text writer's font) sets `/Encoding /WinAnsiEncoding`;
`escape_pdf_string` transcodes Latin-1 / CP1252 characters to octal escapes of their WinAnsi byte
(`é` → `\351`), which those fonts then render correctly; and `ensure_winansi` gates all seven
rendered-text entries (watermark, header/footer, text box, free-text add/update, stamp,
image-stamp), returning a typed `InvalidInput` that names up to three offending characters when
the text needs glyphs the built-in fonts lack. Silent mojibake becomes a loud, honest rejection;
true-Unicode support waits on font embedding (`FABLE_REVIEW` 3.2 stage 2). On the frontend, that
rejection — and every other failed canvas-tool write — now lands in a **toast**
(`state/toast-store.ts` → `app/Toasts.tsx`, pushed by `app/report-error.ts` which maps
`CommandError.code` to friendly copy), the missing last hop of the typed-error chain; the ~21
`console.warn`-only catches on user actions were replaced (passive read/sync failures still just
log).

**Untrusted-input hardening (P4.HF4).** D1b's `import_page_as_form` copies a page's resource
closure out of a **user-picked source PDF** — the one place a Track-D writer parses bytes VibePDF
didn't produce. `background.rs::collect_refs`, which computes that closure, previously recursed on
the object graph, so a crafted deep reference chain could overflow the actor thread's stack. It is
now two explicit worklists — `pending` (object ids, driving the reference chain) and a per-object
`inline` stack (nested arrays/dicts) — so neither reference depth nor inline nesting can grow the
call stack; the `acc` set still bounds each id to one visit. Behaviour on valid input is
unchanged. Testing surfaced a lopdf subtlety worth recording: `get_object` transparently collapses
bare `M 0 R` indirection, so the genuine overflow shape is a chain of *containers*
(`<< /Next n+1 0 R >>`), which the regression test uses.

**Font embedding (P4.HF5, FABLE_REVIEW 3.2 stage-2).** True-Unicode text arrives through a second
text-writing backend, chosen at the last moment: `cos::ensure_winansi` (the HF3 hard gate) split
into a predicate `cos::winansi_fits`, and each rendered-text writer can now branch. WinAnsi-safe
text keeps the existing base-14 lopdf path — small, unchanged, byte-for-byte identical — while text
outside WinAnsi routes to the **hand-built CID** backend (`pdf/font_embed_cid.rs`). Font bytes come
from `font_resolver::covering_font_bytes` (best-effort broad system face; per-glyph coverage
checking is deferred).

**One embedding backend — hand-built CID (P4.HF9 for `/AP`; unified P4.HF18–20).** Rather than take
the font-parsing dependency `docs/03` deliberately avoids, `build_cid_font` builds a `Type0` /
`CIDFontType2` font **by hand in lopdf** — subset via `subsetter` (glyph-ids preserved), real
advances + descriptor metrics via `ttf-parser`, emitting `/FontFile2` + `/FontDescriptor` + `/W` +
a `/ToUnicode` CMap, with Identity-H encoding; text is shown as `<gid…> Tj` hex strings. The same
backend serves both surfaces:

- **Annotation `/AP`** (free-text, stamps): the `Type0` dict goes in the appearance's `/Resources`.
  `cos::free_text_appearance` branches on `winansi_fits`; the plain Unicode stays in the
  annotation's `/Contents` so re-edit reads it, not the appearance.
- **Page content** (header/footer, watermark, text box): `font_embed_cid::place_cid_run` registers
  the font on the page and emits a marked-content-wrapped run — `/VibePDF … BDC … EMC` (HF2 tag, so
  it is operator-removable), a `cm` matrix (the same `visual_transform` the base-14 path uses for
  rotation/CropBox), optional `/ExtGState` opacity, prepend-for-`behind`, and an underline path.
  Widths come from `cid.width` (exact — the §3.10 embed-path fix).

*History:* the page writers originally embedded through a **PDFium** round-trip (`font_embed.rs`,
`load_true_type_from_bytes` → PDFium text objects). It couldn't reach an `/AP` stream, drifted on
alignment (flat-average widths), and carried no HF2 tag — so the CID-path unification (HF18–20)
retired it and **deleted `font_embed.rs`**. There is now a single Unicode backend.

**Click-to-edit (P4.B1)** is the consumer that finally surfaces the whole text engine.
The `ReplaceTextRun` actor message applies A3's `ReplaceTextRunEdit` (record inverse,
mark dirty, return `HistoryState`), exposed as `pdf_replace_text_run`. The frontend
`TextEditLayer` (a per-page overlay beside `FreeTextLayer`) fetches the page's runs via
A1's `extractTextRuns`, lays a hit-zone over each, and on click opens an inline editor at
the run's bbox; commit calls `replaceTextRun` then bumps the edit epoch so the canvas
re-renders. No new write *mechanism* — B1 is pure wiring over A1 (read), A2 (the fallback
banner + an inline per-edit cue), and A3 (the `set_text` write). The `run_index` the layer
hands back is the same ordinal A1 emits, so read and write agree on run identity.

**Deleting annotations (P3-ANN-012)** turned on one thing: a stable identity.
Every annotation our writers create now carries a `/NM` (a uuid; `cos` stamps it
server-side for markup/free-text/shapes, the frontend already did for notes), so
`read_annotations` returns it as the delete handle and the existing
`cos::delete_annotation` removes any annotation by it (a missing-`/NM` foreign
annotation falls back to an `obj:<num> <gen>` object-id handle). The sidebar's ✕
/ Delete-key calls `pdf_delete_annotation` → `bumpEpoch`, and the whole projection
re-syncs: the canvas reload drops the `/AP`, the list re-reads, and a note's
overlay icon clears via `useNotesSync`. No new IPC and no new overlay — the
identity was the only missing piece.

**Editing a free-text box in place (P3-ANN-013)** is the same `/NM`-keyed update:
`cos::read_free_text` parses the box's text + style back out (`/DA` for size +
colour, the `/AP` `/BaseFont` for family/bold/italic — the inverse of the write),
and `cos::update_free_text` rewrites `/Contents` + `/Rect` + `/DA` + `/AP` in place
while keeping the `/NM` (the old `/AP` stream is GC'd). The sidebar's ✎ posts an
*edit request* through `annotation-edit-store` (a one-shot mailbox between the
distant sidebar and the per-page `FreeTextLayer`), which reuses its create-mode
editor — pre-filled and with the toolbar set to the box's style — and commits via
`updateFreeText`. Shape style edit is the same pattern, deferred.

Because the note overlay is *not* the source of truth, it's kept a **projection
of the PDF** (P3.B2b): `cos::read_text_notes` (a read-only `ReadNotes` actor
query, serialize-then-lopdf-parse like `GetBytes`) feeds `pdf_read_text_notes`,
and the frontend `useNotesSync` hook re-reads + `replaceNotes` whenever the
document id or its **edit epoch** changes. That single reactive seam makes saved
notes re-openable in-app *and* keeps undo/redo honest (an actor-level undo bumps
the epoch, so the overlay re-syncs rather than stranding a ghost icon). Placement
deliberately doesn't bump the epoch, so the optimistic icon isn't clobbered before
its write lands.

**The two libraries never share a live handle.** A structural edit is a pass
over **serialized bytes** between PDFium passes:

```
PDFium op ──save_to_bytes──▶ bytes ──lopdf load+rewrite+serialize──▶ bytes'
                                                                       │
                          verify_pdf_reopens(bytes')  ◀── PDFium ──────┘
                                    │ (must succeed before persisting)
                                    ▼
                              save / reload
```

Because `lopdf` is pure Rust it acquires **no `PDFIUM_LOCK`** — there's no
shared global to race. The one hard rule: **every `cos` output is
round-trip-verified by reopening it in PDFium before it's written to the
user's file** (the project's "no silent breakage" constraint). `cos.rs`'s
spike tests assert exactly this. `cos` functions are pure `&[u8] → Vec<u8>`
transforms, so they compose cleanly into the existing `save_document`
pipeline.

`save_document` itself runs one such pass — `cos::prune_dangling_destinations`
removes references to pages a delete/split removed (broken links, bookmarks),
so every file written to disk has clean internal references (P2-PAGE-003). It's
a no-op (returns the input unchanged) when nothing dangles, and infallible, so
it never breaks a save.

---

## The command boundary

The IPC surface is the most important interface in the project. **All write operations to a PDF go through it.** Frontend code never touches PDF bytes.

### Naming and shape

All commands live in `src-tauri/src/commands/` and follow this shape:

```rust
#[tauri::command]
pub async fn pdf_redact_region(
    state: tauri::State<'_, AppState>,
    document_id: DocumentId,
    page: usize,
    region: Rect,
) -> Result<RedactionResult, CommandError> {
    let actor = state.actors.get(&document_id)?;
    actor.send(Message::RedactRegion { page, region }).await
}
```

Command names use `<domain>_<verb>_<noun>`: `pdf_open`, `page_rotate`, `annotation_add`, `form_fill_field`, `signature_apply`, `ocr_run`.

### Typed wrapper on the frontend

For every command, there's a TypeScript wrapper in `src/ipc/`:

```ts
// src/ipc/redact.ts
export async function redactRegion(
  documentId: DocumentId,
  page: number,
  region: Rect,
): Promise<RedactionResult> {
  return invoke<RedactionResult>('pdf_redact_region', { documentId, page, region });
}
```

Types are shared via a code-generation step (`ts-rs` on the Rust side). The frontend never invents Tauri command shapes by hand.

### Error model

Every command returns `Result<T, CommandError>`. `CommandError` is a typed enum: `NotFound`, `InvalidInput`, `PdfError`, `IoError`, `PermissionDenied`, `Internal`. The frontend maps each variant to a specific UI affordance (toast, modal, retry).

We do NOT throw strings across IPC. Strings lose information and break i18n.

### Stateless multi-file operations

Most commands take a `DocumentId` and route to that document's actor (the
actor owns the bytes and serializes mutations). A few operations don't fit
that shape: they read **one or more files from disk that need not be open**
and produce a **new** file, mutating nothing. **Merge** (`pdf_merge_documents`,
P2-PAGE-008) is the first.

These run as **standalone commands** that take file paths instead of a
`DocumentId`. They still obey the two invariants that matter:

- **The frontend never writes bytes** — the Rust side does all I/O through the
  verified `pdf::document::save_document` (atomic temp+rename + round-trip
  reopen), exactly like an actor edit.
- **All PDFium FFI is serialized** — they acquire the process-global
  `PDFIUM_LOCK` for their build span, so they can't race an actor thread.

Because the work is blocking PDFium FFI (opening N documents, importing pages,
serializing), the command body runs inside `tokio::task::spawn_blocking` so it
never stalls the async runtime. No actor is spawned; no `document-changed`
event is emitted (no open document changed).

---

## State on the frontend

Several zustand stores, each owning a clearly scoped slice. Stores never reach
into other stores. The authoritative state for the PDF itself is the Rust
document actor — every frontend store caches a *derived view* or a *preview*.

| Store (`src/state/`) | Owns | Source of truth? |
|---|---|---|
| `useDocumentStore` (`document-store`) | Open documents, current document id | Yes (synced from backend events) |
| `useViewStore` (`view-store`) | Zoom, fit mode, page index, sidebar visibility | Yes |
| `useSettingsStore` (`settings-store`) | App-wide settings | Mirrored to Rust on change |
| `edit-epoch-store` | Per-doc edit epoch — bumped on each edit/undo/redo to drive the PDF.js reload | Derived |
| `history-store` | Per-doc `{canUndo, canRedo}` mirror for button state | Derived (actor owns the stack) |
| `rotation-preview-store` | Cosmetic per-page rotation (the rotate fast-path) | **Preview** (PDFium holds real `/Rotate`) |
| `search-store` | Search query + matches | Derived |
| `view-persistence` | Per-doc last zoom/page (IndexedDB) | Local cache |
| `tool-store` (P3.A1) | Active annotation tool + style options | Yes |
| `annotation-store` (P3.A1) | In-progress draft + committed annotations, per doc | **Preview** — see below |

**Annotation store is a draft/preview layer, not a second source of truth.** Like
`rotation-preview-store`, it stages what the render layer (P3.A2) draws and what
tools build, but it does **not** own the PDF. Authoritative annotation
persistence + undo land in **P3.B1** via the actor (`pdf/annotation.rs` + an
`Edit`); keeping the two in lockstep is B1's job. A1 ships the staging area + the
tool framework so tools and the render layer can be built and tested first.

The **annotation tool framework** (`src/tools/_framework/`, P3.A1) realizes the
`§Edit tools` contract above: a pure lifecycle state machine drives stateless
per-tool reducers over pointer gestures (pointer events, per `§WebView quirks` —
no HTML5 DnD), with screen↔PDF coordinate mapping isolated in `coords.ts`. The
**render layer** (`src/view/annotation-layer.tsx`, P3.A2) is the per-page SVG
overlay that draws the store's annotations and binds those pointer gestures; it
mounts inside `PageVirtualizer`'s `PageSlot` as a sibling of the
imperatively-managed canvas (the canvas gets its own inner div to clear, so the
React overlay survives — and the outer flow element stays the scroll anchor).
Between them sits the **text layer** (`src/view/text-layer.tsx`, P3.B1a) — PDF.js's
`TextLayer`, transparent selectable spans that make page text selectable. Text
markup (highlight/underline/…) is **selection-driven** (read `getSelection()` →
`/QuadPoints`), so it bypasses the pointer lifecycle. On top of the SVG overlay
sits the **note layer** (`src/view/note-layer.tsx`, P3.B2a) — an HTML overlay for
sticky notes, which (carrying no `/AP`) the canvas can't paint; it draws the note
icons + popup from the store. Per-page stacking, bottom to top: **canvas → text
layer → annotation (SVG) overlay → note (HTML) overlay** (both overlays are
click-through when idle, so selection reaches the text layer beneath; only the
note icons/popup and a live tool capture pointer events).

**Backend events** (pages changed, form fields modified, etc.) are pushed via
Tauri's event system; the frontend listens and updates the relevant store. The
backend is always the source of truth for the PDF itself; the frontend stores
cache *derived views* or *previews*.

---

## WebView quirks

The frontend runs in the OS-native webview (Tauri 2), **not** Chromium — so it
is not a Chrome-equivalent. Behaviour differs per platform, and on macOS the
webview is **WKWebView**.

- **No HTML5 drag-and-drop for in-page reordering.** WKWebView fires
  `dragstart`/`dragend` on a `draggable` element but **never delivers the
  drop-target events** (`dragenter`/`dragover`/`drop`), so a native HTML5 DnD
  reorder can never complete. Implement element-to-element drag with **pointer
  events** (`pointerdown`/`pointermove`/`pointerup`) instead: arm on
  pointerdown, begin past a small movement threshold (so a plain click still
  fires), `setPointerCapture` once dragging, and resolve the drop target with
  `document.elementFromPoint` + a `data-*` marker (capture redirects events, not
  hit-testing). The thumbnail reorder (`panels/ThumbnailPanel.tsx`, SPEC
  P2-PAGE-002) is the reference implementation. **Do not reach for HTML5 DnD for
  any future in-app drag UI** (page reorder, tab reorder, annotation drag).

- **The PDF.js text layer needs three WKWebView fixes** (P3.B1a — making page
  text selectable, which markup acts on). All three were silent until exercised:
  1. **`ReadableStream` has no async iterator.** PDF.js v5 `getTextContent` does
     `for await (… of streamTextContent())`; WKWebView/Safari doesn't implement
     `ReadableStream[Symbol.asyncIterator]`, so text extraction throws "undefined
     is not a function". Polyfilled in `src/polyfills.ts` (first import in
     `main.tsx`).
  2. **CSS `round()` may be unsupported.** v5 sizes the text layer with
     `round(down, var(--total-scale-factor) * Npx, …)`; `text-layer.tsx` pins an
     explicit px size after `render()` so the layer overlays the canvas exactly.
  3. **Port the *full* v5 `.textLayer` CSS**, not a minimal subset — the per-span
     `font-size`/`scaleX` rules (driven by `--font-height`/`--scale-x` that PDF.js
     sets inline) are what give spans their size; without them a click selects the
     whole page. In `styles/globals.css`.
  Plus: `getDocument` must set `standardFontDataUrl`/`cMapUrl` (served from
  `/pdfjs/`, copied by `scripts/copy-pdfjs-worker.mjs`) so non-embedded standard
  fonts extract correctly.

## Content-Security-Policy

The webview renders **hostile input** (arbitrary PDFs via PDF.js) while holding
IPC access, so it runs under a strict CSP set in `src-tauri/tauri.conf.json`
(`app.security.csp`) — not the Tauri default of `null`. The policy is
`default-src 'self'` with the minimum relaxations the frontend actually needs,
each earned by a specific resource:

- `script-src 'self' 'wasm-unsafe-eval'` — our bundle, plus PDF.js v5's WASM
  decoders (OpenJPEG/JBIG2 images, QuickJS PDF-function eval). `'wasm-unsafe-eval'`
  is strictly narrower than `'unsafe-eval'` (WASM compile only, no JS `eval`).
- `worker-src 'self'` — the PDF.js worker is a same-origin static asset
  (`/pdfjs/pdf.worker.min.mjs`), not a blob worker.
- `connect-src 'self' ipc: http://ipc.localhost` — same-origin cmap/font fetches
  plus Tauri IPC.
- `img-src 'self' blob: data:` — thumbnail `<img>`s use `blob:` object URLs.
- `style-src 'self' 'unsafe-inline'` — Tailwind + React inline `style={}` attrs.
- `object-src 'none'; base-uri 'self'; frame-src 'none'` — hardening.

A separate `devCsp` adds only what Vite's dev server needs (`'unsafe-inline'`
for the HMR preamble, `ws://localhost:*` / `http://localhost:*` for the HMR
socket); production stays locked down. CSP is enforced by the webview at
**runtime**, so a wrong policy blanks the app and no unit test catches it — the
config is regression-guarded by `src/__tests__/csp.test.ts`, but any change must
be re-smoke-tested in the running app (dev **and** a bundled build). No
`dangerouslySetInnerHTML` exists in the frontend (React escapes rendered
metadata by default). See FABLE_REVIEW §3.8 (P4.HF14).

## The render pipeline

PDF.js handles rendering in the WebView for the interactive canvas. PDFium handles rendering on the Rust side for: thumbnails, export-to-image, OCR input, visual diff. We accept rendering them twice. Performance benchmarking has shown this is fine; the two engines are both fast.

```
┌──────────────────┐
│  Open file       │
└────────┬─────────┘
         │
         ├─► Rust:  PDFium loads, document actor spawned
         │
         ├─► Frontend: PDF.js loads same bytes (read-only view)
         │
         └─► On user save:  PDFium writes; PDF.js re-loads the new file
```

**Important:** PDF.js never writes. It is purely the view layer. This is non-negotiable; sharing write capability between two PDF engines causes drift.

**Edit-preview pipeline.** Edits mutate the actor's in-memory PDFium document, but PDF.js renders from a separate copy — so an edit wouldn't show until reload. The bridge is a per-document **edit epoch** (`state/edit-epoch-store.ts`): every successful edit / undo / redo bumps it. The main view (`PdfViewer`) and the thumbnails subscribe; on a bump the view reloads PDF.js from the actor's *live* bytes via `pdf_get_bytes` (a read-only `save_to_bytes`), and the thumbnails invalidate + re-render. The reload swaps the new document in before destroying the old (no blank) and restores the current page. This is a full re-parse per edit for now — correct and uniform across all edit types; an incremental/single-page fast path is deferred (BACKLOG). It keeps the "PDF.js never writes" rule intact: the actor still owns every byte; the frontend only *reads* the current state.

---

## Edit tools

Each interactive tool (select, text-edit, freehand, shapes, redact, sign) follows the same pattern:

```
src/tools/<tool>/
├── index.ts          # registration
├── state.ts          # tool-specific state machine
├── overlay.tsx       # the SVG/canvas overlay shown while the tool is active
├── intent.ts         # turns gestures into IPC calls
└── README.md         # state diagram, IPC surface used
```

Tools are independent. They subscribe to `useToolStore` to know if they're active, listen for pointer events, and dispatch intents through `src/ipc/`. No tool reaches into another tool's state.

---

## Saving and auto-save

Two save modes:

1. **Explicit save** (Cmd/Ctrl+S): writes the current PDFium document to disk. Backs up the previous version as `<name>.bak` for one save cycle.
2. **Auto-save**: every 30 seconds, if the document is dirty, write a copy under Tauri's `app_data_dir()` at `autosave/<documentId>.pdf`, plus a `<documentId>.json` sidecar recording the original path + timestamp. On startup, scan this directory and offer recovery for any open-at-crash documents.

Auto-save is invisible to the user — it never touches the user's original file. The tick is a dedicated std thread (so no `tokio` `time` feature is needed) that pokes each document actor; a *dirty* actor (dirtiness derived from the undo history's state id — see "Undo/redo" below) writes its copy (atomic temp+rename) and drops it again on **any** successful save (same-path *or* save-as) or a graceful close. A real crash never runs that cleanup, so the copy survives to be offered next launch (`recovery_list` / `recovery_discard`). Implemented in `pdf/autosave.rs`; the actor owns the write because PDFium is single-threaded per document.

**Path policy.** All persistent paths are derived from Tauri's `AppHandle::path()` helpers on every platform — there are **no hardcoded POSIX paths**:

| Concept | API | Typical resolved location |
|---|---|---|
| App config (settings) | `app.path().app_config_dir()` | macOS: `~/Library/Application Support/dev.vibepdf/` · Linux: `~/.config/dev.vibepdf/` · Windows: `%APPDATA%\dev.vibepdf\` |
| App data (autosave, recents, license cache) | `app.path().app_data_dir()` | Same roots, with `data/` substructure on Linux |
| Logs | `app.path().app_log_dir()` | macOS: `~/Library/Logs/dev.vibepdf/` · Linux: `~/.local/share/dev.vibepdf/logs/` · Windows: `%LOCALAPPDATA%\dev.vibepdf\logs\` |
| Crash dumps | `app_data_dir()/crashes/` | Same roots |

---

## Undo/redo (session history)

Each document carries its own undo/redo history, living in the **document
actor's worker state** (alongside the dirty flag) — never on the frontend.
PDFium is single-threaded per document, so the edits that undo/redo apply
run on the one thread that owns the `PdfDocument`.

The mechanism is a **command pattern**: every mutating operation
(`rotate`, `delete`, `insert`, …) is an `Edit<T>` whose `apply` performs
the change *and returns the inverse edit*. Undo pops the undo stack,
applies the inverse, and pushes the result onto the redo stack; redo is
the mirror. A new edit clears the redo stack (history forks). The stack is
generic over the target `T` so its invariants are unit-testable without a
live document; the actor instantiates `UndoStack<PdfDocument>`. History is
bounded two ways: by **count** (`MAX_UNDO_DEPTH`) and by **total heap**
(`MAX_UNDO_BYTES`, 256 MiB). The byte budget matters because nearly every
edit's inverse is a full-document byte snapshot (`restore::RestoreDocEdit`),
so a count cap alone is not a memory bound on big scans; each `Edit` reports
its `heap_bytes()` and the oldest undo entries are evicted once the running
total exceeds the budget (one entry is always kept, so the most recent edit
stays undoable). This keeps the history within NFR-PERF-002/003. See
`pdf/undo.rs` and FABLE_REVIEW §3.6.

Granularity is **page-level**: move, insert, delete, rotate, crop, resize
are each one undoable action. The frontend mirrors only `{canUndo,
canRedo}` (per document, in `history-store.ts`) to drive button state; it
learns the new availability from each `pdf_undo`/`pdf_redo` (and, later,
each mutating command's) return value. This is "session history" — it is
**not** persisted across restarts and is independent of what is saved to
disk (cf. flatten in `docs/02`, line 126).

**The dirty flag is derived from this history, not tracked separately.**
`UndoStack` mints a unique, monotonically-increasing id for every state
(0 = pristine/as-opened); `current_state_id()` returns the live one. The
actor records that id at each successful save (`saved_state_id`) and treats
the document as dirty whenever `current_state_id() != saved_state_id`.
Because ids are never reused, this is correct where a bare bool or depth
counter is not: undoing back to the saved state reports clean, a **save-as**
(any path) clears dirty, a new edit that forks history after an undo is
*not* mistaken for the branch it replaced, and a depth-cap eviction leaves
the un-undoable floor dirty rather than falsely pristine. See FABLE_REVIEW
§3.11 (P4.HF12).

---

## Settings storage

Settings live in two places:

- **App-wide** (theme, language, AI backend, telemetry preference): `<app_config_dir>/settings.json`.
- **Per-document** (last zoom, last page, sidebar state): keyed by the SHA-256 hash of the document path, stored in IndexedDB on the frontend.

The Rust side owns app-wide settings; frontend reads them via a one-shot command on startup. Changes propagate via events.

### The signature library — and why it is *not* in `security/` (P6.A1)

`<app_data_dir>/signatures/` holds `index.json` (versioned metadata: id, kind,
created-at) plus one `<id>.png` blob per entry. It is the store `P6-SEC-001`
calls "the local signature library", written by P6.A2–A4.

The module tree reserves `security/` for "Crypto, signatures, redaction", and
`steps/P6.md` puts every change under that directory behind a per-change human
review, because crypto bugs fail silently. **The signature library is none of
that**: PNG bytes the user drew, an id, a timestamp. No keys, no certificates,
no signing. It is structurally `recents.rs` — versioned JSON index in
`app_data_dir`, atomic write, defensive read — and reuses the `read_json` /
`write_atomic` helpers `settings/mod.rs` exists to provide.

Filing it under `security/` would put a picture store behind the crypto gate
without making anything safer, and would dilute that gate where it does matter:
the PKCS#7 work in P6.B1. The rule stays narrow so it stays meaningful. What
*will* live in `security/`: `sign.rs`, `encrypt.rs`, `redact.rs` — none of which
exist yet.

Two shape decisions worth keeping:

- **Blobs sit beside the index, not base64 inside it.** The index stays small,
  and one corrupt blob fails only its own entry rather than the whole library.
- **Write blob first, then index.** A crash between the two leaves an orphaned
  file — invisible and harmless. The reverse would leave an index row pointing
  at a file that does not exist.

A signature image is personal data, stored unencrypted, exactly as `recents.json`
stores file paths. That is a deliberate accepted risk: encrypting it would need
a key with nowhere to live in an offline, account-less app.

---

## Logging

- Frontend: `console.*` in dev, no-op in prod (no remote logging).
- Rust: `tracing` crate, JSON output, written to `<app_log_dir>/<date>.log`, rotated weekly, max 30 days.
- Crash dumps: written to `<app_data_dir>/crashes/`, displayed on next startup with an "open folder" button. **Never uploaded.**

---

## Threading model

- **Main thread (Rust):** Tauri event loop, IPC dispatch. Never blocks.
- **Per-document thread (Rust):** Document actor (one per open PDF).
- **Tokio runtime (Rust):** For OCR, AI calls, file IO. Bounded thread pool.
- **Main thread (Frontend):** React render loop, user input. Never blocks on PDF parsing — that goes through a Web Worker.
- **Web Worker (Frontend):** PDF.js parsing.

Anything that takes > 50ms goes off the main thread on its side of the IPC boundary.

---

## A worked example: applying a redaction

Read this once. It anchors the rest of the architecture.

1. User selects the "Redact" tool. Frontend: `useToolStore.setActive('redact')`.
2. User drags a rectangle on page 3. The tool's `overlay.tsx` shows the rectangle live.
3. User releases the mouse. The tool's `intent.ts` dispatches `redactRegion(docId, 3, rect)`.
4. `src/ipc/redact.ts` calls `invoke('pdf_redact_region', ...)`.
5. Tauri routes to `src-tauri/src/commands/redact.rs::pdf_redact_region`.
6. The command looks up the document actor and sends `Message::RedactRegion`.
7. The actor calls into `src-tauri/src/security/redact.rs::redact_region`.
8. `redact_region` opens the page via PDFium, walks page objects in the region, removes their content streams, writes back.
9. Actor emits `Event::DocumentChanged { document_id, change: Redacted { page, region } }`.
10. Frontend receives the event. `useDocumentStore` marks the doc dirty. PDF.js re-renders the affected page.
11. The redaction is now reflected in the view and persisted on next save.

There are no shortcuts. The redact tool does not write bytes directly. The frontend store does not know how PDFium implements the operation.
