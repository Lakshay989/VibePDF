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
`/QuadPoints`), so it bypasses the pointer lifecycle. Per-page stacking, bottom
to top: **canvas → text layer → annotation overlay** (the overlay is
click-through when idle, so selection reaches the text layer beneath).

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
