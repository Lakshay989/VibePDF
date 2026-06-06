# Backlog — decide later

Non-blocking items deferred out of an in-flight change. Nothing here gates
the current roadmap phase. When one is picked up, move it into the relevant
`steps/P<n>.md` (or open a spec line) and delete it from here.

> This file is for *judgement calls and polish*, not tracked feature work.
> Feature work lives in `steps/`. The roadmap is in `docs/05_ROADMAP.md`.

## Decisions to make (need a human call)

- **Active reference rewriting needs a dict-level library (from P2.B2).**
  pdfium-render's outline/link/destination API is read-only. So we can't
  remove/rewrite references *to* a deleted page (they dangle) or fix
  *index-based* references — and the same wall blocks **P2-PAGE-002 (reorder)**,
  which the spec says must "update all internal references." Surviving
  *object-ref* destinations already track renumbering for free (verified).
  Decision: when reorder/full-reference-integrity is needed, add **`lopdf`**
  (COS/dict access) alongside PDFium — a `docs/03_TECH_STACK.md` library
  decision — and route the page-tree + reference edits through it. Until
  then, B-track edits keep object-ref integrity but don't clean dangling refs.

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
