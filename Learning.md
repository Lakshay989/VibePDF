# Learning.md — Concepts, tech, and why we used them

> A working journal of the software-engineering ideas this project
> exercises. Each commit that ships a step also appends to this file.
> Read top-down for a curriculum, or jump to a specific step.
>
> **Audience:** you, future-you, and anyone reading the repo who wants
> to know *why* something is the way it is, not just *what* it does.

---

## Table of contents

- [0. The bootstrap](#0-the-bootstrap)
- [1. Architecture concepts that show up everywhere](#1-architecture-concepts-that-show-up-everywhere)
- [Steps](#steps)
  - [P1.A1 — Drag-and-drop file open](#p1a1--drag-and-drop-file-open)
  - [P1.C1 — Virtual-scrolling page list](#p1c1--virtual-scrolling-page-list)
  - [P1.C3 — Keyboard navigation](#p1c3--keyboard-navigation)
  - [P1.C2 — Zoom + fit modes with per-document persistence](#p1c2--zoom--fit-modes-with-per-document-persistence)
  - [P1.C5 — Dark-mode page invert](#p1c5--dark-mode-page-invert)
  - [P1.D2 — Outline sidebar](#p1d2--outline-sidebar)
  - [P1.C4 — Text search (Cmd/Ctrl+F)](#p1c4--text-search-cmdctrlf)
  - [P1.E4 — Acceptance fixture generator](#p1e4--acceptance-fixture-generator)
  - [P1.B1 — Real document actor (per-document thread, mpsc + oneshot)](#p1b1--real-document-actor-per-document-thread-mpsc--oneshot)
  - [P1.B3 — Render-page-to-bitmap message + the PDFium render lock](#p1b3--render-page-to-bitmap-message--the-pdfium-render-lock)

---

## 0. The bootstrap

### What we built

A Tauri 2 desktop application that runs **two engines side by side**:

- A **frontend** (TypeScript + React) that lives inside the OS-native webview. It renders the PDF and owns the user interface.
- A **backend** (Rust) that owns the actual PDF bytes via PDFium, exposes mutating operations through Tauri's IPC layer, and never lets the frontend touch raw PDF data.

These two halves talk over a typed JSON RPC ("Tauri commands"). Nothing more.

### What's in each top-level folder

| Folder | Purpose |
|---|---|
| `docs/` | The specs. Source of truth for product behavior and architecture. |
| `src/` | The frontend (TypeScript + React + Vite + Tailwind). |
| `src-tauri/` | The Rust crate. Contains `Cargo.toml`, the Tauri config, and all PDFium code. |
| `tests/` | Integration tests and PDF fixtures. |
| `steps/` | Per-phase execution plan. Each entry = one commit. |
| `scripts/` | Helper scripts (e.g., fetching the PDFium native binary). |
| `Learning.md` | This file. |

### The pieces and why

| Piece | What it is | Why we picked it |
|---|---|---|
| **Tauri 2** | A framework that builds a native desktop app from a webview + Rust backend. | ~8 MB installer vs Electron's ~120 MB. Native Rust backend means PDF work lives in a real systems language. Capability-based IPC is much easier to audit than "everything is Node.js". |
| **React 19** | UI library based on declarative components and a virtual DOM. | Largest ecosystem and corpus. We're not doing anything React-specific that demands it, but the volume of available patterns matters when an LLM helps write the code. |
| **TypeScript 6 (strict)** | A typed superset of JavaScript. | PDFs are deeply nested data; untyped JS would silently lose information. `strict: true`, no `any`, no `@ts-ignore`. |
| **Vite 8** | A build tool: bundles your code, serves a dev server with hot reload. | Fast. Smallest viable pipeline for a non-SSR app. |
| **Tailwind CSS 4** | Utility-first CSS — instead of writing class names like `.button-primary`, you compose visual properties inline (`px-3 py-1 bg-blue-500 rounded`). | Matches tool UIs where dozens of small controls share styling. No design-system overhead. |
| **zustand 5** | Tiny global state library. Each "store" is a hook (`useDocumentStore`) that components subscribe to. | No boilerplate, scales fine, no Redux. |
| **pdfjs-dist 5** | Mozilla's PDF renderer — the same one that ships in Firefox. We use it for *viewing only*. | Most battle-tested PDF renderer in the world; permissive license; excellent text-layer support. |
| **pdfium-render 0.9** | Rust bindings to Google's PDFium (the engine inside Chromium). We use it for *all mutation*. | BSD-licensed. Full surface area: text edit, annotation, forms, signatures. Single-threaded per document — see the actor pattern below. |

### Why two PDF engines?

PDF.js is great at rendering but its mutation API is fragile. PDFium is great at mutation but rendering inside the webview from native code is awkward. We let each do what it's best at:

- PDF.js renders pixels inside the webview (fast, smooth scroll, text selection).
- PDFium reads and writes bytes on the Rust side (correct mutations, full PDF feature support).

The cost: every byte is parsed twice (once by each engine). Benchmarking shows this is fine; both engines are fast.

### The configuration files, briefly

| File | Role |
|---|---|
| `package.json` | Frontend dependencies + scripts. `npm run dev` is what you'll type 99 % of the time. |
| `tsconfig.json` | TypeScript compiler settings. `strict: true` is what enforces "no `any`". |
| `vite.config.ts` | The build tool's config: aliases, dev server port, Tailwind plugin. |
| `eslint.config.js` | Lint rules — extra checks beyond what TS catches. |
| `index.html` | The single HTML page. Vite injects the bundled JS. |
| `src-tauri/Cargo.toml` | Rust dependencies. The Rust answer to `package.json`. |
| `src-tauri/tauri.conf.json` | Window size, bundle identifier, which resources ship in the installer. |
| `src-tauri/capabilities/default.json` | What the webview is allowed to ask the Rust side to do. *Default-deny.* If a capability isn't listed, the frontend can't invoke it. |

---

## 1. Architecture concepts that show up everywhere

### IPC (Inter-Process Communication)

The frontend and the Rust backend are technically two processes. They communicate by **message passing**:

- The frontend calls `invoke("pdf_open", { path })`.
- Tauri serializes the args to JSON, routes the call to a function on the Rust side annotated with `#[tauri::command]`, awaits its return value, serializes it back.

Every IPC call is async and typed. Errors are values (`Result<T, CommandError>` on the Rust side; we surface them as a `CommandError` JS class with a `code` field) — never raw strings.

### The document actor (Rust side)

PDFium isn't thread-safe per document. To use it from many tasks at once without lock contention, we wrap each open document in an **actor**:

- A dedicated thread owns the `PdfDocument`.
- A `mpsc` (multi-producer / single-consumer) channel feeds it messages: `OpenPage`, `Save`, `RenderPage`, etc.
- The actor processes messages one at a time. Returns flow back via oneshot channels.

This is the standard pattern for serializing access to a non-thread-safe resource. It's also how Erlang/OTP, Akka, and Tokio's actor crates structure long-lived state.

Phase 1 ships a stub actor; the real one lands in P2.A1 / P1.B1.

### Strict separation: who owns the bytes?

**The Rust backend owns the PDF bytes.** The frontend reads a derived, serializable view (page count, dimensions, annotations as JSON) and dispatches *intents* through IPC. It never holds a `Uint8Array` of the file content for writing.

Why this matters: without this rule, two engines would mutate the same document state and drift. With it, there's one source of truth.

### Specs, EARS syntax, and stable IDs

Every feature is described in `docs/02_PRODUCT_SPEC.md` using EARS syntax:
- `WHEN <event>, THE system SHALL …`
- `WHILE <state>, THE system SHALL …`
- `IF <condition>, THEN THE system SHALL …`

Each statement has a stable ID (e.g. `P1-VIEW-006`). Tests reference it. Commits reference it. This is how spec, code, test, and commit history stay aligned.

### Step IDs, decoupled execution

`steps/P<n>.md` breaks each phase into independently shippable steps with IDs like `P1.C2`. Every commit references both the step ID and the spec ID:

```
feat: zoom + fit modes (P1.C2 / P1-VIEW-006)
```

This double-link means:
- From a commit, you can find the step (`git log --grep="P1.C2"`).
- From a step, you can find the spec.
- From a spec line, you can find the commit that implemented it.

This is what makes a 100-commit project navigable two years from now.

---

## Steps

### P1.A1 — Drag-and-drop file open

**Spec:** P1-VIEW-001 · **Commit:** `9313aaf`

#### Problem

The bootstrap could open a PDF via `Cmd+O`. We want to also open by dropping a `.pdf` file onto the window — and politely refuse anything else.

#### Concepts learned

**1. Tauri's event system.** In Tauri 2, native events (drag-drop, file association, window resize) flow into the webview via `getCurrentWebview().onDragDropEvent(handler)`. The handler receives a payload describing which file paths were dropped and where. Handlers return an `unlisten` function for cleanup.

**2. Pure functions vs side-effecting handlers.** We split the logic in two:
- `isPdfPath(path)` and `partitionPaths(paths)` — *pure*. No `await`, no DOM, no Tauri. They can be unit-tested with a single line of code.
- `registerDragDrop(callback)` — the side-effecting wrapper that subscribes to the Tauri event.

Why split? Because the *interesting bug* (does `foo.PDF` count as a PDF? does `foo.pdf.txt`?) lives entirely in the pure half. Putting the pure half in its own export lets us test it without a webview.

This is a general pattern: **keep your "decide what to do" code separate from your "do it" code.** The deciders are testable; the doers integrate them with the outside world.

**3. React's `useEffect` cleanup.** A drag-drop listener is a long-lived resource. If we mount it but never remove it, components that re-mount (which React does in StrictMode during development) leak listeners. So:

```ts
useEffect(() => {
  let unlisten: (() => void) | null = null;
  void registerDragDrop(onDrop).then((u) => { unlisten = u; });
  return () => { unlisten?.(); };
}, []);
```

The function returned from `useEffect` runs on unmount. Always return a cleanup if your effect subscribed to anything.

**4. Toast UX, minimum viable.** We didn't introduce a toast library; we used `useState<string | null>` plus a `setTimeout` to clear after 3 s. That's an example of *resisting premature abstraction* — when one toast is needed, one toast is fine.

#### Files in this step

| File | Role |
|---|---|
| `src/app/drag-drop.ts` | Pure path filter + the Tauri listener wrapper. |
| `src/app/__tests__/drag-drop.test.ts` | 4 vitest cases on the pure functions. |
| `src/app/App.tsx` | Mounts the listener; shows the toast. |

#### Further reading

- Tauri events: https://v2.tauri.app/develop/calling-frontend/
- React effect cleanup: https://react.dev/reference/react/useEffect#parameters

---

### P1.C1 — Virtual-scrolling page list

**Spec:** P1-VIEW-005, NFR-PERF-003 · **Commit:** `d10c601`

#### Problem

A 500-page PDF would crash the browser if we rendered all 500 pages as `<canvas>` elements. Each canvas is megabytes of GPU memory. We need to render only what's near the viewport.

#### Concepts learned

**1. Virtualization.** Instead of mounting one DOM node per item in a large list, we mount **placeholders** of the correct size and only swap in the heavy content when an item is near the viewport. The scrollbar still behaves correctly (because the placeholders have the right height), but memory stays bounded.

Libraries like `react-window`, `react-virtuoso` do this for general lists. We rolled our own because PDF pages have:
- Wildly different heights (depending on the document)
- Heavy render cost (PDF.js rasterization)
- A natural per-page concept (we want to know which page is "current")

**2. IntersectionObserver.** A browser API that fires a callback when an element enters or leaves a region of the viewport. Cheaper than listening to `scroll` and computing `getBoundingClientRect()` for every item.

```ts
const observer = new IntersectionObserver(
  (entries) => { for (const e of entries) setVisible(e.isIntersecting); },
  { rootMargin: "200% 0% 200% 0%" }
);
observer.observe(el);
```

`rootMargin: "200% 0% 200% 0%"` means *"treat the viewport as if it were 2× taller above and 2× taller below"* — so we pre-render pages just outside view, and they're ready by the time the user scrolls to them.

**3. LRU caches.** Least-Recently-Used: a cache that drops its oldest entry when it hits capacity. Implemented in 30 lines using a JavaScript `Map`, which preserves insertion order:

- `set(k, v)`: if `k` exists, delete it first (so re-inserting moves it to the tail). Insert. If over capacity, delete the head.
- `get(k)`: read, then re-insert (promoting to the tail).

Why an LRU here? When the user scrolls down quickly then scrolls back up, we don't want to re-rasterize pages we just unmounted seconds ago. The cache holds 50 page bitmaps — about 10 viewports of warm cache.

**4. React `forwardRef` and `useImperativeHandle`.** Most React component communication is via **props** (parent → child) and **callbacks** (child → parent). But sometimes a parent needs to *imperatively* tell the child "scroll to page 50" — and the child controls the DOM element that scrolls.

The solution is `forwardRef` + `useImperativeHandle`:

```tsx
export const PageVirtualizer = forwardRef<Handle, Props>((props, ref) => {
  useImperativeHandle(ref, () => ({
    scrollToPage: (n) => { /* … */ },
    scrollByLine: (px) => { /* … */ },
  }), [deps]);
  return <div>…</div>;
});
```

Then the parent: `<PageVirtualizer ref={virtRef} ... />` and later `virtRef.current.scrollToPage(50)`.

Use this sparingly — most things should be plain props. It's the right tool when you have **imperative side effects on DOM you don't own**.

**5. Separating natural dimensions from displayed dimensions.** A PDF page has a natural size in points (612 × 792 for US Letter). When we display it, we multiply by a *scale factor*. Storing only the displayed size means re-fetching from PDF.js every time zoom changes. Storing the **natural** size means scale changes are pure multiplication.

This is the same idea as "store the source of truth, derive everything else" — applied to page geometry.

#### Files in this step

| File | Role |
|---|---|
| `src/view/page-cache.ts` | The LRU. Pure data structure. |
| `src/view/__tests__/page-cache.test.ts` | 7 vitest cases. |
| `src/view/PageVirtualizer.tsx` | The virtualizer component. Owns the scroll container. |
| `src/view/PdfViewer.tsx` | Loads the PDF once, hands it to the virtualizer. |
| `src/view/render-page.ts` | Refactored into `renderPageOnDoc` (efficient, reused doc) and `renderPage` (one-shot). |

#### Further reading

- IntersectionObserver: https://developer.mozilla.org/en-US/docs/Web/API/Intersection_Observer_API
- LRU intuition: https://en.wikipedia.org/wiki/Cache_replacement_policies#Least_recently_used_(LRU)
- `useImperativeHandle`: https://react.dev/reference/react/useImperativeHandle

---

### P1.C3 — Keyboard navigation

**Spec:** P1-VIEW-005 · **Commit:** `07f0b3a`

#### Problem

`PageDown`/`PageUp` should jump pages. `Home`/`End` should jump to first/last. Arrow keys should scroll by a line — but only when nothing else (a search box, a future text editor) wants those keys.

#### Concepts learned

**1. Pure intent mapping.** Same pattern as A1 but applied to keyboard events. The function `keyToIntent(event, context)` takes:

```ts
interface KeyEventLike { key: string; metaKey?: boolean; /* … */ }
interface KeyContext   { inputFocused: boolean }
```

…and returns a `ScrollIntent` (or `null`). It has zero side effects. It doesn't know about React. It can be tested by passing object literals.

Why this matters: the *interesting* logic — "Cmd+ArrowDown is for the app, not for line scroll" — is in one place, testable without a browser. The component using the function just dispatches whatever intent comes back.

**2. The carve-out for input focus.** When the user is typing into a `<input>` or `<textarea>`, arrow keys belong to the input (moving the caret), not to the page scroller. We expose this as a `KeyContext`, not as a hard-coded check inside the mapping, so the mapping stays pure.

```ts
isInputFocused(document.activeElement)  // computed by the caller
keyToIntent({ key: "ArrowDown" }, { inputFocused: true })  // → null
```

**3. `event.preventDefault()`.** When we *do* handle a key (e.g., `PageDown`), we call `e.preventDefault()` so the browser doesn't also scroll. Without this, you get the system scroll *and* our scroll — double the distance.

**4. Discriminated unions in TypeScript.** Our intent type uses TS's pattern for "this can be one of several shapes, with a `kind` tag":

```ts
type ScrollIntent =
  | { kind: "page-target"; page: "first" | "last" }
  | { kind: "page-delta"; delta: number }
  | { kind: "line-delta"; delta: number };
```

When you `switch (intent.kind)`, TypeScript *narrows* each case's type — inside `case "page-delta":`, TS knows there's a `delta` field. This is how typed state machines are usually expressed in TS.

#### Files in this step

| File | Role |
|---|---|
| `src/view/keyboard-nav.ts` | `keyToIntent` + `isInputFocused`. |
| `src/view/__tests__/keyboard-nav.test.ts` | 8 cases covering every mapped key + the input carve-out. |
| `src/view/PdfViewer.tsx` | Wires the `keydown` listener into the virtualizer ref. |
| `src/view/PageVirtualizer.tsx` | Adds `scrollByPages` and `scrollByLine` to the imperative API. |

#### Further reading

- TS discriminated unions: https://www.typescriptlang.org/docs/handbook/2/narrowing.html#discriminated-unions
- `KeyboardEvent.key`: https://developer.mozilla.org/en-US/docs/Web/API/UI_Events/Keyboard_event_key_values

---

### P1.C2 — Zoom + fit modes with per-document persistence

**Spec:** P1-VIEW-006 · **Commit:** `16e31bc`

#### Problem

Three things at once:
1. A toolbar with `−` / `+` buttons that change the zoom level by sensible steps.
2. Fit modes (Actual / Fit page / Fit width / Fit height) that compute the right zoom from the *current container size*, and recompute when the user resizes the window.
3. Per-document persistence: close a PDF at 175 %, reopen it months later, still at 175 %.

#### Concepts learned

**1. State design: source of truth vs derived state.**

- The user's *intent* is what we persist: either an explicit `zoom: 1.75` or a `fitMode: "fit-width"`.
- The *effective scale* — the actual number that drives rendering — is **derived** from intent + container size.

We don't store the effective scale. Storing it would mean updating it on every resize, every zoom change, every fit-mode change, and getting them out of sync. By deriving it inside the virtualizer's `useMemo`, we get correctness for free.

The general principle: **store the smallest set of facts that determine all the others; derive everything else.**

**2. `ResizeObserver`.** The browser API for "tell me when this element's size changes." Used here to recompute the effective scale when the user resizes the window with fit-page selected.

```ts
const ro = new ResizeObserver(() => setContainerSize({ w: el.clientWidth, h: el.clientHeight }));
ro.observe(el);
return () => ro.disconnect();
```

Like IntersectionObserver, it batches notifications via the browser's rendering pipeline — much cheaper than polling `clientWidth`.

**3. IndexedDB (the browser database).** A built-in, key/value, async, transactional database in every modern browser. Unlike `localStorage`:
- It can hold structured data (objects, blobs), not just strings.
- It's async (doesn't block the main thread).
- It uses **transactions** — a group of reads/writes either all succeed or all fail.

Why per-document persistence lives in IndexedDB instead of `localStorage`:
- We expect to store more than scale per document (last page, sidebar state, search history). LocalStorage's 5 MB quota gets tight; IDB has gigabytes.
- IDB's transaction model means a partial save can never corrupt our state.

The cost is verbosity:

```ts
const req = db.transaction(STORE, "readonly").objectStore(STORE).get(key);
req.onsuccess = () => resolve(req.result);
req.onerror = () => reject(req.error);
```

…wrapped in a tiny promise helper (`openDb`, `loadViewSettings`, `saveViewSettings`).

**4. Cryptographic hashing as a key.** A document's filesystem path can be long, contain spaces, or change case. We use the **SHA-256 hash** of the path as the IDB key:

- Always 64 hex chars → fixed-length, looks the same in every browser tool.
- One-way → leaking the IDB contents doesn't immediately leak the user's directory layout. (Not a security control — the path's still on the filesystem — but a useful side effect.)

Browser SubtleCrypto handles SHA-256 natively:

```ts
const bytes = new TextEncoder().encode(path);
const digest = await crypto.subtle.digest("SHA-256", bytes);
// → ArrayBuffer of 32 bytes → hex-encode to 64 chars
```

**5. Debouncing.** When the user holds `Cmd+=` to zoom in rapidly, we get many state updates per second. Writing each one to IDB would be wasteful. Debouncing waits for activity to *stop* before committing:

```ts
useEffect(() => {
  const id = setTimeout(() => { save(zoom, fitMode); }, 200);
  return () => clearTimeout(id);
}, [zoom, fitMode]);
```

If `zoom` changes again before 200 ms elapses, the previous timeout is cleared and a new one is set. Only the *last* value within a quiet 200 ms window is written.

(Don't confuse debouncing with throttling: throttling guarantees *at most one call per interval*; debouncing guarantees *one call after activity stops*. Both are useful in different situations.)

**6. Testing async code with fake-indexeddb.** Real IDB doesn't exist in jsdom (vitest's default DOM). We add `fake-indexeddb` (an in-memory pure-JS reimplementation) as a devDependency and `import "fake-indexeddb/auto"` at the top of the test. The rest of the test reads as if IDB were available natively.

This is a common pattern: **make your code depend on a global API; in tests, swap the global for a fake.** As long as the fake honors the same contract, your real code stays unchanged.

**7. Mutually exclusive state.** A page can be at zoom = 175 %, *or* it can be in fit-width mode. It can't be both. Our state stores them together but with the discipline that setting one clears the other:

```ts
setZoom: (z) => set({ zoom: clampZoom(z), fitMode: null }),
setFitMode: (m) => set({ fitMode: m }),  // zoom kept, but it's now overridden
```

A more rigorous version would model this as a tagged union (`{ tag: "manual"; zoom } | { tag: "fit"; mode }`). We didn't, because the current shape is simpler to wire to the UI inputs (a number + a select), and the discipline lives in one file.

The general principle: when two pieces of state are mutually exclusive, either (a) model them as a union so the type system enforces it, or (b) keep them flat but funnel all writes through guard functions. Don't do neither.

#### Files in this step

| File | Role |
|---|---|
| `src/state/view-persistence.ts` | `pathHash`, `loadViewSettings`, `saveViewSettings`. The IDB layer. |
| `src/state/__tests__/view-persistence.test.ts` | 7 round-trip cases using fake-indexeddb. |
| `src/state/view-store.ts` | Zustand store: zoom, fitMode, the guards. |
| `src/app/ZoomToolbar.tsx` | The toolbar UI. |
| `src/view/PageVirtualizer.tsx` | Refactored to compute effective scale from intent + container size. |
| `src/view/PdfViewer.tsx` | Load IDB on path change, debounce-save on state change, wire shortcuts. |

#### Further reading

- IndexedDB: https://developer.mozilla.org/en-US/docs/Web/API/IndexedDB_API
- ResizeObserver: https://developer.mozilla.org/en-US/docs/Web/API/ResizeObserver
- SubtleCrypto.digest: https://developer.mozilla.org/en-US/docs/Web/API/SubtleCrypto/digest
- Debounce vs throttle: https://css-tricks.com/debouncing-throttling-explained-examples/

---

### P1.C5 — Dark-mode page invert

**Spec:** P1-VIEW-010 · **Commit:** _this commit_

#### Problem

PDFs are designed for printing — they assume a white page. In a dark
UI that white slab is blinding. We want, in dark mode:

- Background goes from white to near-black.
- Black text becomes white text.
- **Photos still look like photos** (don't invert a person's skin tone).

The naïve "invert all pixels" CSS filter (`filter: invert(1)`) gets
the background right but turns every photo into a negative.

#### Concepts learned

**1. Heuristics vs structural solutions.** The spec hints at parsing
PDF.js's operator list to find `Image` operations and skip their
bounding boxes. That's the *structural* fix: it knows what's actually
an image because the PDF told us. The cost: it requires per-page
operator parsing on every render, complex bbox geometry, and
coordination with the PDF.js render loop.

We chose the **heuristic** instead: per pixel, ask "is this near
grayscale?" Black-on-white text → yes, invert. A skin-tone pixel →
no, leave alone. The heuristic gets ~95 % of real PDFs right with
~30 lines of code and zero coupling to PDF.js internals. Cost: edge
cases (colored text won't invert; a grayscale X-ray scan *will*).

Engineering judgment: heuristics are fine when (a) the failure mode
is mild and (b) the structural fix is disproportionately expensive.
Both apply here. The Learning.md entry itself documents the
trade-off so we can revisit if anyone files a "my X-ray looks wrong"
bug.

**2. HSV saturation as "color-ness."** A pixel's saturation in HSV
is `(max(R,G,B) − min(R,G,B)) / max(R,G,B)`. It's 0 for any shade of
gray (including pure white and pure black), and approaches 1 for
vivid colors. Threshold at 0.15 ≈ "if R, G, B differ by less than
15 % of the brightest channel, treat as grayscale." That covers
slightly-aged white paper, scanned text with a yellow tint, etc.

**3. ImageData and the canvas pixel pipeline.** A `<canvas>`'s
contents are accessed via:

```ts
const ctx = canvas.getContext("2d");
const img = ctx.getImageData(0, 0, w, h);   // copies into RAM
mutate(img.data);                            // Uint8ClampedArray RGBA
ctx.putImageData(img, 0, 0);                 // copies back
```

The data layout is interleaved RGBA: `[R0, G0, B0, A0, R1, G1, B1, A1, ...]`.
This is the standard browser representation; the same shape is used
by `OffscreenCanvas`, `WebGL`, and `WebGPU` texture uploads.

Performance note: getImageData/putImageData are slow for big canvases.
A 1700×2200 page is ~22 MB of RGBA. The invert loop is JS-native
but the round-trip through the GPU is the slow part — typically
30–50 ms per page. For Phase 1's small-doc smoke demo this is fine.
A real production version would do the invert in a Web Worker against
an `OffscreenCanvas` (no main-thread block), or on the GPU via a WebGL
fragment shader.

**4. `Uint8ClampedArray`.** Browsers expose pixel data as a typed
array that **clamps** assignments to [0, 255] instead of wrapping. So
`data[i] = 300` writes 255, not 44. This matches what pixels do
naturally and saves us from writing `Math.max(0, Math.min(255, ...))`
in every assignment. Most language ecosystems would use `u8` here;
the clamping is the browser-specific touch.

**5. MutationObserver vs polling.** We need to know when `<html>`'s
class changes (because the theme system toggles `.dark`). Two
options:

- **Poll** — `setInterval(() => check(), 100)`. Always 10 wake-ups
  per second whether anything changes or not.
- **MutationObserver** — the browser calls *us* when the attribute
  changes. Zero work when the theme is steady.

We always prefer observers over polling in browser code: less battery
drain, simpler reasoning, and the event fires synchronously after the
mutation so we're never showing stale state.

```ts
const obs = new MutationObserver(update);
obs.observe(document.documentElement, {
  attributes: true,
  attributeFilter: ["class"],
});
```

The `attributeFilter` keeps the callback from firing when other
attributes (like `lang`) change — narrowing observers is good
practice.

**6. Including theme in a cache key.** The page-bitmap cache from
P1.C1 keys by `documentId:page:scale:dpr`. With dark mode we add a
flag: `documentId:page:scale:dpr:d` or `:l`. When the user toggles
themes, the key changes, the cache lookup misses, the page re-renders
under the new theme. Caching is now correctness-by-construction:
there is no path that could serve a light-mode bitmap to a dark-mode
view.

**7. A bug we fixed in passing.** While wiring dark mode I realised
the slot's existing render `useEffect` had a latent leak: on
`cacheKey` change it ran the new branch *without* clearing the
container, leaving the old canvas in the DOM. Light/dark toggle
turned that latent bug into a visible one (the old light canvas
sitting next to the new dark one). The fix is one line — always
clear children at the top of the effect — but the lesson is broader:
when an effect manipulates DOM directly (instead of declaratively via
JSX), it needs to be **idempotent**. The first thing it does should
be "put the world in the known starting state."

#### Files in this step

| File | Role |
|---|---|
| `src/view/dark-invert.ts` | The pure invert + canvas wrapper. |
| `src/view/__tests__/dark-invert.test.ts` | 8 cases covering the threshold edges. |
| `src/app/use-dark-mode.ts` | React hook that returns the current dark-mode state via MutationObserver on `<html>`. |
| `src/view/PageVirtualizer.tsx` | Accepts `darkMode`, threads it into the cache key, applies invert post-render, and (bug fix) always clears the slot before re-populating. |
| `src/view/PdfViewer.tsx` | Reads `useDarkMode()`, passes it to the virtualizer. |
| `eslint.config.js` | Adds `MutationObserver`, `Uint8ClampedArray` to globals. |

#### Further reading

- HSV/HSL color spaces: https://en.wikipedia.org/wiki/HSL_and_HSV
- MDN ImageData: https://developer.mozilla.org/en-US/docs/Web/API/ImageData
- MDN MutationObserver: https://developer.mozilla.org/en-US/docs/Web/API/MutationObserver
- Why heuristics beat parsers (sometimes): Joel Spolsky, "The Law of Leaky Abstractions"

---

### P1.D2 — Outline sidebar

**Spec:** P1-VIEW-009 · **Commit:** _this commit_

#### Problem

A PDF's outline (the "bookmarks" panel in Acrobat) is a tree of named
entries pointing to specific pages. We want to surface it as a
collapsible sidebar; clicking an entry should call into the
virtualizer's `scrollToPage` from P1.C1.

#### Concepts learned

**1. Tree data structures.** A tree is a node with `children`, each of
which is also a node. Recursive by construction. Most non-trivial UI
data structures are trees: filesystems, the DOM itself, this outline,
React component trees, even the JSX you write.

Two things you do with trees constantly:

- **Traversal.** Walk every node. Our `countOutlineEntries` is a
  textbook recursive sum: `count = 1 + sum(count(child) for child)`.
  This shape — base case + recursion — is how you reason about any
  tree operation.
- **Normalisation.** Take the upstream tree's shape and convert to
  the one your code wants. `normalizeOutline` does this: PDF.js gives
  us `{ title, dest, items? }`; we return `{ title, page, children }`
  with destinations already resolved.

**2. Dependency injection for testability.** PDF.js's outline contains
**destinations** (refs into the PDF) that have to be resolved to page
numbers by calling back into the document. If `normalizeOutline`
called `doc.getPageIndex` directly, we couldn't test it without a
real PDFDocumentProxy. Instead the resolver is a parameter:

```ts
export type DestinationResolver =
  (dest: RawDestination) => Promise<number | null>;

export async function normalizeOutline(
  raw: RawOutlineNode[] | null,
  resolvePage: DestinationResolver,   // ← injected
): Promise<NormalizedOutlineNode[]>
```

In tests, we pass a one-line fake (`async (d) => 5`). In production,
we pass a closure that wraps PDF.js. This is **dependency injection**
in its smallest form: hand the dependency *in* rather than letting
the function reach *out* for it.

Same idea applies broadly: keep your pure logic ignorant of how the
outside world gets it the data it needs. The outside world is for
the caller to assemble.

**3. PDF destinations.** A PDF "destination" can be:

- A direct array: `[pageRef, "/XYZ", 0, 700, null]` — points to a
  specific page (by indirect reference) and a position on it.
- A named string: `"my-named-destination"` — looked up in the
  document's name tree to get an array.

Both reach a page through `doc.getPageIndex(arr[0])` which returns
the 0-based index; we add 1 to display 1-based page numbers. The
resolver in `OutlinePanel.tsx` handles both shapes inside a single
try/catch (broken PDFs in the wild routinely have outline entries
pointing to deleted pages — we return `null` and disable the
button).

**4. Recursive components.** `OutlineEntry` renders a single node and
then maps over `node.children` rendering *itself* for each. This is
fine in React — components are just functions. The only thing to
watch is unbounded recursion (would blow the stack); a sane PDF
outline is a few levels deep.

The natural alternative is to flatten the tree into a list with
depth annotations and render with iteration. Both work; the
recursive version is closer to the data's shape and was clearer
here.

**5. The `aria-pressed` toggle button.** The "Outline" button on the
toolbar is a *toggle*, not a momentary action. We mark it with
`aria-pressed={showOutline}` so screen readers announce "pressed" /
"not pressed" instead of just "button." This is the right pattern
for any binary toolbar state (bold/italic, panel show/hide, mute/
unmute). Comparison points:

- `<button>` without `aria-pressed` → "button" (no state info)
- `<button aria-pressed="true|false">` → "toggle button, pressed"
- `<input type="checkbox" role="switch">` → "switch, on/off"
- `<button aria-expanded="true|false">` → "expandable section,
  expanded/collapsed" (for things that open *adjacent* content)

We used `aria-pressed` here because the toolbar button *is* the
state; the panel it controls is somewhere else on the page.
`aria-expanded` would have been right if the button were sitting
*directly above* the panel as a disclosure widget.

**6. A pattern about async useEffect.** Our outline panel does this:

```ts
useEffect(() => {
  let cancelled = false;
  setTree(null);
  setError(null);
  (async () => {
    try {
      const data = await loadAndNormalize();
      if (!cancelled) setTree(data);
    } catch (e) {
      if (!cancelled) setError(/* … */);
    }
  })();
  return () => { cancelled = true; };
}, [doc]);
```

Three things to notice:

- We **immediately** reset `tree` and `error` to their loading state
  on dependency change — so when the user switches docs, they don't
  briefly see the old document's outline.
- The async work is wrapped in an IIFE (`(async () => { … })()`)
  because `useEffect` can't take an async callback (it'd return a
  promise instead of a cleanup function).
- The `cancelled` flag is the standard race-condition guard. If the
  user switches docs faster than the previous outline resolves,
  the stale resolution must not overwrite the new state.

This three-part pattern (`cancelled` flag + IIFE + reset on entry)
shows up in almost every component that loads data from an async
source. Internalise it.

#### Files in this step

| File | Role |
|---|---|
| `src/panels/outline-tree.ts` | Pure `normalizeOutline` + `countOutlineEntries`. The dependency-injected resolver lives here. |
| `src/panels/__tests__/outline-tree.test.ts` | 9 cases on the normaliser (empty input, single level, deeply nested, null dest, broken resolver, named dest) plus the counter. |
| `src/panels/OutlinePanel.tsx` | The component: loads outline, resolves dests via PDF.js, renders the recursive tree with collapse/expand. |
| `src/app/ZoomToolbar.tsx` | New "Outline" toggle button with `aria-pressed`. |
| `src/view/PdfViewer.tsx` | Mounts the panel beside the virtualizer when `showOutline` is on; passes `scrollToPage` as the `onJump` callback. |

#### Further reading

- WAI-ARIA `aria-pressed` vs `aria-expanded`: https://www.w3.org/WAI/ARIA/apg/patterns/button/#wai-ariaroles,states,andpropertiesforatogglebutton
- Dependency injection (mentally): "Constructor injection in five minutes" — search any DI primer; the same principle scales from a single function to a whole framework.
- PDF destinations: ISO 32000-1 § 12.3.2 (named destinations) — the spec is dense but the relevant section is two pages.

---

### P1.C4 — Text search (Cmd/Ctrl+F)

**Spec:** P1-VIEW-007 · **Commit:** _this commit_

#### Problem

Cmd/Ctrl+F should pop up a search bar that finds the query across
**every** page of the open PDF, with case-sensitive and whole-word
toggles, a match-count badge, and next/previous navigation that
scrolls the viewer to each match.

Bootstrap-scope cut: we **scroll to** each match's page but don't
highlight the run of pixels yet. Inline highlights require rendering
PDF.js's text layer over the canvas (a sibling DOM tree of `<span>`
elements per text run) and wrapping matching ranges. That's a
~1 KLOC follow-up; the infrastructure for it lands here.

#### Concepts learned

**1. Splitting "find" from "search."** Two distinct things look the
same to a user but are different in code:

- **find** — a pure function on a string: "where in this text does
  the query occur?" Inputs: text, query, options. Output: ranges.
  No I/O, no Promise, no state.
- **search** — orchestration: walk every page, call find, collect
  results, manage cancellation.

We export them as `findRanges` (pure) and `searchDoc` (async). All
the interesting edge cases — case sensitivity, whole-word boundaries,
regex-metacharacter escaping, overlapping potential matches —
live in `findRanges`, which has 10 vitest cases. `searchDoc` just
calls `findRanges` per page.

Same `pure-core / impure-shell` pattern as P1.A1 and P1.C3.

**2. Regex as an implementation detail.** Our matching uses a regex
under the hood (`new RegExp(escaped, flags)`), but you'd never know
from the API. Callers pass a literal string and options; we escape
metacharacters internally with:

```ts
function escapeForRegex(s: string): string {
  return s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}
```

This matters because `findRanges("axb a.b", "a.b")` should match
the *literal* "a.b" (one position), not the regex `a.b` (would
also match "axb"). The escape table is the standard list of regex
metacharacters; it's worth memorising the regex you can paste into
any project: `/[.*+?^${}()|[\]\\]/g`.

**3. Word boundaries (`\b`).** The "whole word" toggle adds `\b`
before and after the (escaped) query. `\b` is the zero-width assertion
that matches *between* a word character and a non-word character.
"the" matches "the cat" but not "then" because in the second, `\b`
fails between `e` and `n` — both are word characters.

This is one of the few times regex zero-width assertions earn their
keep over a hand-rolled char-by-char check.

**4. The infinite-loop trap with `g`-flag regexes.** When you use
`re.exec` in a loop, you can get into trouble with **zero-width
matches** (a regex like `(?:)` matches the empty string anywhere).
Each iteration matches at the same position; `lastIndex` doesn't
advance; loop runs forever. Defensive line:

```ts
while ((m = re.exec(text)) !== null) {
  if (m[0].length === 0) { re.lastIndex += 1; continue; }
  // …
}
```

We don't *currently* allow user queries that match the empty string
(empty queries return `[]` early), but the guard costs nothing and
prevents future-us from regressing into the bug.

**5. Debounced effects.** When the user types "the", the input fires
`onChange` three times. Each one would kick off a 100-page document
walk. We debounce by 200 ms inside the effect:

```ts
useEffect(() => {
  const timer = setTimeout(() => { runSearch(); }, 200);
  return () => clearTimeout(timer);
}, [query, ...]);
```

If `query` changes before 200 ms pass, the cleanup clears the pending
timer and the next effect schedules a new one. Result: at most one
search per quiet period, no matter how fast the user types.

We saw this pattern in P1.C2 (debounced IDB writes). It's the same
shape every time — `setTimeout` in the effect body, `clearTimeout`
in the cleanup.

**6. Cancellation via a mutable signal object.** Async iteration over
N pages can't be aborted by simply returning from the effect — the
`for` loop inside `searchDoc` is already running. The standard
workaround: pass a small `{ cancelled: false }` object and have the
worker check it between iterations.

```ts
const signal = { cancelled: false };
void searchDoc(doc, q, opts, signal).then(/* … */);
return () => { signal.cancelled = true; };
```

When the user types another letter and a new effect run starts, the
old signal flips. The in-flight search sees `signal.cancelled` on
its next page boundary and returns early. Cheap and effective.

(This is the manual version of the browser's `AbortController` /
`AbortSignal`. We'd use that when interacting with `fetch` or any
API that natively accepts an `AbortSignal`. Our search worker is
homegrown, so the homegrown signal is fine.)

**7. Narrow selectors with Zustand.** The viewer subscribes to ten
slices of the search store: `isOpen`, `query`, `caseSensitive`,
`wholeWord`, `flat`, `currentIndex`, plus several actions. We
deliberately use one `useSearchStore(s => s.field)` call per slice
instead of one big destructure:

```ts
// good
const query = useSearchStore((s) => s.query);
const caseSensitive = useSearchStore((s) => s.caseSensitive);

// bad
const { query, caseSensitive, ... } = useSearchStore();
```

The narrow version only re-renders the component when **its**
slices change. The bad version re-renders on any change to the
store. Same idea as `React.memo`-ing a prop selector. Zustand's
default equality check is `Object.is`, so narrow selectors fall
through unchanged when their slice didn't change.

**8. Discriminated unions hiding in plain sight.** PDF.js's
`getTextContent()` returns `Array<TextItem | TextMarkedContent>` —
some entries have `str: string`, others don't. TypeScript's strict
mode caught us trying to use a type guard that lied about the
narrowed type.

The clean fix is `flatMap` with a narrowed return type:

```ts
content.items.flatMap((it) => {
  const s = (it as { str?: unknown }).str;
  return typeof s === "string" ? [s] : [];
})
```

`flatMap` lets each element produce 0-or-1 elements without an
intermediate `filter`. The single-`as`-cast is the controlled
type-system escape: we're saying "trust me, treat this as an object
with a maybe-`str` field." Doing the same with a type predicate
would have required asserting the full TextItem shape, which we
don't actually rely on.

**9. `aria-pressed` (revisited from P1.D2).** Both toggles on the
search bar — case-sensitive and whole-word — use the same
`aria-pressed={state}` we introduced for the outline toggle. The
pattern repeats wherever a button represents a binary on/off state
rather than a momentary action.

#### Files in this step

| File | Role |
|---|---|
| `src/view/search.ts` | `findRanges` (pure), `searchDoc` (async per-page), `totalMatches`. |
| `src/view/__tests__/search.test.ts` | 10 cases on `findRanges` + 2 on `totalMatches`. Covers regex-metacharacter escaping, `\b` word boundaries, overlapping potential matches, ordering. |
| `src/state/search-store.ts` | Zustand store: query, options, matches, currentIndex, open/close. Flattens per-page matches into a single list for next/prev navigation. |
| `src/app/SearchBar.tsx` | The UI. Auto-focus + select-all on open, Enter / Shift+Enter for next/prev, Escape to close. |
| `src/view/PdfViewer.tsx` | Mounts the bar, wires Cmd+F into `openSearch()`, runs the debounced + cancellable search effect, scrolls the virtualizer to the current match's page on index change. |
| `eslint.config.js` | Adds `HTMLInputElement`, `RegExpExecArray` to globals. |

#### Further reading

- MDN regex escape table: https://developer.mozilla.org/en-US/docs/Web/JavaScript/Guide/Regular_expressions/Cheatsheet
- Word boundary `\b`: https://www.regular-expressions.info/wordboundaries.html
- Zustand selector best practices: https://zustand.docs.pmnd.rs/guides/slices-pattern
- AbortController / AbortSignal (for when you graduate from the manual `{cancelled}` pattern): https://developer.mozilla.org/en-US/docs/Web/API/AbortController

---

### P1.E4 — Acceptance fixture generator

**Spec:** unblocks `docs/05_ROADMAP.md` Phase 1 acceptance demo · **Commit:** _this commit_

#### Problem

Three large PDFs (a 1000-page text doc, an encrypted PDF, and a
~500 MB stress PDF) are needed to actually *run* the Phase 1
roadmap acceptance demo. Committing them is silly (the large one
alone is half a gigabyte). We want a script that regenerates them
deterministically on demand.

#### Concepts learned

**1. Generated fixtures vs committed fixtures.** The default instinct
is "commit the test data so anyone can run the tests offline." That
works for `hello.pdf` (596 bytes). It does not work for `p1-large.pdf`
(500 MB). The threshold isn't fixed; rules of thumb:

- Under ~100 KB and human-meaningful → commit it. Reviewable in a diff.
- Reproducibly derivable from code → generate it. Smaller repo, no LFS
  bills, no merge conflicts on binary blobs.
- Genuinely human-curated and big (a sample contract, a customer PDF
  someone reported a bug on) → Git LFS or an out-of-band store.

`hello.pdf` is in the first bucket. `p1-spec.pdf`, `p1-encrypted.pdf`,
`p1-large.pdf` are in the second. The `tests/fixtures/acceptance/`
directory is gitignored for `*.pdf` with a README + script committed.

**2. The PDF file structure, hands-on.** Writing the minimal builder
forced us to internalize PDF's object graph:

```
%PDF-1.4
%<binary marker>

1 0 obj                       ← object 1, generation 0
<< /Type /Catalog /Pages 2 0 R >>  ← references object 2
endobj

2 0 obj
<< /Type /Pages /Kids [4 0 R 6 0 R] /Count 2 >>
endobj

...

xref                          ← cross-reference table
0 N
0000000000 65535 f            ← free object 0
0000000017 00000 n            ← byte offset of object 1
...

trailer
<< /Size N /Root 1 0 R >>
startxref
<offset of xref table>
%%EOF
```

Three things to notice:

- **Indirect references** (`2 0 R` = object 2, generation 0,
  reference). PDFs are graphs, not trees. Objects refer to each
  other by number, which lets the same object be referenced from
  multiple places (one font shared across all pages).
- **The xref table** lists the byte offset of every object so a
  reader can seek directly to it. This is what makes PDF random-
  access: you don't have to scan the whole file to find page 743.
- **The trailer's `startxref`** points to where the xref table
  starts. The reader opens at the end, reads `startxref`, jumps to
  the xref, walks it, and *then* loads the objects it needs.

The structure looks weird until you realize PDF was designed for
streaming + random access on machines with very little memory. The
indirection is the price.

**3. Subcommands with `argparse`.** Multi-mode CLIs use `add_subparsers`:

```python
sub = parser.add_subparsers(dest="cmd", required=True)
sp_spec = sub.add_parser("spec")
sp_spec.add_argument("--pages", type=int, default=1000)
sp_large = sub.add_parser("large")
sp_large.add_argument("--size-mb", type=int, default=500)
```

Each subcommand gets its own argument set. `args.cmd` tells you which
was invoked. Same shape as `git <subcommand>`, `docker <subcommand>`,
`npm <subcommand>`. Standard library, no dependencies.

**4. Conditional dependencies, surfaced clearly.** The script's
encryption path needs `pypdf`. The spec/large paths don't. We could
have made `pypdf` a hard requirement of the whole file. Instead:

```python
def generate_encrypted() -> Path:
    try:
        import pypdf
    except ImportError:
        print("error: pypdf is required ...\n"
              "  pip install -r tests/fixtures/acceptance/requirements.txt",
              file=sys.stderr)
        sys.exit(2)
    ...
```

The import is **inside the function that needs it**, and the error
message is **actionable** (it tells you the exact pip command).
Lazy imports like this are the right call when:

- One feature of a tool needs a heavy dep
- Other features don't
- You want the no-dep paths to work on a stock Python install

Cost: the import is re-attempted on every call. Negligible here
(one call per script invocation). Don't do this inside a hot loop.

**5. `from __future__ import annotations`.** The line at the top of
the file. Tells Python to lazily evaluate all type annotations —
they live as strings until something asks. Two reasons we use it:

- **Forward references work without quoting.** `def foo() -> list[Path]`
  on Python 3.8 would fail at runtime ("list is not subscriptable");
  with `__future__ annotations`, it's a deferred string and only
  evaluated by tools that ask for the resolved type.
- **Cheaper imports.** Annotations never get evaluated unless
  something (e.g. `inspect.get_type_hints`) demands it.

In Python 3.13+ this becomes the default. Until then it's a one-line
upgrade that pays off in any script that uses generic types.

**6. Byte-accurate output via `bytearray`.** We build the PDF in a
`bytearray` (mutable, efficient appends) and convert to `bytes`
once at the end. Concatenating `bytes` (`b"foo" + b"bar"`) makes a
new object each time — O(N²) for N appends. `bytearray.extend` is
O(amortised 1).

This is the same idea as `StringBuilder` in Java, `String::push_str`
on a `String` in Rust, `[]string` joined with `strings.Builder` in
Go. Whenever you're concatenating many things into one big buffer,
reach for the mutable builder.

**7. Escaping inside PDF string literals.** PDF strings live between
parentheses: `(Hello, world)`. So a string containing parens needs
escaping: `(He said \(yes\) loudly)`. And a backslash needs
escaping: `(C:\\Users\\…)`. Our `_escape_pdf_text` handles both.

Every text format has its own escape table. JSON: `"`, `\`, control
chars. SQL: `'`. Shell: a small encyclopedia. Always escape on the
*write* side, never trust the input side. The bug class is "string
injection," and the only defense is centralised escaping that the
write path can't bypass.

**8. The Python-version-multiplicity trap.** The smoke test caught
that `pip install pypdf` had used `python3.11` from Homebrew, but
the script was running with `/usr/bin/python3` (Apple's system
Python). They have separate `site-packages`. The fix:

```bash
$ python3 -m pip install pypdf
```

— always use the **same interpreter** to install that you'll use to
run. The `python -m pip` invocation forces them to match. The bare
`pip` command is shorthand for "whatever `pip` is on the PATH,"
which can drift.

Generalises beyond Python: any tool with a per-interpreter / per-
environment package set (npm + Node, gem + Ruby, cargo + Rust
toolchain) has the same trap. Invoke the package manager via the
tool you're going to run, not directly.

#### Files in this step

| File | Role |
|---|---|
| `tests/fixtures/acceptance/generate.py` | The script. Subcommands: `spec`, `encrypted`, `large`, `all`. |
| `tests/fixtures/acceptance/requirements.txt` | One line: `pypdf>=5.0`. Only needed for the encrypted fixture. |
| `tests/fixtures/acceptance/README.md` | Documents each fixture, the password, and the regeneration commands. |
| `.gitignore` | Ignores `tests/fixtures/acceptance/*.pdf` so generated outputs don't get committed. |

#### Further reading

- PDF 1.7 spec, § 7.5 "File structure" — the canonical reference for the layout above. Free PDF on Adobe's site.
- Python `argparse` tutorial: https://docs.python.org/3/howto/argparse.html
- `from __future__ import annotations` rationale: PEP 563.
- Python interpreter-vs-package-manager pitfalls: "Why you should use `python -m pip`": https://snarky.ca/why-you-should-use-python-m-pip/

---

### P1.B1 — Real document actor (per-document thread, mpsc + oneshot)

#### Problem

The bootstrap left a stub `DocumentActorHandle` that owned only a path
and a `tokio::sync::mpsc` channel feeding a no-op consumer. Every
`pdf_open` call still re-opened the file via PDFium directly to read
metadata, then dropped the document — so the actor map held nothing
useful and concurrent reads serialised on a `Mutex<PdfDocument>` that
didn't exist yet. Phase 2 onward (rotate, delete, redact, sign) all
need a single owner per document; B1 is the step that puts a real
owner in place.

#### Concepts learned

- **Actor pattern, applied to non-thread-safe libraries.** PDFium is
  single-threaded *per document*. A `Mutex<PdfDocument>` held across
  IPC awaits would block the whole runtime. The actor pattern instead
  gives each document its own OS thread and a mailbox; callers post
  messages and `await` replies. The lock is implicit (only one thread
  ever touches the PDF) and never crosses an `await`.
- **Std mpsc for the mailbox, tokio oneshot for the reply.** The
  worker is sync — it does `for msg in rx { ... }`. `std::sync::mpsc`
  is the right shape: synchronous, cheap `recv`. But the IPC command
  is `async`; if it `recv`'d a sync channel for the reply it would
  block a tokio worker thread. `tokio::sync::oneshot::Sender::send`
  is sync (callable from the worker), and the matching receiver's
  `await` suspends the caller — clean sync→async bridge.
- **Embedding the reply channel in the message.** Instead of pairing
  every send with a separate receiver, each variant carries its own
  `oneshot::Sender<T>`. This is the "Bastion / Ractor / Actix"
  pattern and is the canonical Rust translation of Erlang's
  `gen_server:call`.
- **`OnceLock` is not enough for fallible singletons.** Our first
  attempt used `OnceLock<Pdfium>` with the "check, then set" pattern.
  Two threads can both observe an empty `OnceLock`, both call
  `Pdfium::bind_to_system_library` (which calls `FPDF_InitLibrary`),
  both try to `set`, and the loser's `Pdfium` runs `Drop` — which
  unloads the library while the winner is still using it. Result:
  SIGTRAP under `cargo test`'s default parallel runner. The fix is
  `LazyLock<Result<Pdfium, String>>`: the initializer runs at most
  once, atomically, and the cached error string is re-wrapped per
  caller (because `PdfiumError` isn't `Clone`).
- **`generate_context!` is a build-time validator.** Tauri's
  `tauri::generate_context!()` macro reads `tauri.conf.json` and
  *opens* every referenced icon at compile time to validate format
  (RGBA-required for PNGs). Missing or RGB-only icons fail the build,
  not the bundle step. Bootstrapping a Tauri app therefore needs at
  least placeholder RGBA icons before any `cargo check` will pass.
- **`#[must_use]` and pedantic clippy.** With `clippy::pedantic` on,
  every public getter that returns a value (not `&Self`) wants
  `#[must_use]` so callers don't silently discard the answer.
  `Result<T, E>` is already `#[must_use]` by definition, so the lint
  only fires on functions returning non-`Result` values like `Uuid`
  or `&DocumentMetadata`.
- **`needless_pass_by_value` vs `std::thread::spawn`.** Clippy
  pedantic flags every owned argument that isn't consumed inside the
  function body. But thread closures own their environment — they
  can't borrow from the caller frame — so functions called from
  inside `move ||` closures have to take by value. The right move is
  `#[allow(clippy::needless_pass_by_value)]` on the worker function
  with a comment explaining why.
- **Drop = teardown.** The handle is intentionally non-`Clone`. When
  it drops, the mailbox `Sender` drops with it; the worker's
  `rx.recv()` returns `Err(Closed)` and the `for msg in rx` loop
  exits. No `Drop` impl is needed on the handle itself; we get the
  shutdown for free by structuring ownership correctly.
- **`expect_err` requires `Debug` on the Ok type.** When a test asserts
  "this Result should be Err", `Result::expect_err` formats the
  unexpected Ok value if it pops out — so the Ok type has to impl
  `Debug`. Deriving `Debug` on `DocumentActorHandle` is free because
  every field already does (`Uuid`, `PathBuf`, `mpsc::Sender<T>`,
  `DocumentMetadata`).

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/src/pdf/actor.rs` | Full rewrite. Public façade: `DocumentActorHandle`, `Message`, `Thumbnail`, `DocumentChange`, `DOCUMENT_CHANGED_EVENT`. Spawns the worker thread, owns the mailbox sender, exposes `page_count() / metadata_live() / render_thumbnail() / close()`. |
| `src-tauri/src/pdf/document.rs` | Extended `DocumentMetadata` (title, author, pdf_version). New `open_pdf(path, password)` returning the live `PdfDocument<'_>` for the actor to keep. `pdfium()` swapped from `OnceLock` to `LazyLock<Result<Pdfium, String>>`. |
| `src-tauri/src/commands/pdf.rs` | `pdf_open` now spawns the actor (which opens the file inside the worker thread) and returns the cached metadata. New `pdf_close` drops the actor handle, triggering worker shutdown. |
| `src-tauri/src/lib.rs` | Registered `pdf_close` in `invoke_handler!`. Added `#[allow(clippy::expect_used)]` justification on the de-facto-main `run()` fn. |
| `src/ipc/pdf.ts` | `closePdf(id)` wrapper. Extended `OpenedDocument` with optional `title / author / pdfVersion`. New exported `DocumentChange` discriminated union. |
| `src-tauri/tests/actor_smoke.rs` | Four integration tests against `hello.pdf`: page count, three independent actors, drop-then-respawn, typed error on bad path. |
| `src-tauri/Cargo.toml` | MSRV bumped 1.78 → 1.80 (for `LazyLock`), `tauri-build` constraint 2.11 → 2.6 (the only one that didn't resolve on crates.io). |
| `scripts/fetch-pdfium.sh` | `PDFIUM_RELEASE` bumped chromium/6996 → chromium/7857 (pdfium-render 0.9 needs symbols that landed in PDFium ≥7000). |
| `src-tauri/icons/*.png` | Placeholder RGBA PNGs so `tauri::generate_context!` validates. Real icons are a separate concern. |
| `.gitignore` | Added `src-tauri/gen/` (Tauri build artifacts, regenerated per machine). |

#### Further reading

- "Actors with Tokio" — https://ryhl.io/blog/actors-with-tokio/ — the
  canonical short read on this exact pattern (Alice Ryhl, Tokio
  maintainer).
- `std::sync::LazyLock` stabilisation notes (Rust 1.80) —
  https://blog.rust-lang.org/2024/07/25/Rust-1.80.0/#lazycell-and-lazylock
- pdfium-render guide on PDFium lifetimes —
  https://docs.rs/pdfium-render/latest/pdfium_render/
- Tauri 2 docs on `generate_context!` and icon validation —
  https://v2.tauri.app/develop/configuration-files/#tauri-config
- "Why `OnceLock` isn't enough for fallible singletons" — written up
  in this file because nobody else has, but the underlying pattern
  is in the stdlib docs example for `LazyLock`.

---

### Env fix — restore vitest green on Node 22

#### Problem

The Node 18.15 → 22.4.0 upgrade in P1.B1 left four vitest cases red
(`render-page.test.ts` × 1, `view-persistence.test.ts` × 3). The tests
themselves had not changed; the failures were drift in the test
*environment* after the clean `npm install` re-resolved devDependencies.
Per the workflow rule we cannot "fix" failing tests by editing
assertions, so the entire fix had to land as configuration, polyfills,
or dependency-version changes.

#### Concepts learned

- **Layered failures hide each other.** The user-supplied diagnosis
  listed two issues (DOMMatrix + IDB hook timeout) and was correct
  about those, but stubbing DOMMatrix unmasked three more failures in
  sequence on the same test: missing `GlobalWorkerOptions.workerSrc`,
  missing `Promise.try`, missing `Uint8Array.prototype.toHex`. Every
  fix has to be re-verified end-to-end; the symptom you see is just
  the first error V8 threw, not the only one waiting.
- **PDF.js's `legacy` build is the official Node escape hatch.** Modern
  `pdfjs-dist@5.7.x` declares `engines: { node: ">=22.13.0" }` and on
  older 22.x relies on `Promise.try` (V8 13.0, Node 22.10) and
  `Uint8Array.prototype.toHex` (V8 14.0, Node 22.13). The `legacy/`
  subpath ships a core-js-bundled build that polyfills both at module
  load. The library even prints a runtime warning ("Please use the
  `legacy` build in Node.js environments.") that we were ignoring.
  Aliasing in vitest costs nothing because production still uses the
  modern build.
- **Vite alias forms: prefix vs exact.** `resolve.alias` as an object
  matches prefix-first, so `{ "pdfjs-dist": ".../legacy/build/pdf.mjs" }`
  also rewrites `"pdfjs-dist/legacy/build/pdf.worker.mjs"` to
  `".../legacy/build/pdf.mjs/legacy/build/pdf.worker.mjs"` and fails
  resolution. The array form `{ find: /^pdfjs-dist$/, replacement: ... }`
  anchors the match so subpath imports still resolve normally — a
  must-know whenever you alias a package root.
- **PDF.js fake-worker short-circuit.** In jsdom there is no
  `Worker`, so PDF.js falls back to a "fake worker" that dynamically
  imports `GlobalWorkerOptions.workerSrc`. The render-page test sets
  `workerSrc = ""` — a leftover from when that disabled the worker —
  which in 5.x throws `"No GlobalWorkerOptions.workerSrc specified"`.
  PDF.js also honours `globalThis.pdfjsWorker.WorkerMessageHandler`
  as a short-circuit (checked *before* the workerSrc lookup), so
  preloading the worker module into `globalThis.pdfjsWorker` from
  the test setup keeps the test's source untouched.
- **`fake-indexeddb` 6.x tightened block semantics.** The IDB tests
  do `indexedDB.deleteDatabase()` in `beforeEach` without first
  closing the connection cached from the previous test (the
  module's `dbPromise` keeps it open across tests, intentionally,
  so production code paths don't have to). In 5.x this resolved
  fast enough that the next `open` raced through; in 6.x the spec-
  correct "blocked while a connection is open" path triggers and the
  follow-up `open` deadlocks forever (verified: a 30 s timeout did
  not help). Pinning to `^5.0.2` is a one-line workaround that does
  not require touching the test's open-DB pattern. If we ever want
  to move back to 6.x, the test setup needs to `db.close()` before
  `deleteDatabase()`.
- **TypeScript treats top-level `await` as needing module context.**
  Adding `await import(...)` to a `.ts` file that has no other
  `import`/`export` makes `tsc` fail with TS1375. A trailing
  `export {};` is the standard one-liner to mark the file as a
  module without changing runtime behaviour.

#### Files in this step

| File | Role |
|---|---|
| `package.json` | `fake-indexeddb` pinned `^6.2.5` → `^5.0.2`. |
| `package-lock.json` | Re-keyed after the pin; semantically only the fake-indexeddb subtree changes. |
| `src/test-setup.ts` | Added a minimal `DOMMatrix` constructor stub (legacy build still touches it at module load), preloaded the legacy worker into `globalThis.pdfjsWorker`, and added `export {}` so top-level `await` typechecks. |
| `vite.config.ts` | New `test.alias` entry aliasing the bare `pdfjs-dist` import (regex `^pdfjs-dist$`) to the legacy build. Subpath imports are intentionally not aliased. |

#### Further reading

- PDF.js "legacy build" rationale —
  https://github.com/mozilla/pdf.js/blob/master/README.md#legacy-build
- `Promise.try` TC39 proposal (Stage 4, V8 13.0) —
  https://github.com/tc39/proposal-promise-try
- `Uint8Array.prototype.toHex` TC39 proposal (Stage 4, V8 14.0) —
  https://github.com/tc39/proposal-arraybuffer-base64
- Vite `resolve.alias` array form —
  https://vitejs.dev/config/shared-options.html#resolve-alias
- fake-indexeddb 6.0 release notes (block semantics change) —
  https://github.com/dumbmatter/fakeIndexedDB/releases
- W3C IndexedDB spec — `deleteDatabase` while connections are open —
  https://www.w3.org/TR/IndexedDB/#dom-idbfactory-deletedatabase

---

### P1.B3 — Render-page-to-bitmap message + the PDFium render lock

#### Problem

D1 (thumbnail sidebar), the eventual full-page viewer fallback, E2
(visual-diff harness), and P3's export-to-image all need the same
primitive: "give me page N of document D as bytes at DPI X." B1
shipped the actor scaffolding but only a thumbnail-shaped message
that returned RGBA8 at a pixel-width target. B3 generalises to DPI
input + PNG-or-RGBA8 output, and factors the rasterisation into a
new `pdf::render` module that both messages share.

#### Concepts learned

- **DPI ↔ pixel-width conversion.** PDFium's render API takes a
  target *pixel* width, not a DPI. PDF page geometry is in
  PostScript points (1 pt = 1/72 inch by definition), so
  `pixels = (page_width_pt / 72.0) * dpi`. Clamping is essential:
  a user (or a UI bug) requesting 2000 DPI on a poster-size page
  yields a 600 GB allocation request that PDFium will earnestly
  attempt.
- **Process-global state in a "thread-safe" library.** PDFium's
  documentation calls the binding "thread-safe to share" — the
  `Pdfium` struct can be sent across threads and held by an `Arc`.
  But the *render subsystem* (`FX_GE`) has process-global mutable
  state, and two concurrent `render_with_config` calls — even from
  separate `PdfDocument` instances — race and crash with SIGTRAP /
  SIGABRT. Even `doc.pages().get(idx)` from two threads is enough.
  The per-document actor pattern serialises calls *within* one
  document; B3 adds a process-wide `Mutex<()>` to extend that
  guarantee across documents. The lock costs us multi-document
  render parallelism in exchange for correctness; for Phase 1's
  use-case (one viewer + one thumbnail panel per doc) the budget
  isn't tight.
- **`Vec<u8>` over Tauri's JSON IPC.** Default `serde_json` emits
  `Vec<u8>` as a JSON array of numbers — a 1 MB PNG round-trips as
  ~5 MB of JSON. Acceptable for thumbnails; for full-page renders
  the next layer of refactoring is either `#[serde(with = "serde_bytes")]`
  (base64-encoded inside JSON, ~1.3× overhead) or
  `tauri::ipc::Response` (raw bytes, no JSON envelope). Both are
  one-line swaps from B3's current shape — picking one prematurely
  would mean committing to a model before D1's profile data is in.
- **`PdfDocument<'a>` is invariant in `'a`.** Tried to factor a
  `lookup_page(doc, page) -> PdfPage<'a>` helper out of both
  `render_page` and `render_thumbnail`. The compiler refused: with
  `<'a>(doc: &'a PdfDocument<'a>, ...)`, callers holding
  `PdfDocument<'static>` can't unify the two lifetimes (the outer
  ref scope ≠ `'static`). The fix that's not worth the effort is a
  closure callback pattern; the fix that is worth the effort is to
  inline the four-line lookup into both call sites. Sometimes the
  duplication is the right answer.
- **The `png` crate vs reaching into `image`.** PDFium emits raw
  RGBA; we need PNG bytes for the wire. `image = "0.25"` is already
  in the dep graph transitively (pulled in by pdfium-render's image
  conversion helpers), but reaching into a transitive dep is
  fragile — the upstream is free to swap it for `tiny-skia` or
  whatever next year. `png = "0.17"` is the standalone Mozilla
  encoder used by both image-rs and Firefox; ~100 KB, no decoders,
  no manipulation routines we'll never call. The justification went
  in `Cargo.toml`'s dep block as a comment so future readers
  understand why both crates exist in the graph.
- **`#[must_use]`, `unwrap_used`, and the test allow-list.** Clippy
  pedantic's `unwrap_used` lint catches `.unwrap()` everywhere —
  including in `#[cfg(test)]` blocks. The conventions doc allows
  unwrap in tests, but the lint doesn't know that. `#[allow(clippy::unwrap_used)]`
  on the offending test fn with a one-line justification ("test
  code; the harness panics on Ok anyway") is the standard escape
  hatch.

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/src/pdf/render.rs` | New. Shared DPI math, `PdfRenderConfig` builder, `RENDER_LOCK` process-wide mutex, PNG encoder. Both `Message::RenderPage` and `Message::RenderThumbnail` route through here. |
| `src-tauri/src/pdf/actor.rs` | Added `Message::RenderPage { page, dpi, format, reply }` variant. Added `render_page` (await-the-reply) and `render_page_request` (send-only; lets the IPC layer drop the actor-map lock before awaiting) handle methods. `Message::RenderThumbnail` now returns `RenderedPage` (was `Thumbnail`); the local helper is gone. |
| `src-tauri/src/pdf/mod.rs` | `pub mod render;` |
| `src-tauri/src/commands/pdf.rs` | New `pdf_render_page` IPC command. Carefully drops the `Mutex<HashMap<...>>` guard *before* awaiting the reply, to avoid holding the actor-map lock across an `.await`. |
| `src-tauri/src/lib.rs` | Registered `pdf_render_page` in the `invoke_handler!`. |
| `src/ipc/pdf.ts` | `renderPage()` wrapper, `ImageFormat` union (`"png" \| "rgba8"`), `RenderedPage` type. |
| `src-tauri/tests/render_to_png.rs` | Five tests: PNG magic + size sanity, RGBA8 buffer-size invariant (catches stride padding), 2× DPI scaling, page-out-of-range typed error, RGBA8-larger-than-PNG sanity. Plus an `#[ignore]`'d release-only performance sentinel. |
| `src-tauri/tests/render_verification_artifact.rs` | Ignored helper; writes `/tmp/vibepdf-verify-{72,144}dpi.png` for human eyeball. |
| `src-tauri/Cargo.toml` | New top-level dep `png = "0.17"` with justification in the inline comment. |
| `src-tauri/src/error.rs` | Drive-by — backtick fix on a pre-existing uncommitted B2-flavoured change to `PasswordRequired` doc comments. Not B3's work, but already in the staging area. |

#### Further reading

- PDFium thread-safety: "PDFium and Multi-threading" — Google internal
  doc summarised at https://groups.google.com/g/pdfium/c/zZ4qwK0HxYE
  ("the rendering subsystem is not re-entrant").
- `png` crate encoder API —
  https://docs.rs/png/latest/png/struct.Encoder.html
- PDF specification, §8.2.2.1 "Coordinate systems" — the 1 pt = 1/72 inch
  origin for the DPI math.
- Tauri v2 IPC — JSON vs raw-bytes response paths —
  https://v2.tauri.app/develop/calling-rust/#returning-data
- Holding a mutex guard across `.await` (Tokio book) —
  https://tokio.rs/tokio/tutorial/shared-state#tasks-threads-and-contention
  (don't do it; B3's `pdf_render_page` carefully avoids it).

---

### P1.B2 — Encrypted-PDF password prompt

#### Problem

`docs/02_PRODUCT_SPEC.md` P1-VIEW-003 says the editor must prompt for a
password on encrypted PDFs, retry up to three times, and never persist
the password. Before B2 the actor already accepted `Option<String>` for
the password (B1 plumbed it through), but every IPC call hard-coded
`None`, and any wrong/missing password came back as a generic `PdfError`
toast with no way to recover. B2 is the UI + error-shape change that
turns "encrypted PDFs silently fail" into the spec'd interactive flow.

#### Concepts learned

- **Typed errors as a UI signal, not just a debug aid.** PDFium reports
  both "encrypted file, no password supplied" and "encrypted file,
  wrong password" with the same `FPDF_ERR_PASSWORD` code. The frontend
  doesn't need to distinguish them — both mean "show the prompt and
  ask again" — so the right design is **one** typed
  `CommandError::PasswordRequired` variant, carrying the absolute path
  so the dialog can label which file it's prompting for. The variant
  is the single boundary signal that swaps the UI affordance from
  "toast and give up" to "modal and retry."
- **Why the retry policy lives on the frontend, not the backend.** The
  backend stays stateless w.r.t. attempt counts: it returns
  `PasswordRequired` every time, no memory of how many times you've
  asked. That keeps the backend memorylesss and means the dialog can
  freely reset the counter when the user re-triggers the open later.
  If the policy lived on the backend you'd either need a session
  identifier (more IPC surface) or a global rate-limit that affects
  every document equally — both worse.
- **Promise-of-string for modal dialogs.** The retry loop in
  `src/app/open-with-password.ts` is a plain `async function` that
  calls `askForPassword(...)` and awaits a `Promise<string | null>`.
  The bridge from React state ("dialog is mounted with these props")
  to that promise is a `useRef` that stores the resolver — set when
  the dialog mounts, called from `onSubmit` / `onCancel`. This is the
  standard pattern for "convert a modal into an async function call"
  and works without any side libraries. Key subtlety: the dialog
  stays mounted across retries (we don't unmount/remount the modal on
  each wrong-password attempt) so the password input doesn't
  flash-empty — instead a `useEffect` keyed on the `request` prop
  clears the local input value when `attemptsLeft` decrements.
- **Password hygiene in practice.** The password string lives in three
  places only: (1) the dialog's `useState` (cleared on unmount and on
  every prompt-args change via the same `useEffect`), (2) the
  `askForPassword` resolver value (in scope for one microtask), (3)
  the `pdf_open` IPC arg → `DocumentActorHandle::spawn` → `open_pdf`
  → PDFium. The actor's `tracing::info_span!` was deliberately not
  extended with a password field (would be a real footgun); the
  worker drops the `Option<String>` immediately after the open call.
  No keychain, no localStorage, no autofill.
- **Drive-by-coordination hazard with parallel agents.** During the
  B2 implementation a second Claude session was concurrently shipping
  B3 in the same worktree. The B3 commit (`5303612`) folded my early
  `error.rs` edits into its own changeset as "drive-by ... B2 prep,
  not B3 work." The error variant wound up on `main` ascribed to B3,
  so this commit doesn't own that hunk in `git blame`. Future cross-
  instance work: either an explicit lock file convention or (better)
  per-instance worktrees would avoid this. For one-step overlap it
  was recoverable; for anything larger it would be painful.
- **`Promise<string | null>` vs an event emitter.** I considered an
  alternative design where the modal emits a `"submit"` event on a
  store and the retry loop subscribes. That's worse: it spreads
  state across two stores, requires unsubscribe-on-unmount
  bookkeeping, and produces a less obvious data flow. The async
  function pattern (with the resolver-ref) keeps the retry loop
  linear and locally readable, at the cost of one `useRef`.

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/src/error.rs` | Drive-by'd into B3's commit (`5303612`): adds `CommandError::PasswordRequired(String)` variant + Serialize arm + a smarter `From<PdfiumError>` that maps `PdfiumInternalError::PasswordError` to the new variant (every other PDFium error stays `PdfError`). |
| `src-tauri/src/commands/pdf.rs` | `pdf_open` signature gains `password: Option<String>`; threads it to `DocumentActorHandle::spawn`. Catches a bare `PasswordRequired("")` from the actor and enriches it with the absolute path before propagating, so the dialog can label which file it's prompting for. |
| `src-tauri/tests/encrypted_open.rs` | New integration test. Three cases: no password → `PasswordRequired`, wrong password → `PasswordRequired`, correct password → success + page-count round-trip. Skips with a clear regenerate instruction when the fixture is absent. |
| `src/ipc/invoke.ts` | Adds `"PasswordRequired"` to `CommandErrorPayload['code']`. Type-only change. |
| `src/ipc/pdf.ts` | `openPdfPath(path, password?)`; threads through to the IPC payload. |
| `src/app/open-with-password.ts` | New. The retry loop. Pure async glue, no React. 1 silent open attempt + up to 3 password attempts. Returns a discriminated union (`opened` / `cancelled` / `failed`). |
| `src/app/PasswordPromptDialog.tsx` | New. Radix Dialog (already in deps; first modal in the project). Pure controlled component — parent owns the prompt args, this owns only the in-input string. Effect-clears the input on every prompt-args change. |
| `src/app/App.tsx` | Manages the dialog state and the in-flight `Promise<string\|null>` resolver via a `useRef`. New `openByPath` is the single entry point for any path-driven open (Cmd/Ctrl+O, header button, drag-drop callback). Removes the old `openPdfDialog` indirection — file-dialog code is now inline so the same prompt path covers it. |
| `src/app/drag-drop.ts` | `handleDroppedPaths` and `registerDragDrop` gain an optional `askForPassword` callback. When supplied (production), encrypted drops route through the prompt; when omitted (test), behaviour matches pre-B2 exactly. |
| `tests/fixtures/PROVENANCE.md` | Notes that `p1-encrypted.pdf` is now also consumed by `encrypted_open.rs`, and that the Rust test skips with a regenerate hint when the fixture is missing. |

#### Further reading

- TC39 EARS syntax overview — https://alistairmavin.com/ears/
- PDFium error codes — `FPDF_ERR_PASSWORD` and friends —
  https://pdfium.googlesource.com/pdfium/+/HEAD/public/fpdfview.h
- Radix UI `Dialog` (modal best practices, focus management) —
  https://www.radix-ui.com/primitives/docs/components/dialog
- "Async wait for a React modal" (canonical resolver-ref pattern,
  Kent C. Dodds writeup) —
  https://kentcdodds.com/blog/the-imperative-prompt-component
- PEP 668 (`EXTERNALLY-MANAGED` and why a venv is now mandatory for
  the fixture generator on Homebrew Python) —
  https://peps.python.org/pep-0668/

---

### Fix — `dpi_target_width_math` test assertion (B3 follow-up)

#### Problem

The B3 commit (`5303612`) shipped with a failing unit test:
`pdf::render::tests::dpi_target_width_math` asserted
`target_width_from_dpi(612.0, 99_999.0) == 200_000`, but the function
returns `17_000`. The B3 commit body claimed "cargo test: 10/10 green"
— it counted the *integration* tests and missed this *lib* unit test
under the default runner.

#### Concept learned

- **A clamp can silently mask a later clamp — watch the ordering.**
  `target_width_from_dpi` clamps the DPI input to `[1.0, 2000.0]`
  **before** computing pixels, then clamps the pixel output to
  `MAX_PX = 200_000`. Because the DPI ceiling (2000) caps the result
  at `page_width/72 * 2000`, the output clamp is unreachable for any
  normal page size — a US-letter page tops out at `612/72*2000 =
  17_000` px. The test author reasoned "99_999 DPI → ~850k px → cap at
  200k" and forgot their own DPI clamp fires first. The lesson: when
  two guards stack, the **tighter, earlier** one wins, and a test that
  targets the looser later guard has to construct an input that
  actually reaches it. Fix kept both assertions meaningful: 99_999 DPI
  now asserts `17_000` (exercises the DPI clamp), and a separate
  `10_000` pt page width asserts `200_000` (genuinely exercises
  `MAX_PX`).
- **This was a wrong test, not a weakened one.** The "don't rewrite
  tests to pass" rule guards against gutting a *correct* test to hide
  a code bug. Here the code matched its own doc comment exactly; the
  test encoded wrong arithmetic. Correcting it (and *adding* coverage
  for the path it thought it was testing) is the opposite of
  weakening.

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/src/pdf/render.rs` | Test-only: corrected the `99_999` DPI assertion `200_000 → 17_000`, fixed its misleading comment, added a `10_000` pt page-width case that genuinely reaches the `MAX_PX` output clamp. No production code change. |

---

### P1.A3 — Recents (last 20, clearable, persisted)

#### Problem

P1-VIEW-012 wants the last 20 opened files surfaced on the start screen
and clearable. A `settings-store.ts` already had the cap/dedup logic but
backed it with `localStorage` and — critically — nothing in the app
called it (no UI, and `openByPath` didn't record opens). A3 moves
persistence to the Rust side per the architecture's "Rust owns app-wide
settings" rule, wires opens to record recents, and renders the list.

#### Concepts learned

- **The architecture already told us where this goes.**
  `docs/04_ARCHITECTURE.md` reserves a top-level `settings/` Rust module
  and states "the Rust side owns app-wide settings; frontend reads them
  via a one-shot command on startup." So no architecture-doc edit was
  needed — the module was pre-blessed. Worth re-reading the arch doc
  before inventing a home for persisted state.
- **Split the IO layer from the path-resolution layer for testability.**
  `settings::recents` has two strata: pure list logic (`push_front`) +
  disk IO against an explicit `&Path` (`load`/`save`), and *neither*
  knows about `AppHandle`. Only the thin `commands/recents.rs` wrappers
  resolve `app_data_dir()`. That split is what lets `tests/recents.rs`
  exercise everything against a temp file with no Tauri app — same
  pattern B1/B2 use to keep `cargo test` app-free. If `load`/`save` had
  taken an `AppHandle`, none of it would be unit-testable.
- **Atomic write = temp file + rename.** `save` writes to a uuid-suffixed
  sibling then `std::fs::rename`s over the target. `rename` within a
  filesystem is atomic on every platform we target, so a crash mid-write
  leaves the previous `recents.json` intact rather than a truncated file.
  Pairs with the defensive `load` (any read/parse error → empty list) so
  recents can never wedge the start screen.
- **Backend returns the post-mutation list; frontend never re-derives.**
  `recents_push` / `recents_clear` return the new `Vec<String>` and the
  zustand store sets state directly from it. The cap-at-20 + dedup +
  ordering live in exactly one place (Rust). The store is a pure mirror.
  This avoids the classic bug where frontend and backend both implement
  "the rules" and drift.
- **A `Mutex<()>` to guard a *file*, not data.** Two quick opens could
  race the read-modify-write of `recents.json`. `AppState.recents_lock`
  is a `Mutex<()>` — it owns no data; holding it just serialises the
  file transaction. Cheap, and opens aren't a hot path. (The actor map
  has its own separate mutex; we didn't reuse it because the scopes are
  unrelated and that would create false contention.)
- **Avoided a new dependency by reusing `uuid`.** The plan floated
  `tempfile` for tests; instead the tests build a unique temp path from
  the already-present `uuid` crate (`temp_dir().join(format!("…-{uuid}")`).
  No new dep, same isolation. CLAUDE.md treats new deps as a cost worth
  dodging when an existing one does the job.

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/src/settings/mod.rs` | New top-level module (`pub mod recents;`). |
| `src-tauri/src/settings/recents.rs` | Pure list logic (`push_front`, `MAX_RECENTS = 20`) + disk IO (`load`/`save`, atomic, defensive) against an explicit path. Versioned `{ version, paths }` on-disk format. |
| `src-tauri/src/commands/recents.rs` | `recents_list` / `recents_push` / `recents_clear` — resolve `app_data_dir()/recents.json`, take `AppState.recents_lock`, delegate to `settings::recents`, return the new list. |
| `src-tauri/src/commands/mod.rs` | `pub mod recents;`. |
| `src-tauri/src/lib.rs` | `pub mod settings;`, `recents_lock` field on `AppState`, three commands registered in `invoke_handler!`. |
| `src-tauri/tests/recents.rs` | 6 integration tests: dedup-to-front, cap-at-20, disk round-trip, missing-file→empty, corrupt-file→empty, save-then-clear. App-free (explicit temp paths). |
| `src/ipc/recents.ts` | Typed wrappers `listRecents` / `pushRecent` / `clearRecents`. |
| `src/state/settings-store.ts` | Recents half rewritten: localStorage → IPC. Hydrate on mount; mutations re-sync from the backend's returned list. Theme handling untouched. |
| `src/app/App.tsx` | Hydrate recents on mount; record successful opens (both `openByPath` and the drag-drop callback); render the recents list + "Clear recents" in `EmptyState`; clicking a recent re-opens via `openByPath`. |

#### Further reading

- Atomic file replace via rename — https://doc.rust-lang.org/std/fs/fn.rename.html
  (and why temp-write-then-rename is the standard durable-write idiom).
- Tauri v2 path APIs (`app_data_dir` and friends) —
  https://v2.tauri.app/reference/javascript/api/namespacepath/ (mirror
  of the Rust `AppHandle::path()` helpers).
- zustand "single source of truth" pattern for server-owned state —
  https://github.com/pmndrs/zustand#readingwriting-state-and-reacting-to-changes

---

### P1.E1 — Multi-document tab/session restore

#### Problem

P1-VIEW-011 ("support multiple PDFs as tabs") was already met by the tab
strip. E1 is the step doc's elaboration: re-open at launch the tabs that
were open at last quit, with the same active tab. It builds directly on
A3's `settings/` persistence and is the first feature whose correctness
hinges on React effect *ordering*, not just logic.

#### Concepts learned

- **Two module-level booleans beat a `useRef` under StrictMode.** React
  18 StrictMode (dev) mounts → unmounts → remounts the tree with a
  *fresh* component instance. A per-instance `useRef` gate is reset to
  its initial value on the remount and never re-set, so an effect that
  flips it once would gate forever-closed. The fix is module scope:
  `sessionRestoreStarted` (the restore IIFE runs at most once — no
  double-open / orphaned backend actors) and `sessionRestoreFinished`
  (the persist gate). Both survive the remount because module state
  isn't tied to a component instance. This is the non-obvious failure
  mode the implementation surfaced that the plan hadn't called out.
- **Gate the auto-save effect or it eats the thing you're restoring.**
  The persist effect keys on `[docs, currentId]`. On first mount that
  fires with `docs = []` — *before* the async restore has loaded the
  session — and would `saveSession([])`, wiping the file you're about
  to read. The `finished` gate makes the persist effect early-return
  until restore completes. The ordering works out because `restoreDocs`
  schedules a re-render while the IIFE's `finally` sets the flag
  synchronously in the same microtask, so by the time the persist
  effect actually runs (post-render) the flag is already true and the
  restored set gets persisted (idempotently).
- **Restore opens *silently*, on purpose.** Routing restore through the
  normal `openByPath` would (a) fire password prompts for every
  encrypted tab at launch (a prompt-storm) and (b) bump recents order.
  Restore instead calls `openPdfPath(path)` with no password inside a
  per-file `try/catch`: encrypted / missing / moved files are skipped,
  not surfaced. The user re-opens them from recents to get the prompt.
  "Skip, don't block" is the right posture for best-effort restore.
- **Share the durability primitive, not the data shape.** A3 hard-coded
  its atomic-write + defensive-read inside `recents.rs`. E1 lifted those
  into `settings::{write_atomic, read_json}` so `session.rs` reuses them
  — each concrete module is now just *its data shape* (`RecentsFile` /
  `SessionFile`, both versioned) plus a thin load/save. The 6 existing
  recents tests were the regression guard that the refactor preserved
  behaviour.
- **Coerce dangling references at the trust boundary.** `session.load`
  drops `active` to `None` if it isn't in the surviving `open` set (the
  active file was deleted and pruned). Doing it in `load` — not in the
  UI — means every consumer gets a consistent invariant for free, and
  it's unit-tested (`active_not_in_open_coerces_to_none`).

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/src/settings/mod.rs` | New shared helpers: `read_json<T>` (None on missing/corrupt) and `write_atomic` (temp-sibling + rename, parent-dir create). `pub mod session;`. |
| `src-tauri/src/settings/recents.rs` | Re-pointed `load`/`save` at the shared helpers; behaviour identical (its 6 tests unchanged). |
| `src-tauri/src/settings/session.rs` | `Session { open, active }` + versioned `SessionFile`; `load` (defensive + active-coercion) / `save`. AppHandle-free. |
| `src-tauri/src/commands/session.rs` | `session_load` / `session_save` — resolve `app_data_dir()/session.json`, take `AppState.session_lock`, delegate. |
| `src-tauri/src/commands/mod.rs` | `pub mod session;`. |
| `src-tauri/src/lib.rs` | `session_lock: Mutex<()>` on `AppState`; two commands registered. |
| `src-tauri/tests/session_restore.rs` | 5 tests: round-trip, missing→empty, corrupt→empty, active-not-in-open→None, empty round-trip. |
| `src/ipc/session.ts` | Typed `loadSession` / `saveSession` + `SessionState`. |
| `src/state/document-store.ts` | New `restoreDocs(docs, activePath)` bulk action — one `set()` so restore renders once; falls back to first doc when `activePath` is absent. |
| `src/app/App.tsx` | Module-level `sessionRestoreStarted` / `sessionRestoreFinished` flags; restore-on-mount effect (silent per-file open + `restoreDocs`); persist effect keyed on `[docs, currentId]`, gated on `finished`. |

#### Further reading

- React StrictMode — intentional double-invocation of effects in dev —
  https://react.dev/reference/react/StrictMode#fixing-bugs-found-by-double-rendering-in-development
- "Running an effect only once" / module-scope guards —
  https://react.dev/learn/you-might-not-need-an-effect
- Atomic file replace via rename (shared with A3) —
  https://doc.rust-lang.org/std/fs/fn.rename.html

---

### P1.A2 — CLI-arg file open

#### Problem

P1-VIEW-001 specifies that the user can open a PDF "via menu, drag-drop,
or command-line argument." The first two were already covered (existing
dialog + A1); A2 implements the third — `./vibepdf foo.pdf bar.pdf` →
two tabs. With A2 shipped, Track A is fully done (A1 ✓ A2 ✓ A3 ✓).

#### Concepts learned

- **`tauri::setup` runs before the webview / React mount.** The step
  doc floated emitting a `cli-open` event in `setup`, but at that
  point there is no listener registered yet, so the event is silently
  dropped — a classic emit-before-subscribe race. The fix is to invert
  the channel: parse argv in `setup`, **buffer** the paths in a
  `Mutex<Vec<String>>` on `AppState`, and let the frontend **pull** via
  a command on mount. The pull model has no timing dependency, is
  testable, and `mem::take`-drains so a redundant call is naturally a
  no-op. General lesson: anything raised during process bootstrap that
  must reach the frontend should be buffered, not emitted.
- **Closures captured at `useEffect` setup time can't see later
  hooks — refs bridge the gap.** The session-restore IIFE is created
  inside the first big `useEffect`, but the CLI drain needed to call
  `openByPath`, a `useCallback` declared later in the component body.
  Closing over `openByPath` directly is impossible (temporal dead zone
  + the IIFE pre-dates its definition). The standard pattern is a ref
  declared *early* (`useRef<typeof openByPath | null>(null)`) and
  **assigned during render** further down (`openByPathRef.current =
  openByPath`). Render-time ref writes are a documented React pattern
  for "always read the latest closure" without re-triggering effects.
  The restore IIFE awaits an IPC call first, so by the time it reaches
  the CLI drain phase the render has long since committed and the ref
  is populated.
- **Gate ordering inside an async IIFE matters.** I had to flip
  `sessionRestoreFinished = true` **before** the CLI drain (not in the
  `finally`), otherwise each CLI-opened tab would hit the persist
  effect, see the gate closed, and not be saved — leaving session.json
  in disagreement with what's actually on screen. The `catch` arm
  still sets the flag too, so user actions are saved even if restore
  fails.
- **Pull-drain is naturally StrictMode-safe.** E1's restore IIFE has a
  module-level `sessionRestoreStarted` guard, so it runs once even on
  StrictMode's dev mount/unmount/remount. The CLI drain lives inside
  that same IIFE → guarded for free. And even if some bug *did*
  re-trigger it, `mem::take` returned `Vec::new()` the second time.
  Defense in depth without a dedicated flag.
- **Reuse existing flow rather than duplicating policy.** Routing the
  CLI drain through `openByPath` (not raw `openPdfPath` + `openDoc`)
  means CLI-opened files automatically get the same password prompt
  (B2), recents push (A3), session-persist (E1), and dedup behaviour
  as any other open. Every later feature wired into `openByPath`
  comes along for free. Anti-pattern would have been re-implementing
  pieces of it inside the IIFE.

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/src/commands/cli.rs` | Pure `pdf_paths_from_args<I, S>` (skip argv0, keep `.pdf` case-insensitively, preserve order). `#[tauri::command] cli_take_pending_opens` `mem::take`s the buffer. |
| `src-tauri/src/commands/mod.rs` | `pub mod cli;`. |
| `src-tauri/src/lib.rs` | `cli_pending: Mutex<Vec<String>>` on `AppState`. In `setup`: parse argv, retain `is_file()`, populate buffer. Command registered. |
| `src-tauri/tests/cli_open.rs` | 6 parser tests: case-insensitive `.pdf`, drop argv0 + non-pdf, preserve order, empty input, only-argv0, `.pdf` boundary. |
| `src/ipc/cli.ts` | `takePendingCliOpens()` wrapper. |
| `src/app/App.tsx` | New `openByPathRef` (declared early, assigned at render after `openByPath`). CLI drain runs at the tail of the restore IIFE, calling `openByPathRef.current(p)` per path. Persist gate is opened *before* the drain so CLI tabs are saved. |

#### Further reading

- React refs for "latest closure" / render-time assignment —
  https://react.dev/reference/react/useRef#manipulating-the-dom-with-a-ref
  (and the wider "you can read/write refs during render if it's
  deterministic" pattern).
- Tauri 2 lifecycle (`setup` runs before the webview is ready) —
  https://v2.tauri.app/develop/state-management/#initialize-state
- `std::mem::take` for atomic drain-and-reset —
  https://doc.rust-lang.org/std/mem/fn.take.html

---

## How this file evolves

Every commit that ships a `steps/P<n>.md` step also appends a new section
here. The section should answer four questions:

1. **Problem** — what did this step exist to solve?
2. **Concepts learned** — what's the SWE/CS/web idea here? Briefly
   define new vocabulary the first time it appears.
3. **Files in this step** — the table from the step doc, plus a one-line role.
4. **Further reading** — one or two links per concept.

Aim for clarity, not encyclopaedic depth. The bar is "would a confident
junior engineer who hasn't touched this codebase be able to follow the
choices?" If yes, you're done.
