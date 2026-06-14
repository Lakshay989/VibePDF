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

### Review follow-up — shared `basename` + password-loop tests

#### Problem

A review pass after the A2 ship found two quality issues worth closing
before they compounded: (1) `basename` was implemented twice — in
`App.tsx` and `PasswordPromptDialog.tsx` — with *different* logic, and
(2) the password retry loop (`open-with-password.ts`), which implements
the literal P1-VIEW-003 "retry up to 3 times" clause, had no automated
test despite being trivially unit-testable.

#### Concepts learned

- **Duplicated helpers drift in *behaviour*, not just bytes.** The two
  `basename`s weren't copy-paste twins: `App.tsx` used
  `Math.max(lastIndexOf("/"), lastIndexOf("\\"))` (rightmost separator
  wins, correct for mixed paths), while `PasswordPromptDialog.tsx` used
  `lastIndexOf("/") + 1 || lastIndexOf("\\") + 1` (prefers `/`
  entirely, weaker on a path containing both). Consolidating forced a
  decision about which is correct — that's the hidden value of
  de-duplication beyond line count. Landed the rightmost-wins version
  in `src/app/paths.ts` (kept in `app/` next to its consumers rather
  than a new top-level dir, avoiding an architecture-doc change).
- **"Pure-ish" orchestration is unit-testable if you inject its
  effects.** `openWithPasswordPrompt` looks like UI glue but takes an
  injected `askForPassword` callback and only otherwise depends on
  `openPdfPath`. Mock that one import (`vi.mock("@/ipc/pdf")`) and the
  whole "1 silent attempt + up to 3 prompted" state machine is testable
  with no DOM — the kind of logic (off-by-one on the attempt count)
  that most rewards a test. 7 cases now pin the contract: silent open,
  prompt-then-correct, three-wrong→failed (with the ticking
  attemptsLeft / lastError), cancel, and non-password errors
  propagating from both the silent and the retry path.
- **Mind the vitest version's matcher surface.** `toHaveBeenCalled-
  ExactlyOnceWith` doesn't exist in vitest 2.1.9 (it's newer); the
  portable form is `toHaveBeenCalledTimes(1)` + `toHaveBeenCalledWith`.

#### Files in this step

| File | Role |
|---|---|
| `src/app/paths.ts` | New. Single `basename` (rightmost-separator-wins). |
| `src/app/App.tsx` | Dropped its local `basename`; imports the shared one. |
| `src/app/PasswordPromptDialog.tsx` | Dropped its (weaker) local `basename`; imports the shared one. |
| `src/app/__tests__/open-with-password.test.ts` | New. 7 tests over the retry loop (P1-VIEW-003). |

#### Further reading

- vitest mocking (`vi.mock` factory + `vi.mocked`) —
  https://vitest.dev/api/vi.html#vi-mock

---

### Review follow-up — split `App.tsx` into hooks

#### Problem

Four consecutive features (B2, A3, E1, A2) each bolted orchestration
onto `App.tsx`, growing it to 354 lines carrying six unrelated
concerns: recents, session restore, CLI drain, the password-prompt
state machine, drag-drop, and keyboard. The component had become the
place new wiring went by default — a god-component in the making. The
review pulled it apart before D1 and Phase 2 piled on more.

#### Concepts learned

- **Custom hooks are the unit of concern-extraction in React.** The
  fix wasn't more components (the JSX was fine) — it was moving the
  *stateful logic* into two hooks: `useFileOpen` (everything that turns
  a path or gesture into an open tab — password prompt, toast, the
  `openByPath` orchestrator, the file dialog, drag-drop, recents
  hydration) and `useSessionRestore` (the startup load + CLI drain +
  persist lifecycle). `App.tsx` dropped to 128 lines and now contains
  *zero* `useEffect`/`useState`/`useCallback` — it's composition +
  layout. A hook owns related state + effects + the callbacks that
  mutate them; that cohesion is exactly what was tangled in the
  component body.
- **Pass cross-hook dependencies as arguments, not shared module
  state.** `useSessionRestore` needs `openByPath`, which `useFileOpen`
  produces. Rather than hoist `openByPath` to a context or module
  global, `App` threads it: `const { openByPath } = useFileOpen();
  useSessionRestore(openByPath);`. The hook stashes it in a
  render-assigned ref (the same "latest closure" trick the inline code
  already used) so its once-only restore IIFE always calls the current
  function without depending on its identity.
- **Module-scope flags travel with the logic that needs them.** E1's
  `sessionRestoreStarted`/`sessionRestoreFinished` (the StrictMode-safe
  gates) moved into `use-session-restore.ts` alongside the effects they
  guard — they were always conceptually part of that lifecycle, not the
  component's. Their semantics are unchanged.
- **`tsc` + `eslint-plugin-react-hooks` are the safety net for an
  untested refactor.** `App.tsx` has no automated test, so the
  guarantee that this behaviour-preserving move didn't break a
  dependency array or drop a closure came from the type-checker plus
  the exhaustive-deps lint — both of which re-validate every moved
  effect. The new `open-with-password.test.ts` independently covers the
  one piece of pulled-apart logic with real branching. Manual
  verification of the full open/restore/CLI/drag flows still applies.

#### Files in this step

| File | Role |
|---|---|
| `src/app/use-file-open.ts` | New hook. Owns toast + password-prompt state, `openByPath`, `pickAndOpen`, the Cmd/Ctrl+O + drag-drop effects, and recents hydration. Returns `{ openByPath, pickAndOpen, toast, passwordDialogProps }`. |
| `src/app/use-session-restore.ts` | New hook. Owns the module-level StrictMode gates, the restore-on-mount IIFE + CLI drain, and the persist effect. Takes `openByPath` as an arg. |
| `src/app/App.tsx` | 354 → 128 lines. Composition + layout only; zero raw effects/state/callbacks. `EmptyState` unchanged. |

#### Further reading

- "Reusing logic with custom hooks" —
  https://react.dev/learn/reusing-logic-with-custom-hooks
- "You might not need an effect" (when state belongs in a hook vs not) —
  https://react.dev/learn/you-might-not-need-an-effect

---

### P1.D1 — Thumbnails sidebar with lazy generation

#### Problem

P1-VIEW-008 wants a collapsible thumbnails sidebar whose tiles are
generated lazily as they scroll into view. This is the first frontend
consumer of B3's `pdf_render_page`, and the first feature where the
backend (PDFium rasterisation) and the frontend (PDF.js view layer)
collaborate on the same surface.

#### Concepts learned

- **Two render engines, one thumbnail.** Per `docs/04`, PDFium owns
  thumbnail rasterisation (it already renders for export/OCR), so the
  PNG bytes come from the Rust side via `pdf_render_page`. But that
  command renders by **DPI**, and the spec wants a **96px-wide** tile.
  The DPI is computed frontend-side from the page's point-width, which
  the PDF.js `doc` already knows: `dpi = targetPx·72 / widthPt`. So the
  view-layer engine (PDF.js) supplies the geometry and the mutate-layer
  engine (PDFium) supplies the pixels — a clean division that needed no
  backend change (zero collision with the render module).
- **IntersectionObserver + ref-callback timing is a trap.** First
  attempt registered each tile with the shared observer via a callback
  ref. But callback refs run during commit *before* the `useEffect`
  that creates the observer — so `observerRef.current` was still null
  when the initial tiles registered, and they were never observed (→
  never loaded). Because all tiles mount at once (fixed `pageCount`, no
  virtualization), the robust fix was to drop the per-tile refs
  entirely: stamp each tile with `data-thumb-page` and, in the observer
  effect (which runs *after* commit, when every tile is in the DOM),
  `root.querySelectorAll("[data-thumb-page]").forEach(observe)`. One
  source of truth (the DOM), no ordering dance.
- **A `useEffect` that sets state it also depends on revokes its own
  work.** The tile load effect set `url`, and `url` was in its deps —
  so setting it re-ran the effect, whose cleanup called
  `URL.revokeObjectURL` on the blob URL just handed to `<img>` (a
  revoke-after-set race). Fix: a `startedRef` load-once guard so the
  effect's deps are all stable for the tile's lifetime; cleanup then
  runs only on real unmount, when revoking is correct. General rule:
  if an effect's cleanup frees a resource, that resource's identity
  must not be in the deps.
- **Key the stateful panel by document.** A tab switch reuses the
  `PdfViewer`/`ThumbnailPanel` instance with new props. With per-tile
  `url`/`failed`/`startedRef` state, that would show stale thumbnails.
  `<ThumbnailPanel key={documentId} … />` forces a clean remount —
  fresh observer, empty `visible` set, fresh tiles — which is simpler
  and more correct than threading `documentId` through every reset.
- **ESLint flat-config globals are curated, ES-builtins are free.**
  `no-undef` flagged `Blob` (a Web API) but not `WeakMap`/`ArrayBuffer`
  (ES builtins, recognised via `ecmaVersion`). The project lists Web
  APIs explicitly as they're first used; `Blob` joined `URL`,
  `IntersectionObserver`, etc.
- **TS 5.7's `Uint8Array<ArrayBufferLike>` ≠ `BlobPart`.** The backing
  buffer could in principle be a `SharedArrayBuffer`, which `BlobPart`
  excludes. A one-line copy into a fresh `ArrayBuffer`
  (`new Uint8Array(ab).set(png)`) sidesteps the variance without an
  unsafe cast — cheap for small thumbnails.

#### Files in this step

| File | Role |
|---|---|
| `src/panels/thumbnail-cache.ts` | New. IndexedDB get/put for PNG bytes keyed `${documentId}:${page}:${dpr}`, mirroring C2's `view-persistence.ts` (separate DB `vibepdf-thumbnails`, `_resetForTests`). |
| `src/panels/__tests__/thumbnail-cache.test.ts` | New. 4 tests: miss→null, round-trip, key independence by (doc,page,dpr), overwrite. |
| `src/panels/ThumbnailPanel.tsx` | Rewrote the stub: one shared IntersectionObserver over `data-thumb-page` tiles; on first-visible → cache-get → render+cache → blob-URL `<img>`. Load-once ref guard; active-tile highlight; click → `onJump`. |
| `src/view/PdfViewer.tsx` | Mount `<ThumbnailPanel key={documentId} …>` gated on `showThumbnails && doc`. |
| `src/app/ZoomToolbar.tsx` | "Pages" toggle button (mirrors the Outline toggle; store flag already existed). |
| `eslint.config.js` | Added `Blob` to the browser globals. |

#### Deviations from the step doc

- **No new `src/ipc/render.ts`.** B3 already shipped the render wrapper
  as `renderPage` in `src/ipc/pdf.ts`; D1 reuses it rather than
  duplicate. (`ipc/` wrappers are pure `invoke`s; the thumbnail DPI
  math needs the PDF.js `doc` and lives in the panel.)
- **"96-px-wide" is computed as a DPI frontend-side** (above) rather
  than a backend target-width parameter — avoids touching `render.rs`.

#### Further reading

- IntersectionObserver lazy-loading pattern —
  https://developer.mozilla.org/en-US/docs/Web/API/Intersection_Observer_API
- `URL.createObjectURL` / `revokeObjectURL` lifecycle —
  https://developer.mozilla.org/en-US/docs/Web/API/URL/createObjectURL_static

---

### P1.E2 — Render-failure log scaffold

#### Problem

P1-VIEW-004 sets a high bar — "same pixel-fidelity as Adobe Acrobat for
the W3C conformance suite, failures documented in
`tests/render-failures.md`." We have neither Acrobat output nor that
suite checked in. E2's job (per its title) is the *scaffold*: stand up
the comparison machinery + the failures log now, so the real
conformance work later just plugs into it.

#### Concepts learned

- **A scaffold reinterprets an unreachable spec into a reachable
  invariant.** The literal spec needs Acrobat; the Phase-1 reading is
  "match a committed golden produced by our own pipeline" — i.e. a
  **regression** baseline, not a fidelity reference. That's an honest
  narrowing: it catches *our renderer changing unexpectedly* (real
  value) without pretending to verify Acrobat-equivalence (which we
  can't yet). The distinction is documented in the log header and
  PROVENANCE so nobody mistakes the golden for an Acrobat truth.
- **Compare decoded pixels, not encoded bytes.** Goldens are stored as
  PNG (human-viewable — you can open it and see "Hello, VibePDF."), but
  the comparison renders to **RGBA8** and decodes the golden to RGBA8,
  comparing pixels. Comparing raw PNG bytes would couple the test to
  the `png` encoder's version/settings — a zlib bump would "fail" with
  identical pixels. Pixel comparison is robust to that.
- **Tolerance is the price of cross-platform rasterisation.** Text
  anti-aliasing can differ across PDFium builds / OSes, so an exact
  golden is a CI hair-trigger. The harness allows a per-channel
  |Δ| ≤ 16 and up to 2% mismatched pixels. On the same machine the
  self-render vs its golden is *exact* (PNG is lossless), so the
  tolerance only ever absorbs genuine cross-platform drift. The true
  fix (perceptual diff / normalized goldens) is future work.
- **A test that writes a tracked file must write deterministically.**
  The gate rewrites `tests/render-failures.md` every run — but on an
  all-match run the content is byte-identical (no timestamps, no
  fractions), so `git status` stays clean. Verified by hashing the file
  across two runs. The file only changes when a real divergence appears
  — which is exactly the signal you want in a diff. (If I'd embedded a
  timestamp, every `cargo test` would dirty the tree.)
- **`png` ships a decoder in default features.** B3 added `png = "0.17"`
  and used only the *encoder*; the *decoder* (`png::Decoder`) is
  available with no feature change, so E2 needed no new dependency. The
  `bless_goldens` test reuses the encoder path (`ImageFormat::Png`) to
  (re)write goldens.

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/tests/render_compare.rs` | The harness/test. `CASES` table → render to RGBA8 → decode golden → compare within tolerance → rewrite the log. Gate test `renders_match_goldens` (fails on divergence); `#[ignore] bless_goldens` regenerates goldens. |
| `tests/render-failures.md` | The committed log in its clean "no failures" state. Header documents the Phase-1 interpretation + tolerance + regen command. |
| `tests/fixtures/golden/hello-p0-72dpi.png` | Committed golden (612×792 RGBA, self-generated). |
| `tests/fixtures/PROVENANCE.md` | Golden provenance row (self-baseline, not Acrobat). |

#### Deviations from the step doc

- **Harness at `src-tauri/tests/render_compare.rs`, not
  `tests/integration/render_compare.rs`.** Cargo only compiles
  integration tests under the crate's `tests/` dir; every existing one
  lives in `src-tauri/tests/`. (`docs/04`'s `tests/integration/` is
  aspirational/unused.) The log stays at the spec-mandated
  `tests/render-failures.md`.
- **Golden is a self-render, not Acrobat output** (see above).

#### Further reading

- Visual regression testing tradeoffs (golden/snapshot fragility) —
  https://martinfowler.com/articles/nonDeterminism.html#LackOfIsolation
- `png` crate decoder (`Decoder::read_info` / `next_frame`) —
  https://docs.rs/png/latest/png/struct.Decoder.html

---

### Infra — CI workflow + fix the Rust test scripts

#### Problem

A whole-project audit turned up infra gaps: there was **no CI at all**
(`.github/workflows/` didn't exist), yet the "done" definition and
E3/E5's acceptance both assume it. And `npm run test:pdf` was
referenced in CLAUDE.md but didn't exist — and probing that exposed a
latent bug: `npm run test:rust` (`cd src-tauri && cargo test`) was
**broken**, because nothing put the fetched PDFium dylib on the
loader's search path (every green Rust run in this project had used an
explicit `DYLD_LIBRARY_PATH`).

