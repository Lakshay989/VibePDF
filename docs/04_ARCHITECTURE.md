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

**Identity:** Each opened document gets a `DocumentId` (UUID). All commands take a `DocumentId` and the dispatcher routes the message to that actor's channel.

**Lifetime:** Actors are dropped when the document is closed. If the actor panics, the document is reported as crashed; the user gets a recovery offer from auto-save state.

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

---

## State on the frontend

Five zustand stores. Each store owns a clearly scoped slice. Stores never reach into other stores.

| Store | Owns | Source of truth? |
|---|---|---|
| `useDocumentStore` | Open documents, current document id | Yes, but derived state syncs from backend events |
| `useToolStore` | Active tool, tool options | Yes |
| `useViewStore` | Zoom, scroll position, page index, sidebar visibility | Yes |
| `useSelectionStore` | Selected text/region/object | Yes |
| `useSettingsStore` | App-wide settings | Mirrored to Rust on every change |

**Backend events** (annotations added, pages changed, form fields modified, etc.) are pushed via Tauri's event system. The frontend listens and updates the relevant store. The backend is always the source of truth for the PDF itself; the frontend stores cache *derived views*.

---

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
2. **Auto-save**: every 30 seconds, if the document is dirty, write a copy under Tauri's `app_data_dir()` at `autosave/<documentId>.pdf`. On startup, scan this directory and offer recovery for any open-at-crash documents.

Auto-save is invisible to the user — it never touches the user's original file.

**Path policy.** All persistent paths are derived from Tauri's `AppHandle::path()` helpers on every platform — there are **no hardcoded POSIX paths**:

| Concept | API | Typical resolved location |
|---|---|---|
| App config (settings) | `app.path().app_config_dir()` | macOS: `~/Library/Application Support/dev.vibepdf/` · Linux: `~/.config/dev.vibepdf/` · Windows: `%APPDATA%\dev.vibepdf\` |
| App data (autosave, recents, license cache) | `app.path().app_data_dir()` | Same roots, with `data/` substructure on Linux |
| Logs | `app.path().app_log_dir()` | macOS: `~/Library/Logs/dev.vibepdf/` · Linux: `~/.local/share/dev.vibepdf/logs/` · Windows: `%LOCALAPPDATA%\dev.vibepdf\logs\` |
| Crash dumps | `app_data_dir()/crashes/` | Same roots |

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
