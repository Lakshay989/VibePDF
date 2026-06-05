# Backlog — decide later

Non-blocking items deferred out of an in-flight change. Nothing here gates
the current roadmap phase. When one is picked up, move it into the relevant
`steps/P<n>.md` (or open a spec line) and delete it from here.

> This file is for *judgement calls and polish*, not tracked feature work.
> Feature work lives in `steps/`. The roadmap is in `docs/05_ROADMAP.md`.

## Decisions to make (need a human call)

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