#### Concepts learned

- **A dynamically-loaded native lib needs help at *run* time, not link
  time.** `pdfium-render` `dlopen`s `libpdfium` via
  `bind_to_system_library`, which walks the OS loader path —
  `target/debug/deps/`, `/usr/lib`, etc. — none of which contain our
  fetched `src-tauri/resources/pdfium/libpdfium.{dylib,so}`. There's no
  `build.rs` rpath or `.cargo/config` to bridge it, so the test binary
  must be launched with the lib dir on `DYLD_LIBRARY_PATH` (macOS) /
  `LD_LIBRARY_PATH` (Linux) / `PATH` (Windows). The fix is a tiny
  cross-platform Node wrapper (`scripts/cargo-test.mjs`) that picks the
  right var by `process.platform` and `spawnSync`s cargo with it — so
  `test:rust` and `test:pdf` work without the caller remembering the
  env dance.
- **CI runner choice can be a *correctness* decision, not just cost.**
  The obvious default is `ubuntu-latest`, but E2's render golden was
  generated on macOS arm64 with a pinned PDFium build; `render_compare`
  pixel-diffs against it. A Linux render would use a different PDFium
  build → anti-aliasing drift → likely a red run (the exact
  cross-platform fragility E2 documented). Running CI on
  `macos-latest` keeps the comparison apples-to-apples, needs no
  webkit apt deps, and mirrors dev. Cost is higher, but a green-on-the-
  facts pipeline beats a cheap-but-spuriously-red one. (Linux CI is
  viable later by regenerating the golden there.)
- **Doc drift is a real defect.** CLAUDE.md claimed `npm run test` runs
  "Vitest + cargo test" (it's Vitest-only) and `test:pdf` was "30+
  fixtures" (it didn't exist). An always-loaded instructions file that
  lies about its own commands quietly misleads every future session;
  fixing it to match reality is as real as a code fix.

#### Files in this step

| File | Role |
|---|---|
| `scripts/cargo-test.mjs` | New. Cross-platform `cargo test` wrapper that prepends the PDFium dir to the platform's loader-path var. Forwards extra args. |
| `package.json` | `test:rust` → routes through the wrapper (was broken); new `test:pdf` runs the PDF-touching tests (render_compare, render_to_png, actor_smoke, encrypted_open). |
| `.github/workflows/ci.yml` | New. macos-latest job: npm ci → fetch-pdfium → `npm run check` + `npm run test` + `npm run test:rust`. Concurrency-cancel on superseded pushes. |
| `CLAUDE.md` | Command table + "done" criteria corrected to match the real scripts. |

#### Caveat

The workflow's individual commands are all verified green locally, but
GitHub Actions itself can't be run here — the orchestration (action
versions, macOS-runner specifics, `npm ci` rolldown native binding on
the runner) gets its first real exercise on the first push/PR. Flagged
per the project's "don't claim a check you didn't get" rule.

---

### Bug — PDF.js worker asset was never placed in public/

#### Problem

Found by the first real GUI test: opening *any* PDF in the Tauri window
failed with "This file does not appear to be a valid PDF" +
"Setting up fake worker failed: Importing a module script failed."
`src/view/pdfjs-worker.ts` sets `GlobalWorkerOptions.workerSrc` to
`/pdfjs/pdf.worker.min.mjs`, but `public/` was **empty** — the worker
file was never copied there. So the URL 404'd and PDF.js couldn't
render a single page. The bootstrap designed the public-hosted-worker
approach but never completed the "put the file there" half.

#### Concepts learned

- **A whole class of bugs lives only in the real runtime.** Every
  automated test was green because the render smoke test *mocks*
  `configurePdfJsWorker` and the vitest config aliases pdfjs-dist to a
  preloaded legacy worker (the Node-env fix from a7184d5). None of that
  exercises the actual browser/Tauri worker-load path, so a missing
  public asset that breaks 100% of rendering sailed through CI. This is
  the textbook case for an E2E harness (E5): unit tests can't see "the
  asset 404s in the webview." Until E5 exists, a cheap file-existence
  test is the stopgap.
- **PDF.js needs its worker reachable as a URL, and *where* you host it
  matters under Tauri.** Two options: (a) host under `public/` so it's
  served verbatim by Vite (dev) and Tauri's asset protocol (prod), or
  (b) `import workerUrl from "…/pdf.worker.min.mjs?url"` and let Vite
  emit it. pdfjs-worker.ts deliberately chose (a) — the comment warns
  that bundling the worker through Vite is fragile inside the Tauri
  webview. The bug wasn't the *choice*, it was that nothing made the
  file appear under public/.
- **Generated assets belong in a copy step, not git.** The worker is a
  ~1.2 MB artifact owned by the pdfjs-dist version in node_modules.
  Committing it would rot on every bump. The fix mirrors how PDFium is
  handled (gitignored + fetched): a `scripts/copy-pdfjs-worker.mjs`
  copy wired to `postinstall` (fresh clones / CI), `predev` + `prebuild`
  (running the app), and `pretest` (so the regression test sees it).
  `public/pdfjs/` is gitignored.

#### Files in this step

| File | Role |
|---|---|
| `scripts/copy-pdfjs-worker.mjs` | New. Copies `node_modules/pdfjs-dist/build/pdf.worker.min.mjs` → `public/pdfjs/`. |
| `package.json` | `postinstall` / `predev` / `prebuild` / `pretest` hooks run the copy. |
| `src/view/__tests__/pdfjs-worker.test.ts` | New. Asserts the worker exists at the public path workerSrc points to — the regression guard for this exact bug. |
| `.gitignore` | Ignores the generated `public/pdfjs/`. |

#### Further reading

- PDF.js worker setup / `GlobalWorkerOptions.workerSrc` —
  https://github.com/mozilla/pdf.js/blob/master/examples/webpack/README.md
- Vite `public/` directory (served at root, copied to dist) —
  https://vitejs.dev/guide/assets.html#the-public-directory

---

### Bugs — thumbnail bytes + HiDPI page rendering

#### Problem

Two rendering defects surfaced once a PDF actually rendered in the
window (after the worker fix): the thumbnail sidebar showed a ⚠ on
*every* page, and the main-view text was blurry. Both were invisible to
the test suite.

#### Concepts learned

- **Tauri serializes `Vec<u8>` as a JSON number array, not bytes.** The
  `pdf_render_page` command returns `RenderedPage { bytes: Vec<u8> }`;
  over `invoke` that arrives in JS as a plain `number[]`. B3's frontend
  wrapper *typed* it `Uint8Array` (a runtime lie — never executed,
  since D1 was the first consumer and was never GUI-tested). The
  thumbnail code did `new ArrayBuffer(png.byteLength)` — `number[]` has
  no `.byteLength`, so it became `ArrayBuffer(0)` → `Uint8Array.set`
  threw `RangeError` → caught → ⚠ on every tile. Fix: type `bytes` as
  `number[]` (honest) and `Uint8Array.from(bytes)` at the boundary. The
  perf upgrade (raw bytes via `tauri::ipc::Response`, avoiding the
  ~5×-JSON blowup) is still noted in `render.rs` as future work.
- **HiDPI needs the canvas backing store scaled by devicePixelRatio.**
  `renderPageOnDoc` sized the canvas at `scale` logical pixels; on a
  2× retina display the browser stretched that 1× bitmap to 2× physical
  pixels → blurry text. The fix is the standard pattern: render the
  backing store at `scale × dpr` *physical* pixels, then display it at
  `scale` *CSS* pixels (`canvas.style.width/height`) so the browser
  *down*-samples a too-big bitmap (crisp) instead of *up*-sampling a
  too-small one (blur). Tell: `PageVirtualizer`'s LRU key already
  included `dpr` — the intent was there, but the renderer never used it.
- **These are precisely the bugs unit tests can't see.** Both depend on
  the real browser canvas + the real Tauri IPC byte path. The render
  smoke test mocks the worker and never rasterises; vitest's jsdom has
  no real devicePixelRatio rendering. This trio (worker-missing,
  thumbnail-bytes, HiDPI) is the strongest argument yet for E5 (E2E) —
  three app-breaking/UX-breaking bugs, zero red tests.

#### Files in this step

| File | Role |
|---|---|
| `src/view/render-page.ts` | `renderPageOnDoc` renders the backing store at `scale × dpr` and displays at logical size (HiDPI fix). New optional `dpr` input. |
| `src/ipc/pdf.ts` | `RenderedPage.bytes` retyped `Uint8Array` → `number[]` (matches what Tauri actually returns). |
| `src/panels/ThumbnailPanel.tsx` | `Uint8Array.from(rendered.bytes)` at the IPC boundary; defensive coerce on cache hits. |

#### Further reading

- HiDPI canvas (scale backing store by devicePixelRatio) —
  https://developer.mozilla.org/en-US/docs/Web/API/Window/devicePixelRatio#correcting_resolution_in_a_canvas
- Tauri IPC + binary data (`tauri::ipc::Response`) —
  https://v2.tauri.app/develop/calling-rust/#returning-data

### Gap — theme toggle UI was never wired (P1-VIEW-010)

#### Problem

C5 shipped the *machinery* for light/dark/system themes (`theme.ts`,
`useDarkMode`, the page invert) but nothing in the UI let the user
*choose* — the theme defaulted to "system", so a Mac in dark mode
forced the page invert with no escape. A user on a dark-mode Mac saw an
inverted (black) page and had no way to switch. Same "infra built, UI
not connected" shape as the missing PDF.js worker.

#### Concepts learned

- **"Feature shipped" ≠ "feature reachable."** C5's step was marked
  done with passing tests (the invert is unit-tested, the theme
  resolution works), yet the capability was unreachable because the
  control was never added. Tests on the *logic* don't catch a missing
  *entry point*. The fix was pure wiring: a `<select>` bound to the
  already-present `useSettingsStore.theme` / `setTheme`.
- **The chain was already complete behind the toggle.** `setTheme` →
  `setStoredTheme` (persist) → `applyTheme` (toggle the `.dark` class on
  `<html>`) → `useDarkMode`'s MutationObserver → `PageVirtualizer`'s
  cache key includes `darkMode`, so pages re-render with/without the
  invert. Adding the select was the only missing link.

#### Files in this step

| File | Role |
|---|---|
| `src/app/ZoomToolbar.tsx` | Added a Light / Dark / System `<select>` bound to `useSettingsStore` theme state. |

### Bug — dark-mode invert mangled text (pixel invert → CSS filter)

#### Problem

With the HiDPI fix in, light mode rendered crisply but dark mode looked
"pixelated / black on black." C5 inverted the page by rewriting canvas
pixels (`getImageData` → invert near-grayscale pixels → `putImageData`).
That heuristic produced rough, low-contrast text: edge/colored pixels
that missed the saturation threshold stayed dark on the now-black
background.

#### Concepts learned

- **Invert at the compositor, not in the bitmap.** A per-pixel invert
  bakes a lossy transform into the rasterised page. Replacing it with a
  CSS `filter: invert(1) hue-rotate(180deg)` on the canvas element lets
  the GPU compositor invert at *native* resolution at display time — so
  dark-mode text is exactly as crisp as light mode (the user confirmed
  light was perfect; now dark matches). It's also simpler and faster
  (no `getImageData` round-trip, which also forces a slow software
  read-back path).
- **`invert(1) hue-rotate(180deg)` ≈ "lightness invert, hue keep."**
  Pure `invert(1)` turns a photo into a colour negative; following it
  with `hue-rotate(180deg)` rotates hues back, so colours read as a
  darkened version rather than a negative. Bonus: colored text now
  inverts sensibly too (the old heuristic left it un-inverted —
  e.g. dark-red on black).
- **Trade-off made explicit.** The old heuristic *tried* to leave
  photos untouched (only inverting near-grayscale regions); the CSS
  filter inverts everything. Crisp text for every PDF beat
  photo-preservation for the rare image-heavy one. True
  photo-preservation needs the PDF operator-list approach C5's comments
  mention — a later refinement.
- **Deleting a feature's code means deleting its tests.** Removing
  `dark-invert.ts` made its 8 unit tests dead (they tested the now-gone
  pixel function). Deleting them (68 → 60 tests) is correct cleanup, not
  "removing tests to pass" — the behaviour they covered no longer
  exists.

#### Files in this step

| File | Role |
|---|---|
| `src/view/PageVirtualizer.tsx` | Dark mode sets `canvas.style.filter = "invert(1) hue-rotate(180deg)"` instead of calling the pixel invert. |
| `src/view/dark-invert.ts` | **Deleted** (pixel-invert heuristic, superseded). |
| `src/view/__tests__/dark-invert.test.ts` | **Deleted** (tested the removed function). |

#### Further reading

- CSS `filter` / `invert()` / `hue-rotate()` —
  https://developer.mozilla.org/en-US/docs/Web/CSS/filter
- "Dark mode for PDFs / images via invert + hue-rotate" (common technique)

### Test hardening — component/integration tests for the GUI-bug class

#### Problem

This session found five app-breaking bugs by hand that the whole green
test suite missed — all in the frontend (worker asset, thumbnail byte
type, HiDPI sizing, missing theme wiring, dark invert). Every existing
vitest was a *pure-function* test; nothing rendered a component or
exercised the IPC byte path. This adds the first real
component/integration tests, targeting that exact class.

#### Concepts learned

- **`@testing-library/react` was already installed but unused.** First
  real use: `render(<Component/>)` + `screen.getByRole/getByLabelText`
  in jsdom. No new deps, no jest-dom matchers needed — plain queries +
  vitest `expect`. Reset shared singletons (zustand store, the `.dark`
  class) in `beforeEach`, `cleanup()` in `afterEach`.
- **Drive state via the real interaction, not the store, to avoid
  `act()` warnings.** Setting zustand state directly after render and
  reading the DOM failed (React hadn't re-rendered). `fireEvent.change`
  on the `<select>` (a real user action) wraps in `act()` and flushes —
  the correct way to test the two-way binding.
- **Stub the browser APIs jsdom lacks.** The thumbnail test needed an
  `IntersectionObserver` (jsdom has none) that reports tiles visible
  immediately, plus `URL.createObjectURL` (unimplemented in jsdom) —
  both via `vi.stubGlobal`. That let the lazy-load path run headless and
  assert `<img>`s appear (vs the ⚠ failure glyph) when fed the real
  `number[]` IPC byte shape.
- **A regression test must be shown to fail on the bug.** Each guard was
  mutation-tested: reverting `scale × dpr` → `scale` made the HiDPI test
  red (`400 ≠ 800`); restoring the `number[]`→`byteLength` crash made
  the thumbnail test red ("Unable to find role=img"). A guard that stays
  green when you reintroduce the bug is theatre — verify the teeth.
- **Still a partial net.** These catch ~3 of the 5 (byte type, HiDPI,
  theme wiring). The worker-missing bug already has a file-existence
  test; the dark-invert is visual and best left to E5. Component tests
  narrow the gap but don't replace a real E2E harness.

#### Files in this step

| File | Role |
|---|---|
| `src/app/__tests__/ZoomToolbar.test.tsx` | First component test. Asserts the Theme control exists + is two-way bound (the "UI not wired" gap). |
| `src/view/__tests__/render-page-hidpi.test.ts` | `renderPageOnDoc` sizes the backing store at `scale × dpr`, CSS box at logical size (blur guard). |
| `src/panels/__tests__/ThumbnailPanel.test.tsx` | Renders `<img>`s from `number[]` IPC bytes, no ⚠ (the thumbnail-bytes crash). |
| `eslint.config.js` | Added `HTMLSelectElement`, `IntersectionObserver{Callback,Entry}` DOM globals. |

#### Further reading

- Testing Library queries / guiding principles —
  https://testing-library.com/docs/queries/about/
- "Avoid the `act` warning" (drive via fireEvent) —
  https://testing-library.com/docs/react-testing-library/api#fireevent

### P1.E5 — E2E harness (WebdriverIO + tauri-driver)

#### Problem

This session found five app-breaking bugs by hand that every automated
test missed — all only reproduce in the real Tauri webview (worker
loading over `tauri://`, IPC byte serialization, HiDPI canvas). E5 adds
the one test layer that drives the **real built app**, so that class is
catchable in CI.

#### Concepts learned

- **"Playwright + tauri-driver" doesn't compose.** The step doc (and
  `docs/03`/`docs/04`) named Playwright, but Playwright drives browsers
  over CDP/its own protocol — it can't attach to a Tauri webview.
  `tauri-driver` implements **W3C WebDriver**, which **WebdriverIO**
  (or Selenium) speaks. WebdriverIO is the officially documented Tauri 2
  E2E stack. Corrected the docs as part of the change (CLAUDE.md:
  architecture/tooling changes update the doc first).
