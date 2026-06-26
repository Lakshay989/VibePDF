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
│   │   │   ├── cos.rs            # lopdf object-model layer (structural edits)
│   │   │   ├── page.rs           # Page operations
│   │   │   ├── text.rs           # Text editing
│   │   │   ├── annotation.rs     # Annotations
│   │   │   ├── xfdf.rs           # XFDF annotation import/export (P3.E1)
│   │   │   ├── flatten.rs        # Flatten annotations into page content (P3.E2)
│   │   │   ├── image_xobject.rs  # PNG → Image XObject + /SMask (P3.C3b)
│   │   │   ├── text_extract.rs   # Text-run extraction + doc font scan (live PDFium read, P4.A1/A2)
│   │   │   ├── font_resolver.rs  # Font fallback: base-14/system check + substitute (pure, P4.A2)
│   │   │   ├── form.rs           # Form fields
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

Auto-save is invisible to the user — it never touches the user's original file. The tick is a dedicated std thread (so no `tokio` `time` feature is needed) that pokes each document actor; a *dirty* actor writes its copy (atomic temp+rename) and clears it again on a clean save or a graceful close. A real crash never runs that cleanup, so the copy survives to be offered next launch (`recovery_list` / `recovery_discard`). Implemented in `pdf/autosave.rs`; the actor owns the write because PDFium is single-threaded per document.

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
live document; the actor instantiates `UndoStack<PdfDocument>`. Depth is
capped (`MAX_UNDO_DEPTH`) because an inverse can retain page content (a
deleted page must be remembered to restore it). See `pdf/undo.rs`.

Granularity is **page-level**: move, insert, delete, rotate, crop, resize
are each one undoable action. The frontend mirrors only `{canUndo,
canRedo}` (per document, in `history-store.ts`) to drive button state; it
learns the new availability from each `pdf_undo`/`pdf_redo` (and, later,
each mutating command's) return value. This is "session history" — it is
**not** persisted across restarts and is independent of what is saved to
disk (cf. flatten in `docs/02`, line 126).

---

## Settings storage

Settings live in two places:

- **App-wide** (theme, language, AI backend, telemetry preference): `<app_config_dir>/settings.json`.
- **Per-document** (last zoom, last page, sidebar state): keyed by the SHA-256 hash of the document path, stored in IndexedDB on the frontend.

The Rust side owns app-wide settings; frontend reads them via a one-shot command on startup. Changes propagate via events.

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