- **A native file dialog is not webview-automatable.** WebDriver drives
  the *webview*, not OS chrome. So "open via the file dialog" can't be
  E2E-tested directly. The harness instead launches the app with the
  PDF as a **CLI argument** (the A2 path) — which exercises the same
  open→render pipeline and sidesteps the dialog. General lesson: design
  the E2E entry point around what the driver can reach.
- **Some infrastructure can't be verified from where you build it.**
  `tauri-driver` supports Linux/Windows only (no WKWebView WebDriver on
  macOS), and the dev machine is macOS. So this is the one step I wrote
  **blind** — I can only validate that the TS typechecks, the YAML
  parses, and the deps resolve; the real acceptance (`npm run test:e2e`
  green) happens on Linux CI and will likely need a fixup pass. Marked
  the step `[~]` (not `[x]`) and said so plainly — per "don't claim a
  check you didn't get."
- **Keep the slow, OS-specific job separate.** E2E gets its own
  `e2e.yml` on `ubuntu-22.04` (build the app + webkit + xvfb +
  tauri-driver), kept off the fast macOS `ci.yml` and off every-push
  (PRs + main only). The macOS `ci.yml` keeps the render-golden
  comparison apples-to-apples; the Linux E2E job never runs it.
- **The built app still needs PDFium + the worker.** The unbundled
  debug binary dlopens `libpdfium.so` (so `LD_LIBRARY_PATH` →
  `resources/pdfium`, which propagates fine on Linux — no macOS SIP
  stripping) and serves the worker from the embedded `dist/` (the
  `postinstall` worker-copy runs on `npm ci`, before `tauri build`'s
  `vite build`).

#### Files in this step

| File | Role |
|---|---|
| `tests/e2e/wdio.conf.ts` | WebdriverIO config: spawns tauri-driver, points at the built binary, passes hello.pdf as argv. |
| `tests/e2e/specs/smoke.e2e.ts` | Launch → wait for `[data-page="1"] canvas` → assert it rendered. |
| `tests/e2e/tsconfig.json` | Scoped TS config (wdio + mocha types); isolated from the app's `tsc`. |
| `tests/e2e/README.md` | How/why Linux-only, the CLI-arg entry rationale. |
| `.github/workflows/e2e.yml` | Separate Linux job: deps → build → `xvfb-run npm run test:e2e`. |
| `package.json` | `@wdio/*` + `tsx` devDeps; `test:e2e` script. |
| `docs/03_TECH_STACK.md`, `docs/04_ARCHITECTURE.md` | Playwright → WebdriverIO correction. |

#### Caveat

Written entirely from macOS, which can't run it. Locally validated:
e2e TS typechecks, `e2e.yml` is valid YAML, wdio CLI resolves, app
`check` + vitest unaffected. **Unverified:** the actual E2E run. First
CI run is the real test.

#### Further reading

- Tauri 2 WebDriver / WebdriverIO guide —
  https://v2.tauri.app/develop/tests/webdriver/example/webdriverio/
- `tauri-driver` (platform support) —
  https://crates.io/crates/tauri-driver

---

### P2.A1 — Save (explicit Cmd/Ctrl+S)

The first feature in the project that **writes PDF bytes to disk**. Phase 1
was all read-only (open, render, thumbnails). This sets the template every
later mutation (rotate, delete, merge…) will reuse.

#### Problem

Give the user a Save button that writes the open document back to disk —
without ever corrupting their file. Two non-obvious sub-problems:

1. A *no-op save* (saving a document you haven't edited) must leave the file
   **byte-for-byte identical** — same SHA-256. But PDFium re-serializes a
   document differently from how it was written originally (different xref
   layout, object order, compression), so "just re-save it" would change the
   bytes. The only correct no-op is to **not write at all**.
2. A write must never destroy the original until the new bytes are proven
   good. "Open the file, truncate it, stream new bytes" is how editors corrupt
   documents on a crash or a bad write.

#### Concepts learned

- **Dirty flag.** A boolean per document tracking "are there unsaved
  changes?" Starts `false`; mutation operations flip it `true`; a successful
  save resets it. A same-path save of a *clean* (non-dirty) document
  short-circuits to a true no-op — we never touch the file. In P2.A1 nothing
  sets it `true` yet (no edit ops exist), so every same-path save is a no-op;
  the page-op steps (P2.B*) will flip it. Building the flag now means those
  steps inherit correct no-op semantics for free.
- **Atomic write via temp + rename.** Write the new bytes to a sibling temp
  file in the *same directory*, then `rename()` it onto the destination.
  `rename` within one filesystem is **atomic** at the OS level — a reader sees
  either the whole old file or the whole new file, never a half-written one. A
  crash mid-write leaves a stray `.tmp`, not a corrupt PDF.
- **Why the temp must be a *sibling*.** `rename` is only atomic (and only
  succeeds without a copy) *within a single filesystem*. A temp in `/tmp` and a
  destination on another volume would fail with `EXDEV` ("cross-device link").
  Putting the temp next to the destination guarantees one filesystem.
- **Round-trip verification.** After writing the temp file, we re-open it in
  PDFium and confirm it has pages. A write PDFium can't read back is rejected
  *before* it can replace the original. This is the "no silent breakage" rule
  from CLAUDE.md made concrete.
- **`.bak` rotation.** When overwriting the original, the previous version is
  renamed to `<name>.bak` first — kept for exactly one save cycle (a prior
  `.bak` is overwritten). One free undo at the filesystem level.
- **Writes happen on the actor thread.** PDFium isn't thread-safe per
  document, so the save (and its verify-reopen) run inside the document
  actor's own thread via a new `Message::Save`, not in the async IPC handler.
  Same discipline as render: the command sends the message and awaits a
  one-shot reply, holding no lock across the `.await`.

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/src/pdf/document.rs` | `save_document()` — atomic temp+rename, `.bak` rotation, round-trip verify; `SaveOutcome` struct. |
| `src-tauri/src/pdf/actor.rs` | `Message::Save` + `dirty` flag + `save`/`save_request` handle methods. |
| `src-tauri/src/commands/save.rs` | `pdf_save` IPC command (thin: look up actor, send, await). |
| `src-tauri/src/commands/mod.rs`, `lib.rs` | Register the new module + command. |
| `src/ipc/save.ts` | `savePdf(id, path?)` wrapper + `SaveOutcome` type. |
| `src/app/use-save.ts` | Cmd/Ctrl+S keydown hook (mirrors `use-file-open.ts`). |
| `src/app/App.tsx` | Save button + merged status toast. |
| `src-tauri/tests/save_noop.rs` | save-as round-trip, true-no-op, `.bak` rotation, + on-demand verify artifact. |
| `src/ipc/__tests__/save.test.ts` | `path ?? null` marshalling for the `Option<String>` arg. |

#### Why no `P2-PAGE-*` spec ID

Save is infrastructure — the step doc carries no `P2-PAGE-*` line. The
governing text is `NFR-PERF-004` (50 MB in <3 s) plus `docs/04_ARCHITECTURE.md`
§ "Saving and auto-save". A candidate EARS line (`P2-SAVE-001`) was drafted in
the plan for the human to optionally add to `02_PRODUCT_SPEC.md`.

#### Further reading

- Atomic file writes / `rename(2)` durability —
  https://man7.org/linux/man-pages/man2/rename.2.html
- `EXDEV` and why temp files go next to their target —
  https://man7.org/linux/man-pages/man2/link.2.html
- pdfium-render `save_to_bytes` / `save_to_file` —
  https://docs.rs/pdfium-render/latest/pdfium_render/document/struct.PdfDocument.html

---

### Bug — thumbnail sidebar ignored dark mode (P1-VIEW-010)

#### Problem

In dark mode the main page view inverted (light pages → dark) but the
thumbnail sidebar stayed bright white. Found by eye in the dev app, not by
any test — the classic "looks wrong only in the real GUI" class.

#### Cause

Dark mode is a single boolean from `useDarkMode()`. `PdfViewer` passed it
to `PageVirtualizer` (which set `canvas.style.filter = DARK_PAGE_FILTER`),
but **never passed it to `ThumbnailPanel`** — the panel had no way to know
the app was in dark mode, so its `<img>` thumbnails rendered true-colour.

#### Fix

- Lifted `DARK_PAGE_FILTER` out of `PageVirtualizer` into a shared
  `src/view/dark-page-filter.ts` so both renderers reference one string
  (they can't drift — the dark-mode test asserts the exact value).
- Threaded `darkMode` down `PdfViewer → ThumbnailPanel → ThumbTile` and
  applied the same filter to each thumbnail `<img>`: `style={darkMode ?
  { filter: DARK_PAGE_FILTER } : undefined}`.

#### Concept — single source of truth for a cross-component constant

When two components must render *identically*, the value they share should
live in one module, not be copy-pasted. Here the regression test pins the
literal (`"invert(1) hue-rotate(180deg)"`) so a future edit to one renderer
that forgets the other fails CI.

#### Files in this fix

| File | Role |
|---|---|
| `src/view/dark-page-filter.ts` | New — the shared `DARK_PAGE_FILTER` constant. |
| `src/view/PageVirtualizer.tsx` | Imports the constant instead of a local copy. |
| `src/panels/ThumbnailPanel.tsx` | New `darkMode` prop; applies the filter to each thumbnail. |
| `src/view/PdfViewer.tsx` | Passes `darkMode` to `ThumbnailPanel`. |
| `src/panels/__tests__/ThumbnailPanel.test.tsx` | +1 test: dark mode applies the filter; light mode does not. |
| `eslint.config.js` | Added `HTMLImageElement` to DOM globals (test cast). |

---

### P2.A3 — Undo/redo stack (session history)

#### Problem

Give every document an undo/redo history (Cmd+Z / Cmd+Shift+Z). The twist:
this is the *infrastructure* step — it lands **before** any actual page
operation exists (rotate/delete are P2.B*). So the challenge is building a
stack that's complete and testable now, with nothing yet to put on it.

#### Concepts learned

- **Command pattern with inverses.** Each edit is an object that knows how
  to *do* itself and returns the edit that *undoes* itself. `apply(target)
  -> inverse`. Undo = apply the inverse (which returns the original, for
  redo). Undo and redo are literally the same code running against opposite
  stacks. No "snapshot the whole document" needed — the inverse of "rotate
  +90°" is "rotate −90°", the inverse of "delete page 3" is "insert <saved
  page> at 3".
- **Generic over the target = testable without the hard dependency.** The
  stack is `UndoStack<T>` and the trait is `Edit<T>`, generic over what's
  being edited. The actor uses `UndoStack<PdfDocument>`, but the tests use
  `UndoStack<i32>` with a trivial "add N / subtract N" edit. This is the
  key trick that let A3 land fully tested before any PdfDocument mutation
  API was touched — the same move as A1's dirty flag and A2's autosave-
  later design. Build the rails now, let later steps lay track on them.
- **Redo is cleared on a new edit.** History is linear, not a tree: once
  you undo and then do something new, the old "redo" future is gone. One
  `self.redo.clear()` in `record()`.
- **Bounded history.** Inverses can hold data (a deleted page's content),
  so the undo stack is a `VecDeque` capped at `MAX_UNDO_DEPTH` — oldest
  actions fall off the front. Redo never needs its own cap (it only ever
  holds what undo already bounded).
- **State lives where the data lives.** The stacks live in the actor's
  worker thread (next to the `PdfDocument` and the dirty flag), because
  PDFium is single-threaded per document. The frontend mirrors only two
  booleans (`canUndo`, `canRedo`) to grey out buttons — it never holds the
  history itself.

#### Why `[~]` and not `[x]`

The step's acceptance ("delete pages 3,5,7 → undo three times → restored")
needs a real `Edit<PdfDocument>`, which arrives with **P2.B2 (delete)**.
A3 ships the machinery; the end-to-end PDF round-trip is proven when the
first concrete edit plugs in. Until then the stack is always empty and
undo/redo are verified no-ops.

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/src/pdf/undo.rs` | `UndoStack<T>` + `Edit<T>` + `HistoryState` + `MAX_UNDO_DEPTH`; 4 mechanics unit tests. |
| `src-tauri/src/pdf/actor.rs` | Holds the stack; `Undo`/`Redo`/`GetHistoryState` messages + handle methods; `doc` is now `mut`. |
| `src-tauri/src/commands/history.rs` | `pdf_undo` / `pdf_redo` / `pdf_history_state` commands. |
| `src/ipc/history.ts` | Wrappers + `HistoryState` type. |
| `src/state/history-store.ts` | Per-document availability mirror + `useDocHistory` selector. |
| `src/app/use-history.ts` | Cmd/Ctrl+Z, Cmd/Ctrl+Shift+Z hook. |
| `src/app/App.tsx` | Undo/Redo toolbar buttons (disabled-state from the store). |
| `src-tauri/tests/undo_redo.rs` | Actor-level: empty history, no-op undo/redo. |
| `src/state/__tests__/history-store.test.ts` | Store action transitions. |
| `docs/04_ARCHITECTURE.md` | New "Undo/redo (session history)" section (pattern doc). |

#### Further reading

- Command pattern (do/undo) — https://refactoring.guru/design-patterns/command
- Memento vs. command for undo — https://refactoring.guru/design-patterns/memento
- `VecDeque` (ring buffer for the capped stack) —
  https://doc.rust-lang.org/std/collections/struct.VecDeque.html

---

### P2.A2 — Auto-save + crash recovery

#### Problem

If the app crashes with unsaved edits, the user shouldn't lose them. Every
30s, write a recovery copy of each *dirty* document somewhere private; on
the next launch, offer to reopen any copy a crash left behind.

#### Concepts learned

- **Side-copy, never the original.** Autosave writes to
  `<app_data_dir>/autosave/<id>.pdf` — Tauri-resolved, never a hardcoded
  path, and **never the user's file**. The user's document is only ever
  touched by an explicit Save.
- **The sidecar pattern.** The `.pdf` alone doesn't say where it came from,
  so each copy gets a `<id>.json` sidecar: `{ documentId, originalPath,
  savedAt }`. Recovery reads sidecars, not PDFs. A `.pdf` with no sidecar
  (or vice-versa) is skipped — robust against partial/orphaned state.
- **Crash detection by *absence of cleanup*.** There's no "am I a crash?"
  flag. Instead: a graceful exit (explicit close, or the mailbox closing
  when the last handle drops) *deletes* the recovery copy; a clean save
  deletes it too. A hard crash runs none of that cleanup, so the copy
  *survives* — and surviving copies are exactly what we offer to recover.
  Recovery = "what cleanup didn't get to."
- **A timer without `tokio::time`.** Our tokio build doesn't enable the
  `time` feature, and rather than add it, the 30s tick is a dedicated std
  thread that `sleep`s and pokes each actor. The poke is **fire-and-forget**
  (a `Message::Autosave` with no reply) — the actor writes its copy as a
  side effect and logs; the tick never waits.
- **The actor owns the write.** PDFium is single-threaded per document, so
  the autosave (like the save and the render) happens on the actor's own
  thread, not in the tick thread or an async handler. The tick only *pokes*.
- **Dormant rails again.** Nothing dirties a document in A2 (edits are
  B-steps), so the live loop writes nothing yet. Same pattern as A1's dirty
  flag and A3's empty stack: build + unit-test the mechanism now, let B2
  light it up. The functions (write/scan/discard) are tested directly.

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/src/pdf/autosave.rs` | `write_autosave` / `scan_autosaves` / `discard_autosave`, `RecoveryEntry`, the sidecar, `spawn_autosave_tick`. |
| `src-tauri/src/pdf/actor.rs` | `Message::Autosave` (write iff dirty); discard on clean save + graceful exit; `poke_autosave`. |
| `src-tauri/src/commands/recovery.rs` | `recovery_list` / `recovery_discard`. |
| `src-tauri/src/lib.rs` | Spawn the tick in `setup`; register the commands. |
| `src/ipc/recovery.ts` | Wrappers + `RecoveryEntry`. |
| `src/app/use-recovery.ts` | Once-per-launch scan (module-guarded); recover/discard. |
| `src/app/RecoveryDialog.tsx` | Startup "Recover unsaved changes?" prompt. |
| `src/app/App.tsx` | Mounts the hook + dialog. |
| `src-tauri/tests/autosave.rs` | write→scan round-trip, discard idempotent, skip orphaned/malformed, missing-dir empty. |
| `src/app/__tests__/use-recovery.test.tsx` | Hook surfaces entries; recover opens+drops; discard drops only. |
| `docs/04_ARCHITECTURE.md` | Expanded the auto-save section (sidecar, tick thread, cleanup). |

#### Further reading

- Tauri path resolver (`app_data_dir`) —
  https://v2.tauri.app/reference/javascript/api/namespacepath/
- Crash-only software (the cleanup-vs-crash idea) —
  https://en.wikipedia.org/wiki/Crash-only_software
- Atomic file replacement via rename — https://man7.org/linux/man-pages/man2/rename.2.html

---

### P2.B1 — Rotate page(s)

#### Problem

Rotate a page by 90/180/270° and have it *persist* in the PDF (not just a
viewer trick). This is the **first real edit** — the moment the undo stack
(A3), dirty flag (A1), and autosave (A2) all stop being dormant rails and
carry real traffic.

#### Concepts learned

- **Rotation is metadata, not pixels.** A PDF page has a `/Rotate` entry
  (0/90/180/270). Rotating sets that integer; no content stream is
  rewritten. Every reader honours it. `PdfPage::set_rotation` →
  `FPDFPage_SetRotation` → `/Rotate`. (Per the spec, this is the *only*
  correct rotation — a viewer-only transform wouldn't survive save.)
- **The first `Edit<PdfDocument>`.** `RotateEdit` implements the A3 trait:
  `apply` rotates and returns its inverse (`-quarter_turns`). Because
  rotation is additive mod 4, the inverse is exact without remembering each
  page's prior angle. The actor's `RotatePages` handler does the
  three-line dance the whole undo system was built for: `apply` → record
  the inverse → mark dirty.
- **Atomic-ish edits.** A bad page index is validated *before* any page is
  mutated, so a failure can't leave the document half-rotated with no undo
  entry (a partial edit you can't take back is worse than no edit).
- **PDFium is not thread-safe across documents** (the big one — see below).
- **Two PDF engines, two refresh stories.** Thumbnails render through
  PDFium (the actor), so re-rendering one reflects an edit immediately; the
  main view renders through *PDF.js from disk*, so it only reflects edits
  after save/reopen. B1 refreshes the thumbnail (cache-invalidate + a
  per-page "version" token that re-keys the tile's load effect); the
  main-view live preview is a deferred shared pipeline (BACKLOG).

#### The concurrency bug B1 surfaced

Rotating under `cargo test`'s parallel runner went `SIGABRT`, then
`SIGSEGV`. Root cause: PDFium has process-global state and is unsafe even
across *different* documents — page lookup, save, and `FPDF_CloseDocument`
(a `PdfDocument`'s `Drop`) from two threads corrupt it. A render-only lock
already existed; B1 forced the generalisation:

- One **process-global `PDFIUM_LOCK`** now serializes *every* PDFium FFI
  span (load/save/metadata/rotate/render), held around the minimal span and
  never across a re-locking call (the `Mutex` isn't reentrant — `open_pdf`
  and `save_document` release before paths that re-lock).
- The actor **closes its document under the lock** (Drop races too).
- **Tests run single-threaded** (`--test-threads=1` in the cargo wrapper):
  they open/drop their own documents, which can't take the crate-private
  lock. (Test *binaries* are separate processes, so they stay parallel.)

Lesson: a "thread-safe per X" library often still has global state; assume
*everything* through the FFI needs one lock until proven otherwise.

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/src/pdf/rotate.rs` | `RotateEdit: Edit<PdfDocument>` + rotation arithmetic. |
| `src-tauri/src/pdf/document.rs` | The shared `PDFIUM_LOCK` + `pdfium_lock()`; lock load/save/metadata. |
| `src-tauri/src/pdf/render.rs` | Use the shared lock (was a private render-only one). |
| `src-tauri/src/pdf/actor.rs` | `RotatePages` message; close the doc under the lock. |
| `src-tauri/src/commands/pdf.rs` | `pdf_rotate_pages` (degrees → quarter-turns). |
| `src/ipc/rotate.ts` | `rotatePages` wrapper. |
| `src/tools/rotate/RotateMenu.tsx` | The right-click rotate menu. |
| `src/panels/ThumbnailPanel.tsx` | Context menu + per-page refresh token + Undo sync. |
| `src/panels/thumbnail-cache.ts` | `deleteThumb` (invalidate on edit). |
| `scripts/cargo-test.mjs` | Run the PDFium tests single-threaded. |
| `src-tauri/tests/rotate.rs` | persist/undo/redo/atomic-on-error + a rotated artifact. |

#### Further reading

- PDF `/Rotate` (PDF 32000-1, §7.7.3.3 page object) — any PDF reference.
- PDFium thread-safety notes — https://pdfium.googlesource.com/pdfium/
- Command-pattern undo (recap) — https://refactoring.guru/design-patterns/command

---

### Edit-preview pipeline (live view refresh on edits)

#### Problem

We have *two* PDF engines: PDFium (the actor, owns edits) and PDF.js (the
view, renders from a separate copy of the bytes). After B1, rotating
mutated the PDFium document but the main view kept showing the old PDF.js
copy — the edit only appeared after save + reopen. Needed: any edit (and
undo/redo) shows live, in both the main view and the thumbnails, for *all*
current and future edit types.

#### Concepts learned

- **A signal, not a data flow.** Rather than plumbing edit details to the
  view, there's one per-document **edit epoch** (a counter). Every
  successful edit/undo/redo bumps it; the view and thumbnails subscribe and
  react. Decoupled: the view doesn't care *what* changed, only *that*
  something did. (And because edits are frontend-initiated, no backend
  event is needed — the caller bumps the epoch.)
- **Reload from the live source, not the disk.** The view used to load
  bytes from the file on disk. Now: epoch 0 → load from disk; epoch > 0 →
  load from `pdf_get_bytes` (the actor's in-memory document via
  `save_to_bytes`). So the view reflects *unsaved* edits. The actor still
  owns every byte — the frontend only *reads* the current state, keeping
  the "PDF.js never writes" rule intact.
- **Swap without a blank.** Naively, reloading sets `doc = null` (blank) then
  loads the new one. Instead: load the new document fully, *then* swap it in
  and destroy the old one. The user never sees an empty frame. Same idea as
  the atomic temp+rename save: stage the new thing, commit, discard the old.
- **Preserve the cursor.** A reload would jump scroll to page 1. We capture
  `getCurrentPage()` before and `scrollToPage()` after, but only for a
  *same-document* reload (an edit), not a tab switch — tracked with a
  "last documentId" ref.
- **One signal, two consumers, free generality.** The thumbnails dropped
  their B1 per-page refresh token and now key off the same epoch. Bonus:
  undo/redo refresh thumbnails too (a B1 gap), and delete/insert (which
  shift every page index) will Just Work — a doc-level signal is the right
  granularity for them.

#### Cost (and the deferred fix)

This re-parses + re-renders the *whole* document on every edit, and ships
the full bytes over IPC as a `number[]`. Correct and uniform, but not
cheap for large PDFs. The optimization (re-render only affected pages; a
rotate-only viewport-rotation fast path; raw-bytes IPC) is deferred to
BACKLOG — build the correct rail first, make it fast when a real PDF feels
slow.

#### Files in this step

| File | Role |
|---|---|
| `src/state/edit-epoch-store.ts` | The per-doc epoch + `bumpEpoch` + `useDocEpoch`. |
| `src-tauri/src/pdf/actor.rs` | `Message::GetBytes` (serialize live doc under the lock). |
| `src-tauri/src/commands/pdf.rs` | `pdf_get_bytes`. |
| `src/ipc/pdf.ts` | `getPdfBytes` wrapper. |
| `src/view/PdfViewer.tsx` | Reload-from-actor-bytes on epoch bump; no-blank swap; page restore. |
| `src/panels/ThumbnailPanel.tsx` | Thumbnails key off the epoch; rotate bumps it. |
| `src/app/use-history.ts` | undo/redo bump the epoch. |
| `src-tauri/tests/get_bytes.rs` | The live bytes carry an unsaved rotation. |
| `src/state/__tests__/edit-epoch-store.test.ts` | Bump/independence. |
| `docs/04_ARCHITECTURE.md` | The pipeline. |

#### Further reading

- PDF.js `getDocument({ data })` — https://mozilla.github.io/pdf.js/api/
- Double-buffering / atomic swap (the no-blank idea) —
  https://en.wikipedia.org/wiki/Multiple_buffering

---

### P2.B2 — Delete page(s)

#### Problem

Delete pages, renumber, keep internal references correct, and make it
undoable. The hard parts: undo must restore the *content* of a deleted
page (not just a blank), and the spec says "update internal references."

#### Concepts learned

- **PDF references are object refs, not indices — the key realization.** A
  link/bookmark/destination points to a page *object* (`5 0 R`), not "page
  3." So deleting page 2 and renumbering doesn't break a reference to page
  3 — it still points to the same object, which is now page 2. "Update
  internal references" mostly **falls out for free**; we verified it with a
  fixture (`links.pdf`) whose page-1 link survives a deletion and tracks
  page 3 to its new index. *What you can't fix* is references *to* the
  deleted page (dangling) — and pdfium-render's outline/link API is
  read-only, so that cleanup (and reorder's full rewrite) waits on a
  dict-level library (`lopdf`) — BACKLOG.
- **Undo by stashing, not snapshotting.** `DeleteEdit`'s inverse can't just
  remember indices — it must remember *content*. So `apply` copies the
  doomed pages into a fresh holding `PdfDocument` (`create_new_pdf` +
  `copy_pages_from_document` / `FPDF_ImportPages`), serializes it to bytes,
  and stores those bytes in the inverse `RestorePagesEdit`. Undo loads the
  holding doc and re-imports the pages. Bounded (bytes ride the undo stack,
  capped at `MAX_UNDO_DEPTH`).
- **Order matters twice.** Delete **descending** (so removing page 1 doesn't
  shift the index of page 3 you're about to remove). Re-insert **ascending**
  at the original target indices (each insert only shifts still-pending,
  larger targets). Both directions are tested.
- **Validate before you mutate.** Sort + de-dup + range-check the whole
  index list up front, so a bad index in a batch fails atomically rather
  than half-deleting.
- **Free inheritance.** Because delete is just another `Edit<PdfDocument>`
  routed through the actor's message + epoch bump, it got live preview
  (pipeline), undo/redo (A3), and autosave/crash-recovery (A2) with no new
  wiring. The rails paid off.

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/src/pdf/delete_page.rs` | `DeleteEdit` + content-preserving `RestorePagesEdit`; validate + range-string helpers (+ unit tests). |
| `src-tauri/src/pdf/actor.rs` | `DeletePages` message + handle methods. |
| `src-tauri/src/commands/pdf.rs` | `pdf_delete_pages`. |
| `src/ipc/delete-pages.ts` | `deletePages` wrapper. |
| `src/tools/rotate/RotateMenu.tsx` | "Delete page" menu item. |
| `src/panels/ThumbnailPanel.tsx` | Delete handler + Delete/Backspace key on a focused tile. |
| `tests/fixtures/basic/links.pdf` (+ generator) | 3-page fixture with a page-1→page-3 link. |
| `src-tauri/tests/delete_page.rs` | count/undo-order/redo/**reference-integrity**/atomic-error + artifact. |
| `src/ipc/__tests__/delete-pages.test.ts` | Marshalling. |

#### Further reading

- PDF page tree + indirect references (PDF 32000-1 §7.7.3) — any PDF reference.
- `FPDF_ImportPages` (page copy between documents) — PDFium docs.
- COS/dict access for reference rewriting — https://docs.rs/lopdf (the deferred path).

---

### Bug fixes — viewer regressions from the edit-preview pipeline (+ pinch zoom)

Three issues surfaced the moment a human drove the real app — none caught by
tests (the page virtualizer needs a real DOM + canvas + IntersectionObserver
+ PDF.js, so it has no unit coverage).

#### 1. Switching documents kept showing the old page

The edit-preview pipeline's "no-blank swap" stopped nulling `doc` on a
document switch, so `PageVirtualizer` (rendered only `{doc ? … }`, not
keyed) stayed **mounted** across the switch and kept the previous
document's pages — made worse by React **StrictMode** double-invoking the
load effect. Fix: on a *switch* (documentId changed) `setDoc(null)` first
so the virtualizer unmounts and remounts clean; an *edit reload* (same
documentId, bumped epoch) still skips it to stay blank-free. **Lesson:** an
in-place optimization quietly relied on the old unmount/remount to reset
child state; keep the reset where state must be fresh.

#### 2. Trackpad pinch / Ctrl+wheel didn't zoom

There was never a wheel handler — zoom was toolbar-only. macOS delivers a
trackpad pinch as a **Ctrl+wheel** event, so a non-passive `wheel` listener
on the scroll container (so we can `preventDefault` the webview's own page
zoom) maps it to `setZoom(scale × exp(-deltaY·0.01))`. The exponential
factor makes equal in/out gestures round-trip to the same scale.

#### 3. Rotate 180° updated only the thumbnail

The page-render LRU is keyed `documentId:page:scale:dpr:dark`. A 90° rotate
swaps width↔height (so scale/layout shifts → new key → re-render), but a
**180° rotate leaves dimensions unchanged** → identical key → a *stale*
cached canvas was served. Fix: put the **edit epoch in the cache key**, so
any edit invalidates the cache by construction — immune to React's
child-before-parent effect ordering (a parent "clear the cache" effect runs
*after* the child slot already read it). **Lesson:** invalidate caches by
*key*, not by a separate clear effect, when a child consumes the cache.

#### Files

`src/view/PdfViewer.tsx` (switch reset, pass `epoch`/`onZoom`),
`src/view/PageVirtualizer.tsx` (epoch in cache key, wheel-zoom listener),
`eslint.config.js` (`WheelEvent` global). GUI-verified — no unit coverage
for the virtualizer.

---

### P2.B3 — Insert blank page

#### Problem

Insert a blank page at a position, undoably, inheriting the neighbour's
size and orientation. After rotate (B1) and delete (B2), this is the third
page edit — and the shortest, because it leans on what's already there.

#### Concepts learned

- **The inverse of an insert is a delete — so reuse it.** `InsertBlankEdit`
  doesn't define its own undo logic; `apply` inserts the page and returns a
  `DeleteEdit { pages: [index] }` (the B2 type). Undo runs that delete
  (which itself returns a re-insert for redo). Two edits compose into a
  full undo/redo cycle with almost no new code — the payoff of the generic
  `Edit<T>` design from A3.
- **Orientation = dimensions.** A page is "landscape" because it's wider
  than tall — there's no separate orientation flag. So inheriting the
  neighbour's width/height (via `create_page_at_index` with a `Custom`
  paper size) satisfies the spec's "size *and* orientation" in one step.
- **Insert can't use the rotate fast-path.** Rotate previews cosmetically
  (a `getViewport` rotation) because the page set is unchanged. Insert
  changes the page *count*, so the main view must actually reload (bump the
  epoch, like delete). Cosmetic tricks only work when the page tree's shape
  is stable.
- **Append is index == count.** Unlike delete (valid range `0..count`),
  insert's valid range is `0..=count` — inserting *at* the end appends.
  Off-by-one in the bound check is a real bug class; tested at both ends.
- **A cached value quietly went stale.** The actor's `GetPageCount` returns
  the count captured at *open*, so a test that read it after an insert saw
  the old number. Fix in the test (use the live `GetMetadata` re-read);
  logged the real cache-staleness as a backlog trap.

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/src/pdf/insert_blank.rs` | `InsertBlankEdit` (`Edit<PdfDocument>`) + adjacent-page size inheritance; inverse = `DeleteEdit`. |
| `src-tauri/src/pdf/actor.rs` | `InsertBlankPage` message + handle methods. |
| `src-tauri/src/commands/pdf.rs` | `pdf_insert_blank_page` (optional `width`/`height`). |
| `src/ipc/insert-blank.ts` | `insertBlankPage` wrapper. |
| `src/tools/rotate/RotateMenu.tsx`, `src/panels/ThumbnailPanel.tsx` | "Insert blank page after" + handler (full-reload preview). |
| `src-tauri/tests/insert_blank.rs` | count/inherit-dims/undo-redo/prepend-append/out-of-range + artifact. |
| `src/ipc/__tests__/insert-blank.test.ts` | Marshalling. |

#### Further reading

- `FPDFPage_New` (create a blank page) — PDFium docs.
- PDF page boxes / MediaBox (size = the page rectangle) — PDF 32000-1 §14.11.2.

---

### P2.B4 — Crop page (CropBox only)

#### Problem

Crop a page without throwing away content — so it's reversible — and let
undo and "reset" both work.

#### Concepts learned

- **Crop is a window, not a cut.** A page has a `/MediaBox` (the real
  paper) and a `/CropBox` (the visible window). Cropping just shrinks the
  `/CropBox`; the content streams are untouched, so nothing is destroyed and
  resetting `/CropBox` back to `/MediaBox` restores the full page. Both
  PDFium and PDF.js render the CropBox region, so the preview reflects it.
- **The inverse must capture the *old* box.** Unlike rotate (inverse = a
  fixed −angle) or insert (inverse = delete), crop's inverse is "restore
  whatever the box was before." So `apply` reads the current box, sets the
  new one, and returns a `CropEdit` carrying the old rectangle. "Reset
  crop" is just a crop to the MediaBox — and it's undoable for free because
  it captures the pre-reset box the same way.
- **A defaulted value can still error.** `boundaries().crop()` *errors*
  (not "returns the MediaBox") when a page has no explicit `/CropBox` —
  found via a test that exploded on the fixture. The effective box is the
  MediaBox in that case, so the fix is `crop().or_else(|_| media())`.
  Lesson: don't assume a "get" returns a sensible default; check.
- **Coordinates: bottom-left origin + absolute space.** PDF rectangles are
  (left, bottom, right, top) with y *up* from the bottom. The crop dialog
  collects edge margins and converts to an absolute box, offset by the
  page's current box origin (`page.view` from PDF.js) so it's correct even
  for an already-cropped page.
- **A full overlay isn't required to ship.** A margins dialog satisfies the
  spec ("when the user crops a page…") with far less risk than a
  drag-select overlay on the viewer (which we'd just been debugging).
  Drag-select is deferred — the backend already takes any rectangle.

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/src/pdf/crop.rs` | `CropEdit` (`Edit<PdfDocument>`): set `/CropBox`, reset-to-MediaBox, capture-and-restore inverse. |
| `src-tauri/src/pdf/actor.rs` | `CropPage` message + handle methods. |
| `src-tauri/src/commands/pdf.rs` | `pdf_crop_page` (all-four-edges = crop, all-none = reset). |
| `src/ipc/crop.ts` | `cropPage` wrapper. |
| `src/tools/crop/CropDialog.tsx` | Margin-input dialog (+ reset). |
| `src/tools/rotate/RotateMenu.tsx`, `src/panels/ThumbnailPanel.tsx` | "Crop page…" entry + handlers (reads `page.view`). |
| `src-tauri/tests/crop.rs` | set/persist, reset, undo, out-of-range, inverted-rect + artifact. |
| `src/ipc/__tests__/crop.test.ts` | Marshalling (rect vs reset). |

#### Further reading

- PDF boundary boxes (MediaBox/CropBox) — PDF 32000-1 §14.11.2.
- `FPDFPage_GetCropBox` / `FPDFPage_SetCropBox` — PDFium docs.

---

### P2.C2 — Extract pages to a new PDF

#### Problem

Produce a new PDF containing exactly the chosen pages, with their resources
(fonts/images) intact — the first Track-C feature, and the first that
*writes a different file* rather than editing the open one.

#### Concepts learned

- **Not every operation is an `Edit`.** Rotate/delete/insert/crop mutate the
  open document and live on the undo stack. Extract is *read-only* on the
  source — it builds and saves a *new* file. So it skips the whole
  `Edit<PdfDocument>` / undo / dirty / epoch machinery; it's just "read
  these pages, write a new doc." Recognising what *doesn't* need the
  framework keeps it simple.
- **Reuse beats reinvention — twice over.** The output is built with the
  exact `create_new_pdf()` + `copy_pages_from_document()` (FPDF_ImportPages)
  pair from delete's undo holding-doc, and written with A1's
  `save_document` (atomic temp+rename + round-trip verify). FPDF_ImportPages
  copies each page's referenced resources, which is what satisfies the
  spec's "resources copied" — for free.
- **Lock discipline across a reused helper.** `extract_pages` builds the new
  doc *under* the PDFium lock, then *releases* it before calling
  `save_document` (which re-locks to serialize + verify) — the lock isn't
  reentrant. Same rule as save's own internal staging.
- **Validate the input where it's cheap, but trust the backend.** The
  dialog's page-range parser (`parsePageRange`) is pure + unit-tested and
  validates against the live count; the actor re-validates against its own
  live document, so a stale frontend count can't corrupt the output.
- **Document-level UI ≠ page-level UI.** Rotate/delete/etc. hang off the
  per-page thumbnail menu; extract is a *document* action, so it's a viewer-
  toolbar button → a range dialog → the native save dialog.

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/src/pdf/extract.rs` | `extract_pages` — new doc via import + `save_document`. |
| `src-tauri/src/pdf/delete_page.rs` | `validate` / `range_string` promoted to `pub(crate)` (shared). |
| `src-tauri/src/pdf/actor.rs` | `ExtractPages` message (read-only; no undo/dirty). |
| `src-tauri/src/commands/pdf.rs` | `pdf_extract_pages`. |
| `src/ipc/extract.ts` | `extractPages` wrapper. |
| `src/tools/extract/page-range.ts` | `parsePageRange` ("1-3,5" → 0-based indices). |
| `src/app/ExtractDialog.tsx`, `src/app/ZoomToolbar.tsx`, `src/view/PdfViewer.tsx` | Range dialog + toolbar "Extract…" + native save dialog. |
| `src-tauri/tests/extract.rs`, `src/tools/extract/__tests__/page-range.test.ts`, `src/ipc/__tests__/extract.test.ts` | Tests. |

#### Further reading

- `FPDF_ImportPages` — copying pages + resources between documents.
- Tauri save dialog (`@tauri-apps/plugin-dialog`) — https://v2.tauri.app/plugin/dialog/

---

### P2.C3 — Split (4 modes)

#### Problem

Acrobat lets you break one PDF into many. The spec (P2-PAGE-007) wants four
ways to choose the cut points: every N pages, at specific pages, by file-size
target, and by top-level bookmarks. Each produces N separate files.

#### Concepts learned

- **One algorithm, four front-ends.** Every mode collapses to the same
  shape: *a list of contiguous page groups*. `plan_groups` turns the mode
  into `Vec<Vec<i32>>`; the writer loop is mode-agnostic. New requirements
  ("split by chapter") become a new `plan_groups` arm, nothing else.
- **Reuse the verified writer.** Splitting is "extract, N times." Pulling
  `write_subset_pdf` out of C2 means every output file gets the same atomic
  temp+rename + round-trip-reopen guarantee for free — no second write path
  to keep correct.
- **No size oracle → measure by serializing.** PDFium won't tell you how big
  an output will be without building it. By-size mode therefore *probes*:
  grow a chunk a page at a time, `save_to_bytes()`, compare. It's O(n²) and
  approximate (shared resources compress unpredictably) — a deliberate,
  documented trade for a one-shot operation. A lone page bigger than the
  target still gets its own file: never drop a page.
- **Reading the outline needs no dict library.** Splitting by bookmarks only
  *reads* where each top-level bookmark points (`bookmark.destination()?.
  page_index()`), so PDFium's read-only outline API is enough. This is the
  opposite of *rewriting* references (reorder / dangling-ref cleanup), which
  still needs lopdf. Read vs. write is the whole distinction.
- **`root()` is the first bookmark, not a synthetic parent.** In
  pdfium-render, `bookmarks().root()` returns the first top-level item
  (`FPDFBookmark_GetFirstChild(doc, NULL)`); walk `next_sibling()` to visit
  the rest. There is no node above the top level.
- **A degenerate split is an error, not a silent copy.** If a mode yields
  fewer than two groups (e.g. "every 10 pages" on a 3-page doc), the actor
  returns an error rather than writing one file — clearer feedback, and it
  keeps the by-size "huge target" case honest.
- **N output files → pick a directory, not a file.** Unlike extract (one
  file → `save` dialog), split needs `open({ directory: true })`; the backend
  names files `{stem}-NNN.pdf` (zero-padded so they sort lexically).

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/src/pdf/split.rs` | `SplitMode`/`SplitOutcome`; `split_document`; `plan_groups` (the four-mode boundary math) + `probe_chunk_size`. |
| `src-tauri/src/pdf/extract.rs` | Refactor: shared `write_subset_pdf` (used by extract + every split chunk). |
| `src-tauri/src/pdf/actor.rs` | `SplitDocument` message (read-only; no undo/dirty). |
| `src-tauri/src/commands/pdf.rs` | `pdf_split_document`. |
| `src/ipc/split.ts` | `splitDocument` wrapper + `SplitMode`/`SplitOutcome` types. |
| `src/tools/split/split-points.ts` | `parseSplitPoints` (mode-b "split before pages"). |
| `src/app/SplitDialog.tsx`, `src/app/ZoomToolbar.tsx`, `src/view/PdfViewer.tsx` | Mode-picker dialog + toolbar "Split…" + native directory picker. |
| `tests/fixtures/basic/bookmarks.pdf` (+ `generate-bookmarks.py`) | 6 pp, 3 top-level bookmarks → pages 0/2/4 (mode-d fixture). |
| `src-tauri/tests/split.rs`, `src/tools/split/__tests__/split-points.test.ts`, `src/ipc/__tests__/split.test.ts` | Tests. |

#### Further reading

- `FPDFBookmark_GetFirstChild` / `FPDFDest_GetDestPageIndex` — reading the
  outline tree and resolving a bookmark to a page index.
- `FPDF_SaveAsCopy` (via `save_to_bytes`) — serializing a document to measure
  or persist it.
- Tauri dialog `open({ directory: true })` — https://v2.tauri.app/plugin/dialog/

---

### P2.C4 — Merge (basic: concat + annotations)

#### Problem

Combine several PDFs into one. Spec P2-PAGE-008 wants annotations, form
fields, AND bookmarks preserved, with colliding form-field names made unique.
We can do the page/annotation part with PDFium today; the rest needs a
dict-level library, so this ships as an explicit **partial**.

#### Concepts learned

- **Know what your primitive actually copies.** `FPDF_ImportPages` (our only
  merge tool) copies pages, their resources, and their *page-level
  annotations* — but **not** the document `/Outlines` (bookmarks) or the
  `/AcroForm` (interactive form fields), and it can't rename anything. So
  "merge" splits cleanly into what PDFium can do (concat + annotations) and
  what needs lopdf (bookmarks, form fields, collision renaming). Naming that
  boundary up front turned a vague feature into a shippable slice + a tracked
  follow-up.
- **Ship a partial honestly, and *lock the gap with a test*.**
  `merge_does_not_yet_carry_bookmarks` asserts the merged file has *no*
  bookmarks. That feels backwards (testing a missing feature), but it's a
  tripwire: when the lopdf follow-up adds bookmark preservation, that test
  fails on purpose, forcing whoever lands it to notice and update. A deferred
  requirement with a failing-when-fixed test can't be silently forgotten.
- **Not every command fits the actor.** The architecture says "every command
  takes a `DocumentId` and routes to that document's actor." Merge reads N
  files that needn't be open and writes a new one — it owns no document. So it
  became a **standalone command**, documented in `docs/04` as a "stateless
  multi-file operation." It still honors the two real invariants: the frontend
  never writes bytes (Rust does, via `save_document`), and all PDFium FFI is
  serialized under `PDFIUM_LOCK`. The rule existed to enforce *those*; the
  `DocumentId` was the mechanism, not the point.
- **Blocking work belongs on a blocking thread.** Actor edits run on the
  actor's own OS thread, off the async runtime. A standalone async command
  doing heavy FFI (open N docs, import, serialize) would instead stall a tokio
  worker — so the body runs inside `tokio::task::spawn_blocking`. Paths are
  `Send`; the `PdfDocument`s are created and consumed *inside* the closure, so
  nothing un-`Send` crosses the thread boundary.
- **DRY the write spine, not the orchestration.** Extract, split, and now
  merge all end in the same verified `save_document` (atomic temp+rename +
  round-trip reopen). The differences are only *which pages from where* —
  extract: one open doc; split: one doc, N groups; merge: N files, all pages.
- **Stable references for "seed once" props.** `MergeDialog` resets its list
  from `initialPaths` when it opens; passing `initialPaths={[path]}` (a fresh
  array each render) would reset it on *every* render and eat the user's edits.
  `useMemo(() => [path], [path])` makes the reference stable so the reset fires
  only when the dialog opens or the file actually changes.

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/src/pdf/merge.rs` | `merge_documents(sources, dest)` — open N, append all pages, verified save. |
| `src-tauri/src/commands/pdf.rs` | `pdf_merge_documents` — standalone command, `spawn_blocking`. |
| `docs/04_ARCHITECTURE.md` | New "Stateless multi-file operations" subsection (the architecture note). |
| `src/ipc/merge.ts` | `mergeDocuments` wrapper. |
| `src/tools/merge/reorder.ts` | Pure `moveUp`/`moveDown`/`removeAt` for the file list. |
| `src/app/MergeDialog.tsx`, `src/app/ZoomToolbar.tsx`, `src/view/PdfViewer.tsx` | Ordered-list dialog + toolbar "Merge…" + native pickers. |
| `src-tauri/tests/merge.rs`, `src/tools/merge/__tests__/reorder.test.ts`, `src/ipc/__tests__/merge.test.ts` | Tests (incl. the deferred-bookmark tripwire). |

#### Further reading

- `FPDF_ImportPages` — what it copies (pages, resources, annots) and what it
  doesn't (outline, AcroForm).
- `tokio::task::spawn_blocking` — running blocking work without starving the
  async runtime: https://docs.rs/tokio/latest/tokio/task/fn.spawn_blocking.html

---

### P2.D1 — Insert pages from another PDF (partial)

#### Problem

Pull pages out of one PDF and drop them into the document you're editing, at a
spot you choose — undoably. Spec P2-PAGE-005 also wants form fields preserved;
PDFium can't do that part, so this ships content + annotations + dimensions and
defers form fields.

#### Concepts learned

- **The import primitive, pointed inward.** Extract/split/merge used
  `copy_pages_from_document` to build *new* files. D1 aims it at the *open*
  document — same call, but now it's a mutation, so it becomes an `Edit`. The
  reusable verb didn't change; where it writes did.
- **Compose the inverse from edits you already have.** `InsertFromEdit`'s
  inverse is just `DeleteEdit` over the block it inserted — the exact pattern
  insert-blank uses. And because `DeleteEdit` stashes the removed pages' bytes
  in a holding doc, *redo* restores the imported content even if the user has
  since moved or deleted the source file. The undo system's primitives compose:
  insert⁻¹ = delete, delete⁻¹ = restore-bytes. No new inverse logic to write.
- **Open before you lock; close under the lock.** `apply` runs on the actor
  thread. It must open the source *without* holding `PDFIUM_LOCK` (because
  `open_pdf` locks internally and the lock isn't reentrant), then take the lock
  to copy, then take it again to `drop(source)` — because `Drop` calls
  `FPDF_CloseDocument` and an unlocked close can race other PDFium threads.
  Getting drop-ordering right matters: a guard declared after the source would
  release *before* the source dropped, leaving the close unlocked. (This also
  surfaced a latent gap — transient docs in extract/split/merge drop unlocked
  too; logged to BACKLOG.)
- **A tiny read-only command beats a heavy one.** The dialog needs the source's
  page count to validate the range. Re-using `pdf_open` would spawn a whole
  actor and register a document. Instead, `pdf_peek_page_count` is a standalone
  read-only op (wraps `open_document_metadata`, opens+reads+drops). `DocumentMetadata`
  isn't `Serialize`, so rather than change it, the command returns just the
  `u32` the caller actually needs.
- **`bumpEpoch` already implies "edited".** A content change must reload PDF.js
  from the actor's live bytes, not disk. `bumpEpoch` both increments the reload
  counter *and* sets the `edited` flag (which flips the loader's byte source),
  so insert-from is the same one-liner as delete/insert-blank — no separate
  `markEdited`. (Rotate is the exception: cosmetic preview, so it `markEdited`s
  without bumping.)

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/src/pdf/insert_from.rs` | `InsertFromEdit` — open source, copy into open doc, inverse = `DeleteEdit`; source closed under the lock. |
| `src-tauri/src/pdf/actor.rs` | `InsertFromPdf` message + handle methods + worker arm (records inverse, dirty). |
| `src-tauri/src/commands/pdf.rs` | `pdf_insert_from_pdf` (actor edit) + `pdf_peek_page_count` (standalone read). |
| `src/ipc/insert-from.ts`, `src/ipc/peek.ts` | Typed wrappers. |
| `src/app/InsertFromDialog.tsx`, `src/app/ZoomToolbar.tsx`, `src/view/PdfViewer.tsx` | File picker + range (reuses `parsePageRange`) + position; toolbar "Insert PDF…"; epoch+history wiring. |
| `src-tauri/tests/insert_from.rs`, `src/ipc/__tests__/{insert-from,peek}.test.ts` | Tests (count+undo/redo, annotation survival, validation, missing source, peek). |

#### Further reading

- `FPDF_ImportPages` into an existing document vs. a fresh one — same call, the
  destination handle differs.
- Drop order in Rust (reverse declaration order) — why the close-under-lock
  guard must outlive the document it closes: https://doc.rust-lang.org/reference/destructors.html

---

### lopdf adoption — a second PDF library (COS layer)

#### Problem

Four spec clauses were stuck because PDFium's binding is read-only for the
bits that live in the PDF's *object dictionaries*: bookmarks (`/Outlines`),
form fields (`/AcroForm`), the page tree, and indirect references. Reorder,
merge-bookmarks, merge/insert-form-fields, and dangling-ref cleanup all need
to *rewrite* those. We needed a tool that can.

#### Concepts learned

- **A PDF is two things at once: a render target and an object graph.** PDFium
  is superb at the first (it's Chrome's engine) and deliberately hides the
  second. `lopdf` is the opposite — it's a read/write model of the **COS**
  (Carousel Object System: the `obj`/`endobj` dictionaries, arrays, refs, and
  the xref table). Picking the right tool means knowing which *layer* a problem
  lives on. "Preserve bookmarks on merge" is an object-graph problem, so it's a
  COS-library job, not a render-engine job.
- **"Don't add a competing engine" ≠ "never add a second library."** The rule
  exists to stop *redundant* engines (two renderers, two content editors). A
  library that does only what the first one *can't*, with zero overlap, is
  complementary, not competing. The discipline is to write down *why* the new
  thing isn't redundant — that's what the `docs/03` review gate forced.
- **Make two libraries cooperate by handing off bytes, not handles.** The whole
  integration is: PDFium → `save_to_bytes` → lopdf loads/edits/serializes →
  PDFium reopens. Neither library ever sees the other's live state. That one
  decision erases a whole class of nightmares — no shared FFI handle, no
  cross-library locking, no "who owns this pointer." Pure byte buffers compose.
- **Trust, but verify across the boundary.** The real risk of two serializers
  is that one writes bytes the other mangles or rejects. The mitigation isn't
  hope — it's a hard rule: *every lopdf output is reopened in PDFium before it's
  persisted*, and the spike tests assert it (we even checked a third engine,
  Ghostscript). A "no silent breakage" constraint becomes a concrete gate.
- **De-risk a one-way door with a throwaway spike.** Adding a dependency is hard
  to undo. So this step shipped *only* the decision + a capability spike
  (`cos.rs` + tests) wired to nothing — proving outline-write and form-rename
  round-trip before any feature leans on them. If the round-trip had failed,
  we'd have spent a day and walked away. Front-load the scary risk; defer the
  cheap wiring.
- **Trim a dependency's surface.** `default-features = false` dropped lopdf's
  `chrono`/`jiff`/`time`/`rayon` (we need neither dates nor parallelism), and a
  license audit confirmed the whole transitive tree (RustCrypto, `flate2`,
  `nom`, …) is permissive — no GPL. A dependency is a liability you can shrink.
- **lopdf API texture:** `Document::load_mem(&[u8])` / `save_to(&mut Vec<u8>)`;
  `catalog()`/`catalog_mut()`; `get_dictionary(_mut)(id)`; `add_object`;
  `get_pages()` (1-based → `ObjectId`). Gotchas: `From<&str> for Object` builds
  a **Name**, so a *title* must use `Object::string_literal`; and `save_to`
  returns `io::Error` (not `lopdf::Error`).

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/src/pdf/cos.rs` | The lopdf layer: outline read/write, form-field read/rename — pure `&[u8]→Vec<u8>` transforms. |
| `src-tauri/Cargo.toml` | `lopdf = { version = "0.36.0", default-features = false }` + justification. |
| `docs/03_TECH_STACK.md` | The library decision + "not a double engine" review-gate sign-off + version lock. |
| `docs/04_ARCHITECTURE.md` | The byte-handoff integration model + `cos.rs` in the module tree. |
| `tests/fixtures/basic/forms.pdf` (+ generator) | AcroForm fixture (one text field) for the rename test. |
| `src-tauri/tests/cos.rs` | Spike tests — each asserts the lopdf output reopens in PDFium. |

#### Further reading

- `lopdf` — the COS document model: https://docs.rs/lopdf
- PDF object model (COS) & the `/Outlines`, `/AcroForm` dictionaries — PDF 32000-1, §7 and §12.

---

### P2.C1 — Reorder via thumbnail drag

#### Problem

Drag a page thumbnail to a new slot and the document reorders — Acrobat's
bread-and-butter page management, and the first feature built on the lopdf
COS layer. It was *blocked* (not just partial) before lopdf: PDFium exposes no
way to reorder the page tree.

#### Concepts learned

- **Reordering is an object-graph edit, not a render edit.** Page order lives
  in the root `/Pages` dictionary's `/Kids` array. Reordering = permuting that
  array. lopdf does it in a few lines; PDFium can't touch it. This is the COS
  layer's first real job.
- **Reference integrity is *free* when references are object-refs.** Links,
  bookmarks, and named destinations point at page **objects** (by id), not page
  positions. Permuting `/Kids` moves the pages but leaves the objects (and their
  ids) untouched — so every reference still resolves to the same page. The spec
  says "update all internal references"; for real PDFs there's nothing to
  update. (Same insight as delete.)
- **A cos edit *replaces* the live document; an in-place edit *mutates* it.**
  rotate/delete/crop mutate the actor's `PdfDocument` through PDFium calls.
  Reorder can't — the transform happens in lopdf, on bytes. So `ReorderEdit`
  does: serialize (`save_to_bytes`) → `cos::reorder_pages` → **`*doc = pdfium
  .load_pdf_from_byte_vec(new_bytes)`**. This is the byte-handoff made concrete,
  and every future cos-based mutation (merge/insert form fields, ref cleanup)
  reuses the exact shape.
- **Two subtleties of replacing `*doc`:** (1) `load_pdf_from_byte_vec` takes an
  *owned* `Vec<u8>` and hands the buffer to the document, so the new doc is
  `'static` — and `PdfDocument` is covariant in its lifetime (`PhantomData<&'a>`
  + `&'a` borrows, no `&mut`/`fn`), so the `'static` value slots into the
  generic `'a`. (2) The assignment **drops the old document**, which calls
  `FPDF_CloseDocument` — so it must happen *under* `PDFIUM_LOCK`, like every FFI
  call. Both handled by doing the reload inside one lock guard.
- **Invert a permutation for a free undo.** The inverse of "reorder by `p`" is
  "reorder by `p⁻¹`", where `inv[p[i]] = i`. So undo/redo store a `Vec<usize>`,
  not a multi-megabyte byte snapshot. Cheap and exact.
- **Identify a moved page without reading its text.** The test marks a page with
  an annotation (links.pdf's page 1) and asserts *which index* now carries the
  annotation after the reorder — proving order changed without OCR or text
  extraction. The same annotation trick verified merge/insert order.
- **Drag-reorder = one move → a full permutation.** A drag is "page `from` →
  position `to`"; the backend wants the whole new order. `movePage` (pure,
  unit-tested) does the splice; `isPermutation` is a defensive guard before the
  IPC call. Native HTML5 DnD on the `<li>` (draggable + onDragOver-preventDefault
  + onDrop); the dragged index rides in a ref so mutating it doesn't re-render.

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/src/pdf/cos.rs` | `reorder_pages` — permute root `/Kids` (flat-tree, validates permutation). |
| `src-tauri/src/pdf/reorder.rs` | `ReorderEdit` — serialize → cos → replace `*doc`; inverse = inverse permutation. |
| `src-tauri/src/pdf/actor.rs`, `commands/pdf.rs` | `ReorderPages` message + `pdf_reorder_pages`. |
| `src/ipc/reorder.ts`, `src/tools/reorder/compute-reorder.ts` | Wrapper + `movePage`/`isPermutation`. |
| `src/panels/ThumbnailPanel.tsx` | HTML5 drag-and-drop on tiles → reorder → epoch + history. |
| `src-tauri/tests/reorder.rs`, `tests/cos.rs` (+2), `src/tools/reorder/__tests__/…`, `src/ipc/__tests__/reorder.test.ts` | Tests. |

#### Further reading

- PDF page tree (`/Pages`, `/Kids`, `/Count`, inheritance) — PDF 32000-1 §7.7.3.
- `FPDF_LoadMemDocument64` ownership — why `load_pdf_from_byte_vec` keeps the buffer alive.
- HTML Drag and Drop API: https://developer.mozilla.org/en-US/docs/Web/API/HTML_Drag_and_Drop_API

---

### P2.C4 completion — full merge (bookmarks + form fields + rename)

#### Problem

The shipped merge concatenated pages + annotations but dropped bookmarks and
form fields (a `FPDF_ImportPages` limitation). Completing P2-PAGE-008 means
preserving the `/Outlines` and `/AcroForm` too, with colliding form-field
names made unique.

#### Concepts learned

- **Sometimes "complete the feature" means "replace the engine."** No lopdf
  fixup could cleanly recover what `FPDF_ImportPages` threw away (the AcroForm
  is gone; widgets are orphaned). So merge switched from PDFium-import to an
  **all-lopdf merge**: copy the *whole object graph* of each source into one
  document. Because nothing is discarded, outlines and form-field dicts arrive
  intact — there's nothing to reconstruct, only to *re-link*. Choosing the
  right layer (object graph vs. page API) turned a hard reconstruction problem
  into an easy bookkeeping one.
- **Merging documents = renumber to disjoint id ranges, then union.** Two PDFs
  both have an object `(1,0)`. lopdf's `renumber_objects_with(max_id)` shifts a
  source's ids past everything seen so far; then every object can be dumped
  into one `objects` map with no collisions, and — crucially — **every internal
  reference stays valid** because renumber rewrites refs too. After that, merge
  is just: pick one Catalog, build one `/Pages` with all the page refs,
  re-parent the pages, and re-link `/Outlines` + `/AcroForm`.
- **Preserve, don't rebuild, the outline.** lopdf's own merge example *drops*
  source outlines and synthesizes a "Page 1, Page 2…" TOC via `add_bookmark`.
  We do better: keep each source's outline items (they're already copied +
  renumbered, with correct `/Dest` refs and nesting) and only **re-chain the
  top level** under one new `/Outlines` root (`First`/`Last`/`Count` + each
  item's `Parent`/`Next`/`Prev`). Real titles, real nesting, real targets.
- **Collision rename is a seen-set over `/T`.** Walk the combined `/Fields`,
  track each name's count; the 2nd `name` becomes `name_2`, the 3rd `name_3`.
  Read the old `/T` (immutable borrow) and write the new one (mutable borrow) in
  two steps — never both at once.
- **Round-trip through PDFium is both the writer and the proof.** The lopdf
  merge produces bytes; we load them into PDFium and write via the verified
  `save_document`. That gives the atomic write *and* validates that PDFium
  accepts the lopdf output — and the bookmark test reads the result back
  *through PDFium's* outline API, proving the two engines agree.
- **Flip the tripwire you planted.** `merge_does_not_yet_carry_bookmarks` was a
  test asserting a *missing* feature. Completing the feature flips it to
  `merge_carries_bookmarks` — the deferred gap couldn't be silently forgotten.
- **Keep the old tests as regression guards across an engine swap.** Page
  count, annotation survival, and order were proven under the PDFium merge;
  keeping them green under the lopdf merge is how you swap engines without
  regressing the behavior that already worked.

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/src/pdf/cos.rs` | `merge_documents` (+ `merge_outlines`, `merge_acroform`, `resolve_acroform`, `type_is`) — the all-lopdf merge. |
| `src-tauri/src/pdf/merge.rs` | Engine swap: read source bytes → `cos::merge_documents` → load into PDFium → verified `save_document`. |
| `src-tauri/tests/merge.rs` | Flipped `merge_carries_bookmarks`; new `merge_carries_form_fields_with_rename`; old tests kept as regression guards. |
| `src-tauri/tests/cos.rs` | `cos_merges_outlines_and_fields_reopens_in_pdfium`. |

#### Further reading

- lopdf `examples/merge.rs` — the renumber+combine merge pattern (we extend it with outline + AcroForm preservation).
- PDF interactive forms (`/AcroForm`, field `/T`, `/Kids`) — PDF 32000-1 §12.7.

---

### P2.D1 completion — form fields on insert + a snapshot-undo primitive

#### Problem

Insert-from-PDF preserved content/annotations/dimensions but dropped form
fields (`FPDF_ImportPages` copies the widget annotations but doesn't link them
into the document `/AcroForm`). Completing P2-PAGE-005 means re-attaching them
— and making that reversible.

#### Concepts learned

- **A widget annotation and a form field aren't the same thing.** A form
  field's interactivity comes from being listed in the document `/AcroForm`
  `/Fields`. Page import copies the *widget* (it rides in the page's `/Annots`)
  but not the `/AcroForm` membership — so the inserted field renders but isn't
  "registered." The fix is a small lopdf pass that scans the inserted pages'
  `/Annots` for widgets carrying a `/T` and adds them to `/AcroForm /Fields`,
  creating the form if absent.
- **Don't swap the engine when a patch will do.** Merge needed a full lopdf
  rewrite (the AcroForm was *gone*). Insert didn't — PDFium already copies the
  widgets correctly, so the cheaper *hybrid* wins: keep the working PDFium
  page-copy, then a lopdf post-pass for the `/AcroForm` linkage only. Match the
  size of the fix to the size of the gap.
- **Some edits are easier to undo by snapshot than by inverse.** The old insert
  inverse was a `DeleteEdit` (remove the pages). But the edit now *also* mutates
  `/AcroForm` — and a delete wouldn't unwind that, leaving orphaned fields. The
  general fix is **`RestoreDocEdit`**: capture the document's bytes *before* the
  edit; undo replaces the live doc with that snapshot; redo replaces it with the
  post-edit snapshot. Correct for *any* structural change, at the cost of
  holding a full-doc byte snapshot in the undo stack. A reusable primitive — the
  escape hatch for edits too tangled for a clean inverse.
- **Make idempotent operations actually idempotent.** First cut of the field
  pass renamed a field that was *already* registered (it appeared in both
  "existing fields" and "widgets on this page" → false collision → `name_2`).
  The fix: subtract the already-registered set before treating widgets as new.
  Re-running the pass, or registering a page whose field is already in the form,
  is now a no-op. (Caught by the cos unit test — the integration tests, which
  insert genuinely new fields, passed regardless.)
- **Verify with a tool that doesn't share your code.** Beyond the tests
  (which read fields via our own `cos`), a raw-byte grep of the artifact
  confirmed `/AcroForm`, `/Widget`, and `/T (name)` are physically present — an
  independent check that the linkage is really in the file.

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/src/pdf/restore.rs` | `RestoreDocEdit` — generic snapshot-undo (replace `*doc` from bytes). |
| `src-tauri/src/pdf/cos.rs` | `register_inserted_form_fields` — re-attach inserted pages' terminal fields, idempotently, with `/T` collision rename. |
| `src-tauri/src/pdf/insert_from.rs` | `apply` now: snapshot → PDFium copy → field pass → reload; inverse = `RestoreDocEdit`. |
| `src-tauri/tests/insert_from.rs`, `tests/cos.rs` | Form-field preserve + rename tests; old tests kept as regression guards. |

#### Further reading

- PDF interactive forms: the widget-annotation ↔ field relationship (`/AcroForm`, `/T`, `/FT`) — PDF 32000-1 §12.7.
- Command pattern: snapshot vs. inverse-operation undo (memory/time trade-off).

---

### B2/C3 — dangling-reference cleanup on the write path

#### Problem

Deleting a page (B2) or splitting (C3) leaves *references to* the removed page
dangling: a link or bookmark that jumps to a page that's no longer there. The
spec's "update internal references" has two halves — surviving refs track
(done), and refs to removed pages get cleaned (this).

#### Concepts learned

- **Clean the artifact, not the session.** The fork was: prune the *live edited
  document* (per-edit) or the *file written to disk* (write-path). Per-edit
  would have forced delete onto a serialize→prune→reload with snapshot undo,
  **regressing its lightweight inverse**. Putting the prune in `save_document`
  — the one choke point every write passes — means the *saved file* is clean
  with zero undo-model change, and it covers delete, extract, **and** split in
  one place. "What reaches disk" is what the spec actually cares about.
- **Make a cross-cutting pass safe to be everywhere.** A prune in the universal
  save path is scary. Two properties tame it: it **returns the input unchanged
  when nothing dangles** (clean docs aren't even re-serialized), and it's
  **infallible** (any lopdf error → return the input), so it can never break a
  save. The existing round-trip-verify is the backstop.
- **Your test harness can lie about the system under test.** I tried to
  manufacture a dangling ref with lopdf's `delete_pages` — but lopdf's *writer
  strips references to deleted objects on save*, so it produced a *clean* doc.
  Even hand-removing the object didn't help: lopdf cleans on write. The dangling
  state only exists because **PDFium doesn't** clean it. Lesson: when testing
  "what tool A leaves behind," manufacture the state *with tool A* (the
  integration tests via the real PDFium delete), not with tool B that has
  different semantics.
- **Two shapes of "broken link."** PDFium-delete leaves a `/Dest [5 0 R]` whose
  target is gone (a *dangling* ref). `FPDF_ImportPages` (split) copies the link
  but *strips its dest entirely* (a *dead* link with no `/Dest`/`/A`). Both are
  broken; the detector handles both — while **keeping** `/URI` (external) links
  and named destinations, which have no resolvable page target by design.
- **Removing from a chain ≠ removing from the file.** Re-chaining the outline
  (fixing `First`/`Last`/`Next`/`Prev`) makes PDFium *show* the right bookmarks,
  but the dropped item still sits in the file orphaned (the integration test
  passed while a raw grep still found "Chapter 2"). `prune_objects()` GCs the
  orphans so the file is truly clean, not just functionally correct.
- **A correct cleanup can invalidate an old test's premise.** `insert`'s
  annotation test used links.pdf, whose only annotation is an internal link —
  which import dangles and the prune now (correctly) removes. The honest fix
  wasn't to weaken the prune but to test annotation-preservation with an
  annotation that *isn't* a broken link: a new `annots.pdf` with a `/Square`
  markup annotation (page-independent, survives both import and prune).

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/src/pdf/cos.rs` | `prune_dangling_destinations` (+ `is_broken_link`, `nav_target_page`, `prune_outline`, …) — remove broken links/bookmarks, GC orphans. |
| `src-tauri/src/pdf/document.rs` | `save_document` runs the prune before staging the temp file. |
| `tests/fixtures/basic/annots.pdf` (+ generator) | A `/Square` markup-annotation fixture for insert's annotation test. |
| `src-tauri/tests/{cos,delete_page,split,insert_from}.rs` | Prune tests via the real PDFium paths; insert annotation test repointed at `annots.pdf`. |

#### Further reading

- PDF destinations & GoTo actions (`/Dest`, `/A /S /GoTo`, named dests) — PDF 32000-1 §12.3.
- lopdf `prune_objects` / `delete_pages` — object GC and why the writer rewrites references.

---

### P2.C1 (GUI fix) — thumbnail reorder via pointer events

#### Problem

Drag-to-reorder thumbnails *looked* implemented (backend + tests all green) but
**did nothing in the running app**: you could pick up a thumbnail but dropping
it changed no order. The reorder backend (`reorder.rs` → `cos::reorder_pages`)
was never the problem — the drag never reached it.

#### Concepts learned

- **The Tauri webview is not Chrome.** Tauri renders the UI in the OS-native
  webview. On macOS that's **WKWebView** (the Safari engine). Anything that
  "works in Chrome" must still be checked there — the DOM APIs are *mostly* the
  same but not identical.
- **WKWebView doesn't support HTML5 drag-and-drop for in-page reordering.** The
  HTML5 DnD model is a sequence of events: `dragstart` on the source, then
  `dragenter`/`dragover` on whatever you pass over, then `drop` on the target,
  then `dragend`. We proved with one-line `console.warn` probes at each handler
  that WKWebView fires only **`dragstart` and `dragend`** — the three
  drop-target events **never fire**. With no `drop`, the handler that computes
  the new order can't run. No amount of `preventDefault`/`dataTransfer` fiddling
  fixes a missing event.
- **Instrument before you rewrite.** Rather than guess (the first guess — a
  nested page-tree that the backend rejects — was *wrong*), we added logging at
  every stage and had the human read the console. The trace (`dragstart` +
  `dragend`, nothing else) pinpointed the exact failure and ruled out four other
  hypotheses in one shot. This is the diagnosis loop: reproduce → instrument →
  read the evidence → *then* fix.
- **Pointer events are the portable substitute.** `pointerdown`/`pointermove`/
  `pointerup` are a unified model for mouse, touch, and pen, and WKWebView
  delivers them reliably. To rebuild drag-reorder on them:
  - **Click vs. drag:** on `pointerdown` just record the start; only treat it as
    a drag once the pointer moves past a small threshold (6px). Under the
    threshold it stays a click (→ select the page). This is the standard way to
    let one gesture mean two things.
  - **Pointer capture:** `setPointerCapture(pointerId)` makes one element keep
    receiving the move/up events even when the pointer leaves it — so the drag
    doesn't "drop out" when you move fast or past the list edge.
  - **Finding the drop target:** capture redirects *events*, not *hit-testing*,
    so `document.elementFromPoint(x, y)` still returns the real element under the
    cursor. Tag each tile with a `data-thumb-tile={page}` attribute and
    `.closest('[data-thumb-tile]')` from that point to read which page you're
    over. (We kept the lazy-load observer's separate `data-thumb-page` on the
    inner image div untouched — two attributes, two concerns.)
  - **Suppress the trailing click:** after a real drag, set a ref flag so the
    `click` the browser may synthesize next doesn't also "select" the page.

#### Files in this step

| File | Role |
|---|---|
| `src/panels/ThumbnailPanel.tsx` | Replaced HTML5 DnD (`draggable`/`onDragStart`/`onDrop`) with pointer-event handlers on the `<ul>` (`onTilePointerDown/Move/Up/Cancel`), a 6px click/drag threshold, `data-thumb-tile` hit-testing, and visual feedback (source dims, hovered tile rings). Backend call (`reorderPages`) unchanged. |
| `docs/04_ARCHITECTURE.md` | New "WebView quirks" section documenting the WKWebView DnD gap + the pointer-event pattern, so no future drag UI reaches for HTML5 DnD. |

#### Further reading

- Pointer events (unified mouse/touch/pen): https://developer.mozilla.org/en-US/docs/Web/API/Pointer_events
- `setPointerCapture`: https://developer.mozilla.org/en-US/docs/Web/API/Element/setPointerCapture
- `Document.elementFromPoint`: https://developer.mozilla.org/en-US/docs/Web/API/Document/elementFromPoint
- WKWebView HTML5 drag-and-drop limitations (WebKit) — long-standing gap for in-page DnD; pointer/mouse events are the portable workaround.

---

### P2.B5 — resize a page by scaling content (lopdf content-stream wrap)

#### Problem

"Resize a page to A4" sounds like just changing a number (the `/MediaBox`), but
the spec requires **scaling the content to fit** the new size — otherwise the
text would stay its original size in a differently-sized box. So we need to
transform every drawing on the page, then relabel the box.

#### Concepts learned

- **A PDF page is content + a box.** The `/MediaBox` (`[llx lly urx ury]`, in
  points = 1/72") defines the page rectangle; the `/Contents` stream(s) draw the
  marks. Resizing must scale the *contents* and set a new *box* — two separate
  steps. The box can also be **inherited** from a parent `/Pages` node, so to
  read a page's effective size you walk up `/Parent` until you find a `/MediaBox`.
- **Scaling content with a transformation matrix.** PDF content operators draw
  in "user space"; the `cm` operator concatenates an affine matrix
  `[a b c d e f]` onto the current transform. For a pure scale + offset that's
  `[sx 0 0 sy e f]`: `x' = sx·x + e`, `y' = sy·y + f`. Wrapping the whole content
  in `q … Q` (save/restore graphics state) keeps the scale from leaking. So
  prepending one stream `q sx 0 0 sy e f cm` and appending `Q` scales the entire
  page **without decompressing or parsing the original content** — `/Contents`
  is allowed to be an *array* of streams that PDFium concatenates.
  - **Preserve aspect** = uniform scale `s = min(W/w, H/h)` and centre the
    result (`e,f` add the leftover margin/2). **Stretch** = independent
    `sx = W/w, sy = H/h`.
- **When the obvious API is a trap — the diagnosis that forced a pivot.** The
  plan was to use PDFium's `FPDFPage_TransFormWithClip`. It *worked* (assertions
  passed) but the pdfium-render wrapper calls `reload_in_place()` (a documented
  workaround for PDFium issue #93) that left the document in a state that
  **SIGSEGV'd at process teardown** — and nondeterministically, which is the
  worst kind. Every PDFium content-transform path (even `page.scale()`) routes
  through it. Lesson: a passing assertion isn't a passing *process*; watch the
  exit code, and when a library's only API for a job is unreliable, change the
  mechanism rather than paper over it.
- **The byte-handoff makes the pivot cheap.** Because `cos` edits are pure
  `&[u8] → Vec<u8>`, swapping resize from "PDFium transform" to "lopdf
  content-wrap" touched only `ResizeEdit`'s internals + one new `cos` function —
  the actor, command, IPC, dialog, undo, and most tests were unchanged. That
  composability is the whole point of the byte-handoff architecture.

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/src/pdf/cos.rs` | `resize_pages()` — wrap each page's content with `q <matrix> cm … Q`, set the new `/MediaBox`, drop crop/bleed/trim/art boxes; `effective_media_box()` walks `/Parent` for an inherited box. |
| `src-tauri/src/pdf/resize.rs` | `ResizeEdit` — byte-handoff (serialize → `cos::resize_pages` → reload/replace doc); inverse is a `RestoreDocEdit` pre-resize snapshot. |
| `src-tauri/src/pdf/actor.rs`, `commands/pdf.rs`, `lib.rs` | `ResizePages` message + handle, `pdf_resize_pages` command + registration. |
| `src/ipc/resize.ts`, `src/tools/resize/{page-sizes.ts,ResizeDialog.tsx}` | Typed wrapper, the standard-size table, and the preset/custom dialog (size, preserve-aspect, this-page/all-pages). |
| `src-tauri/tests/resize.rs`, `tests/cos.rs`, `src/tools/resize/page-sizes.test.ts` | Integration (MediaBox/undo/validation), a byte-level content-wrap test, and the preset-table unit tests. |

#### Further reading

- PDF content streams & the `cm` operator — PDF 32000-1 §8.3 (coordinate systems & transforms), §7.8.2 (content streams).
- Graphics state `q`/`Q` — PDF 32000-1 §8.4.
- Inherited page attributes (`/MediaBox` on `/Pages`) — PDF 32000-1 §7.7.3.4.
- pdfium-render `reload_in_place` / issue #93 — why the PDFium content transform was unusable here.

---

### P3.A1 — the annotation tool framework (start of Phase 3)

#### Problem

Phase 3 adds ~10 annotation tools (highlight, sticky note, shapes, ink, …). They
all share one interaction — press, drag, release, commit — and one hard problem,
mapping screen pixels to PDF coordinates. Building that once, as a tested
framework, is the difference between ten consistent tools and ten subtly
different ones.

#### Concepts learned

- **A "framework" here = a contract + a driver, not a base class.** The contract
  (`AnnotationTool`) is three pure functions: `onPointerDown/Move/Up`, each a
  reducer `(draft, point) → draft`. The driver (`lifecycle.ts`) is a **state
  machine** that owns the phase (`idle`/`drawing`) and the in-progress draft, and
  calls the tool's reducers. Tools stay **stateless** — the lifecycle threads
  their draft through — which is why one tool instance can serve every gesture
  and why the whole thing is unit-testable with no DOM.
- **State machines make gesture code boring (in a good way).** Modelling input as
  `(state, event) → { state, committed? }` kills the class of bugs where a stray
  `move` without a `down`, or a second `down` mid-drag, corrupts things — those
  are just transitions that return unchanged. The test feeds event sequences and
  asserts the committed result.
- **Coordinate spaces: PDF vs. screen.** PDF user space has its origin at the
  **bottom-left, y up**, in points (1/72"). The browser has its origin
  **top-left, y down**, in CSS pixels, and the page may be displayed rotated
  (`/Rotate`). Converting between them means: divide by the render scale, flip y
  (`y_pdf = height − y_screen/scale`), and undo the rotation. Getting this wrong
  is *the* classic "my highlight is in the wrong place" bug, so it lives in one
  module (`coords.ts`) with **round-trip property tests** (`screen → pdf → screen
  ≈ identity`) for all four rotations.
- **Anchor convention for drag-rects:** while dragging, keep the press point as
  one corner and the cursor as the other *without normalizing*; normalize
  (min/max) only on commit. Otherwise dragging back across the start point loses
  the anchor.
- **A "preview store" is not a source of truth.** The annotation store is a
  frontend staging area (like the rotation-preview store) so tools + the render
  layer can be built now; the **actor** will own persisted annotations + undo in
  B1. Naming this boundary explicitly (in the store's header + docs/04) prevents
  the frontend quietly becoming a second, drifting source of truth — the same
  discipline as "PDF.js never writes."

#### Files in this step

| File | Role |
|---|---|
| `src/tools/_framework/types.ts` | Annotation domain model (PDF-space shapes, draft = annotation minus identity). |
| `src/tools/_framework/tool-contract.ts` | The `AnnotationTool` interface — three stateless gesture reducers. |
| `src/tools/_framework/lifecycle.ts` | Pure `stepTool` state machine (idle→drawing→commit/cancel). |
| `src/tools/_framework/coords.ts` | screen↔PDF mapping (scale, y-flip, 4 rotations) + rect helpers. |
| `src/tools/_framework/registry.ts`, `example-rect-tool.ts`, `index.ts` | Tool registry, a test-only drag-rect tool, public surface. |
| `src/state/{tool-store,annotation-store}.ts` | Active tool + options; per-doc draft + committed annotations (preview layer). |

#### Further reading

- Pointer Events spec & `setPointerCapture` — the portable gesture API (and our WKWebView workaround, docs/04).
- PDF coordinate system (`/MediaBox`, user space, `/Rotate`) — PDF 32000-1 §8.3.
- State machines for UI input (xstate's "why statecharts" essays) — the idea, not the library; ours is a hand-rolled reducer.

---

### P3.A2 — the annotation render layer

#### Problem

A1 built the *logic* (lifecycle, coords, stores) but nothing visible. A2 is the
DOM: an SVG layer over each page that draws annotations and turns real pointer
events into the gestures A1's reducers consume. Two non-obvious problems showed
up — coexisting with an imperatively-managed canvas, and pointer events in tests.

#### Concepts learned

- **An SVG overlay is the natural layer for vector annotations.** Each page gets
  an `<svg>` sized to the rendered canvas; annotations become `<rect>`/`<ellipse>`
  positioned with `coords.pdfToScreen`. SVG (not a second canvas) means crisp
  shapes at any zoom and free hit-testing — the browser tells you which `<rect>`
  you clicked.
- **`pointer-events` is how you stack interactive layers.** The overlay sits on
  top of the page. While a tool is active it must catch every pointer event
  (`pointer-events: auto`) to draw; while idle it must be **click-through**
  (`pointer-events: none`) so scrolling/selection underneath still works — *except*
  the annotation shapes themselves, which keep `pointer-events: auto` so a click
  still selects one. That per-element override (parent `none`, child `auto`) is
  the whole trick.
- **React vs. imperative DOM in the same box.** `PageSlot` mounts its canvas
  *imperatively* (`appendChild`, and a `while (firstChild) removeChild` to clear).
  A React-rendered overlay placed as a sibling **inside that same node gets wiped**
  by the clear. Fix: give the canvas its **own inner div** to clear, and make the
  overlay a sibling — React owns one subtree, the imperative code owns the other.
  A subtlety that bit back: the element registered for **scroll-to-page** must stay
  the *outer, in-flow* div (an absolutely-positioned inner div has `offsetTop: 0`,
  which would re-break the jump-to-page fix from earlier).
- **jsdom doesn't implement `PointerEvent`.** `fireEvent.pointerDown(el, {clientX})`
  silently produces a bare event with no coordinates, so the committed annotation
  came out `NaN`. A ~10-line subclass of `MouseEvent` (which jsdom *does* do,
  including `clientX/Y`) in the shared test setup fixes every pointer-driven
  component test now and later. Lesson: when a component test yields `NaN`/
  `undefined` from an event, suspect the test environment's event model before the
  component.
- **Keep the demo honest but cheap.** A2 has no persistence, so to make it
  eyeball-able we registered the *test* rect tool behind a temporary toolbar
  toggle, clearly marked for removal in B1. Better a labelled throwaway than
  pretending infra is a finished feature.

#### Files in this step

| File | Role |
|---|---|
| `src/view/annotation-layer.tsx` | Per-page SVG overlay: render committed + draft annotations; drive the A1 lifecycle from pointer events; hit-test for select. |
| `src/view/PageVirtualizer.tsx` | `PageSlot` restructured — inner canvas div + sibling overlay; outer flow element still registered for scroll. |
| `src/app/ZoomToolbar.tsx` | Temporary "▭" toggle + example-tool registration (A2 demo; removed in B1/C1). |
| `src/test-setup.ts` | jsdom `PointerEvent` polyfill (MouseEvent subclass) for pointer-driven component tests. |

#### Further reading

- `pointer-events` (CSS) — MDN; the layered-interaction pattern.
- SVG coordinate system & `<rect>`/`<ellipse>` — MDN SVG tutorial.
- jsdom limitations (no PointerEvent / layout) — why component tests stub the environment.

---

### P3.B1a — text selection + markup preview

#### Problem

Highlighting needs two things the app didn't have: **selectable text**, and a way
to turn a selection into the geometry a PDF highlight stores (`/QuadPoints`). B1a
builds both, preview-only — the actual PDF write is B1b.

#### Concepts learned

- **A PDF page on screen is three stacked layers.** Bottom: the rasterised
  **canvas** (what you see). Middle: the **text layer** — transparent, precisely
  positioned `<span>`s of the real text, so the browser's native selection works
  over the page. Top: our **annotation overlay** (SVG). PDF.js's `TextLayer`
  builds the middle layer from `page.getTextContent()` + the same viewport the
  canvas used; it needs a `--scale-factor` CSS var and the `.textLayer` rules to
  position spans. "Selectable PDF text" is this invisible layer, not the canvas.
- **Not every tool is a drag gesture.** Shapes/ink fit the A1 pointer lifecycle
  (down→move→up). **Text markup doesn't** — the interaction is "make a native
  text selection, then click a button." So markup skips `stepTool` entirely:
  read `window.getSelection()`, map it, done. Recognising that two interaction
  models coexist (gesture vs. selection-apply) kept us from forcing markup
  through the wrong abstraction.
- **`Range.getClientRects()` → `/QuadPoints`.** A text selection exposes **one
  rect per line**; each becomes a *quad* (4 corners) in PDF space. The corner
  order matters (`UL, UR, LL, LR`) — get it wrong and readers draw a bow-tie. We
  isolate the mapping (rects → quads, via the A1 `coords`) in a pure module with
  tests, because it's the bug-prone part.
- **`mousedown` steals your selection.** Clicking a toolbar button normally
  collapses the page selection *before* the click handler runs. The fix is one
  line — `onMouseDown={e => e.preventDefault()}` on the markup buttons — so the
  selection survives to be read on click. A classic, easy-to-miss gotcha.
- **`Omit` doesn't distribute over unions.** Once `Annotation` became
  `Rect | Markup`, `Omit<Annotation, "id">` silently collapsed to only the
  *common* keys (TS computes `keyof` as the intersection), dropping `rect`/
  `quads`. A `DistributiveOmit` (`T extends unknown ? Omit<T,K> : never`) fixes
  it — and accessing a variant-specific field then needs a narrow (`"rect" in x`).
- **`mix-blend-mode: multiply`** makes an SVG highlight behave like a real
  highlighter: the translucent colour darkens the text underneath instead of
  hiding it — no manual alpha-compositing.

#### Files in this step

| File | Role |
|---|---|
| `src/view/text-layer.tsx` | PDF.js `TextLayer` over each page (selectable text). |
| `src/tools/text-markup/quads.ts` | Pure: selection line-rects → `/QuadPoints`. |
| `src/tools/text-markup/apply-markup.ts` | Read the selection, group rects per page, add markup to the store (pure `buildMarkupDrafts` core). |
| `src/app/MarkupToolbar.tsx` | Highlight/underline/strike/squiggly + colour; `mousedown` preventDefault preserves the selection. |
| `src/view/annotation-layer.tsx` | Render markup (highlight polygons w/ multiply blend; underline/strike/squiggly lines). |
| `src/view/PageVirtualizer.tsx`, `styles/globals.css` | Mount the text layer + geometry data-attrs; `.textLayer` CSS. |

#### Further reading

- PDF.js `TextLayer` (display API) + `.textLayer` CSS — the selectable-text layer.
- `/QuadPoints` (PDF 32000-1 §12.5.6.10, text-markup annotations) — corner order.
- `Selection`/`Range.getClientRects()` — MDN; turning a selection into rects.
- `Omit` over unions / distributive conditional types — the TS gotcha.

---

### P3.B1b — persist text markup to the PDF (the first annotation write)

#### Problem

B1a previewed highlights in an overlay; they vanished on reload. B1b makes them
**real**: a standard PDF annotation, written into the file, visible in Acrobat /
Preview / Chrome and undoable — the first time we write an *annotation* to a PDF.

#### Concepts learned

- **A text-markup annotation is a dictionary + an appearance.** The dict says
  *what* it is: `/Subtype /Highlight`, `/QuadPoints` (the marked rectangles),
  `/C` (colour), `/Rect`, `/P` (its page). The **`/AP`** ("appearance") says *how
  to draw it* — a little embedded form (a Form XObject) with a content stream.
  Acrobat can regenerate appearances from `/QuadPoints`+`/C`, but Preview/PDF.js
  largely won't, so **shipping an `/AP` is what makes the markup show everywhere**.
- **Generating an appearance stream.** The `/AP` is drawn in the page's own
  coordinates by setting the form's `/BBox == /Rect` with an identity matrix —
  then you draw with absolute page coords. Highlight = fill each quad's rectangle
  with the colour under a **Multiply blend** (`/ExtGState << /BM /Multiply >>`,
  `re f`) so the text underneath stays readable; underline/strikeout = a stroked
  line (`m … l S`); squiggly = a little zigzag path. PDF content operators are
  just `x y w h re`, `rg`/`RG` (fill/stroke colour), `gs` (graphics state), `f`/`S`.
- **PDFium can read/keep annotations but can't *author* a coloured one.** Its
  Rust binding exposes quadpoints but no colour setter, so authoring goes through
  **lopdf** (build the dict + stream, append to `/Annots`) — same byte-handoff as
  every other structural edit. PDFium then preserves it across the save round-trip
  (verified: `/Subtype/Highlight` + `/QuadPoints` + `/AP` survive).
- **"Who renders the committed annotation?" is an architectural choice.** Two
  models: the **engine** renders it (write to PDF → reload → PDF.js draws the
  `/AP`), or the **frontend overlay** renders it from a store. We chose the
  engine — it matches every other edit ("the PDF is the source of truth; the
  viewer renders it"), needs no store↔PDF sync, and makes *reopened* files just
  work. B1a's overlay-rendering became preview scaffolding, kept (inert) for a
  future optimistic preview.
- **Undo of a "write a whole annotation" edit** reuses the snapshot pattern
  (`RestoreDocEdit`): cheaper to remember the bytes-before than to author a
  precise "remove that one annotation" inverse — same call we made for resize/D1.

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/src/pdf/cos.rs` | `add_text_markup` — annot dict + generated `/AP` appearance → `/Annots`. |
| `src-tauri/src/pdf/annotation.rs` | `TextMarkupEdit` (byte-handoff; `RestoreDocEdit` inverse). |
| `actor.rs`, `commands/pdf.rs`, `lib.rs` | `AddTextMarkup` message + `pdf_add_text_markup` command. |
| `src/ipc/annotations.ts`, `src/app/MarkupToolbar.tsx`, `src/tools/text-markup/apply-markup.ts` | Typed wrapper; toolbar now writes via IPC (not the store) → epoch reload. |

#### Further reading

- Text-markup annotations + `/QuadPoints` — PDF 32000-1 §12.5.6.10.
- Appearance streams (`/AP`, Form XObjects) — §12.5.5, §8.10.
- Blend modes (`/BM /Multiply`, `/ExtGState`) — §11.3.5.
- PDF content stream operators (`re`, `rg`, `gs`, `f`, `S`) — §8.

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
