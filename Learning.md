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

### P3.B1 (debug) — making the text layer actually work in WKWebView

#### Problem

Text markup looked done (tests green, `/AP` correct), but in the *real app* you
couldn't select text at all — and once you could, highlights covered the whole
page or came out the wrong colour. The PDF.js text layer was fighting WKWebView
on three fronts, each invisible until exercised. This is the "passing tests ≠
working feature" lesson, hard.

#### Concepts learned

- **The app isn't Chrome — verify in the actual webview.** Every problem here was
  WKWebView-specific and *zero* of them showed in jsdom tests or Chrome. The dev
  console (captured in the vite log) was the only way to see them. When a feature
  works in tests but not the app, suspect the webview.
- **`ReadableStream` isn't async-iterable in Safari/WKWebView.** PDF.js v5
  `getTextContent` does `for await (… of streamTextContent())`; WKWebView throws
  "undefined is not a function" because `ReadableStream[Symbol.asyncIterator]`
  doesn't exist. A ~15-line polyfill (wrap a reader as an async iterator) fixes
  every `for await`-on-a-stream in the app.
- **Read the error, don't guess.** I burned a round guessing it was font data
  (`standardFontDataUrl`). The actual message — "undefined is not a function
  (near '…value of readableStream…')" — pointed straight at the async iterator.
  Instrument and read the message *first*.
- **Port the *whole* vendor CSS, not a "minimal" subset.** My hand-rolled
  `.textLayer` CSS dropped the rule that turns PDF.js's per-span `--font-height`
  / `--scale-x` variables into `font-size` + `scaleX(...)`. Without it spans had
  no size, so a click selected the entire page. The fix was copying pdfjs-dist's
  real `pdf_viewer.css` `.textLayer` rules verbatim. Lesson: when integrating a
  library's DOM layer, use its CSS as-is — "minimal ports" silently break the
  parts you didn't understand.
- **CSS `round()` may be unsupported.** v5 sizes the layer with `round(down,
  var(--total-scale-factor) * Npx, …)`; pinning an explicit px size after render
  sidesteps webviews without `round()`.
- **`Range.getClientRects()` includes container rects.** A text selection
  returned the line rect *plus* the text-layer's own full-page rect — which
  became a full-page highlight (and stacked with each new one). Filter rects that
  are implausibly tall for a text line (> half the page).
- **Generated assets are scripted, not committed.** `public/pdfjs/` is gitignored
  and populated by `scripts/copy-pdfjs-worker.mjs` from node_modules; new runtime
  assets (the standard fonts / cmaps) go in that script, not a 185-file commit.

#### Files in this step

| File | Role |
|---|---|
| `src/polyfills.ts` (+ `main.tsx`) | `ReadableStream` async-iterator polyfill, loaded first. |
| `src/styles/globals.css` | The full v5 `.textLayer` CSS (span font-size + scaleX). |
| `src/view/text-layer.tsx` | Set `--scale-factor`; pin explicit px size after render. |
| `src/view/render-page.ts` | `standardFontDataUrl` / `cMapUrl` on `getDocument`. |
| `src/tools/text-markup/apply-markup.ts` | Drop the page-tall (container) selection rect. |
| `scripts/copy-pdfjs-worker.mjs` | Also copy `standard_fonts/` + `cmaps/` into public/. |
| `src-tauri/src/pdf/cos.rs`, `annotation.rs`, … | `clear_text_markup` + `ClearMarkupEdit` + `pdf_clear_text_markup` + the Clear button. |

#### Further reading

- `ReadableStream` async iteration support (Safari/WebKit) — and the reader-based polyfill.
- PDF.js `pdf_viewer.css` `.textLayer` rules — the canonical text-layer styling.
- `getDocument` params (`standardFontDataUrl`, `cMapUrl`, `cMapPacked`) — non-embedded font / CID support.

---

### P3.B2a — sticky notes (place / edit / delete + persist)

#### Problem

A sticky note is the second annotation type, and the first one that is
*interactive after it lands*: you place an icon, click it to open a popup, edit
the body, and delete it. We needed all of that to persist into the PDF as a
standard `/Text` annotation that other readers understand — while reusing the
undo/actor plumbing from the markup write.

#### Concepts learned

- **PDF `/Text` annotation** — the "sticky note" of the PDF spec: a fixed-size
  icon plus a popup. Key dict entries: `/Contents` (body), `/T` (author/title),
  `/M` + `/CreationDate` (timestamps), `/Name` (which icon — `/Note`, `/Comment`,
  …), `/C` (colour), `/Open` (popup open by default?), and `/F` (flags).
- **Annotation flags `/F`** — a bitfield. We set `28` = `Print`(4) + `NoZoom`(8)
  + `NoRotate`(16): the icon prints, stays a constant on-screen size as you zoom,
  and doesn't rotate with the page. That `NoZoom` is why we render the overlay
  icon at a **fixed pixel size**, not scaled by the zoom factor.
- **Why a note carries NO `/AP`** — unlike markup, where we *generate* an
  appearance stream so the PDF.js canvas can draw it, a reader is expected to draw
  the note icon itself from `/Name`. If we also shipped an `/AP` we'd either
  double-draw or fight the reader's own icon. So: no `/AP`, and **we** draw the
  icon in an HTML overlay.
- **Canvas-rendered vs overlay-rendered annotations** — the project now has both
  paths. Markup → bake an `/AP`, reload, let the canvas paint it (the store is
  *not* the source). Notes → no `/AP`, so the `NoteLayer` HTML overlay paints the
  icon + popup from the annotation store. Picking the right path per annotation
  type is the core design call here.
- **Stable id ↔ `/NM`** — to edit or delete *this* note later we need to find it
  again. We use the frontend store's id as the annotation's `/NM` (name) and look
  it up with `find_annotation_by_nm`. The id is generated once at placement and is
  the single handle shared by the store and the PDF.
- **Optimistic UI with rollback** — placement adds the icon to the store
  immediately (so it appears instantly) and fires the actor write in the
  background; if the write rejects, we remove the icon. Store and PDF stay in
  lockstep from the first frame.
- **A civil date without `chrono`** — `pdf_date_now` formats `D:YYYYMMDDHHmmSSZ`
  from a Unix timestamp using Howard Hinnant's `days_from_civil` algorithm
  inverted, avoiding a new dependency for one date string.
- **`pointer-events` parent/child override** — the overlay container is
  `pointer-events: none` when idle (so page scroll / text selection pass
  through), but the icons and popup set `pointer-events: auto`, so they're still
  clickable. A child can re-enable hit-testing its parent disabled.

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/src/pdf/cos.rs` | `add_text_note` / `update_text_note` / `delete_annotation` + `pdf_date_now` / `append_annotation` / `find_annotation_by_nm` (lopdf). |
| `src-tauri/src/pdf/annotation.rs` | `cos_edit` helper + `AddNoteEdit` / `UpdateNoteEdit` / `DeleteAnnotationEdit`. |
| `src-tauri/src/pdf/actor.rs`, `commands/pdf.rs`, `lib.rs` | Actor messages + commands `pdf_add_text_note` / `pdf_update_text_note` / `pdf_delete_annotation`. |
| `src/ipc/notes.ts` | Typed IPC wrappers for the three note commands. |
| `src/tools/sticky-note/note-tool.ts` | Click-to-place reducer + `noteDraftAt` + default author / colour. |
| `src/view/note-layer.tsx` | HTML overlay: draws note icons, places + persists, hosts the popup. |
| `src/view/NotePopup.tsx` | Controlled popup editor (body + Save + Delete). |
| `src/view/annotation-layer.tsx` | Skip `note` annotations (the overlay owns them). |
| `src/view/PageVirtualizer.tsx` | Mount `NoteLayer` above the SVG layer. |
| `src/app/MarkupToolbar.tsx` | The **Note** tool toggle. |
| `src-tauri/tests/text_note.rs`, `tests/cos.rs` | Round-trip + dict-shape tests; the `Sample PDFs` artifact. |

#### Further reading

- PDF 32000-1:2008 §12.5.6.4 (Text annotations) and §12.5.3 (annotation flags `/F`).
- Howard Hinnant, "chrono-Compatible Low-Level Date Algorithms" (`days_from_civil`).
- CSS `pointer-events` — parent `none` + child `auto` hit-testing.

---

### P3.B2b — notes as a projection of the PDF (re-openable + undo-safe)

#### Problem

B2a's note overlay was an *in-session cache*: it only knew about notes placed
this session. So a saved-then-reopened file showed no notes in-app (only other
readers saw them), and an actor-level ⌘Z left a *ghost* — the PDF lost the note
but the overlay kept drawing its icon. The spec says notes "SHALL be
re-openable"; both gaps had to close.

#### Concepts learned

- **Source of truth vs. projection.** The architecture rule is "the actor owns
  every byte." A frontend store that the actor doesn't drive is a *cache*, and
  caches drift. The fix is to treat the store as a **projection**: re-derive it
  from the PDF whenever the PDF could have changed, so the two can't disagree. We
  stopped *mutating* the note store in lockstep and started *replacing* it from a
  read.
- **Read path as the inverse of the write path.** `cos::read_text_notes` is the
  mirror image of `add_text_note`: walk each page's `/Annots`, keep `/Subtype
  /Text`, pull `/NM`, `/Rect` lower-left, `/Contents`, `/T` back out into a
  `NoteData`. Round-trip tests assert *write then read* returns what you wrote.
- **A read-only actor message.** Edits snapshot → transform → reload and push an
  undo entry; a *query* (`ReadNotes`) just serializes the live doc under the
  PDFium lock and parses it — no `dirty`, no history. Modeled on `GetBytes`. Keeps
  "all bytes flow through the actor" intact without pretending a read is an edit.
- **Choosing a re-sync trigger.** What signals "the PDF's notes might have
  changed"? Document identity (open/restore/tab-switch) and the **edit epoch** —
  a monotonic per-doc counter already bumped by every reload-edit, including
  undo/redo. Keying the effect on `[documentId, epoch]` covers all of them with
  no new plumbing, and decouples the re-sync from the undo/redo code itself.
- **Deliberately *not* signaling.** Note *placement* skips the epoch bump on
  purpose: the icon is added optimistically and persisted in the background, and a
  re-sync firing mid-flight could read the PDF before the write landed and drop
  the new note. Knowing which actions should *not* invalidate a projection is as
  important as knowing which should.
- **Replace, don't append.** `replaceNotes` swaps only the `note`-type
  annotations for a doc and keeps the rest — so re-syncing notes never disturbs
  other annotation types, and a stale icon can't survive a re-read.
- **Synthesizing a stable id.** Edits target a note by `/NM`. A note authored
  elsewhere may have none, so we synthesize `obj-<num>-<gen>` from its object id —
  stable within a load, enough to render, and a later edit writes a real `/NM`.

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/src/pdf/cos.rs` | `read_text_notes` + the `NoteData` DTO (serde, the inverse of `add_text_note`). |
| `src-tauri/src/pdf/actor.rs` | Read-only `ReadNotes` message + `read_notes` handle (serialize → lopdf-parse). |
| `src-tauri/src/commands/pdf.rs`, `lib.rs` | `pdf_read_text_notes` command + registration. |
| `src/ipc/notes.ts` | `NoteData` type + `readTextNotes` wrapper. |
| `src/state/annotation-store.ts` | `replaceNotes` — swap a doc's notes, keep other types. |
| `src/app/use-notes-sync.ts` | Hook: read → map → `replaceNotes` on `[documentId, epoch]`. |
| `src/app/App.tsx` | Mount `useNotesSync` beside `useHistory`. |
| `src-tauri/tests/{cos,text_note}.rs`, FE `notes` / `annotation-store` / `use-notes-sync` | Read-back, undo/redo, projection-replace tests. |

#### Further reading

- "Projection" / read model (CQRS) — deriving a queryable view from a source of truth.
- React `useEffect` dependency keys as invalidation signals; `renderHook` + `waitFor` for async-effect tests.
- PDF 32000-1:2008 §12.5.2 (annotation `/Rect`, `/NM`).

---

### P3.B3a — free-text boxes (drawing text into a PDF)

#### Problem

The third annotation type writes *typed text* into the page — a box you size,
fill with text, and style (font, size, colour, bold/italic). Unlike a note (an
icon) or markup (a shape over existing text), the appearance is glyphs we have to
draw ourselves, and the text has to render in every reader.

#### Concepts learned

- **PDF `/FreeText` annotation** — a box of text. Key entries: `/Rect` (the box),
  `/Contents` (the plain text), `/DA` (default appearance — a tiny graphics
  snippet: font + size + colour), and an `/AP` appearance stream that *draws* the
  text. We generate the `/AP` so it looks identical everywhere.
- **A text-drawing content stream.** Inside the `/AP` form XObject, text is drawn
  with PDF text operators: `BT`/`ET` (begin/end text), `Tf` (set font + size),
  `rg` (fill colour), `Td` (set the start position = first baseline), `TL` +
  `T*` (leading + next-line), and `Tj` (show a string). One `Tj` per line; `T*`
  between lines. Coordinates are bottom-up, so the first baseline sits near the
  *top* of the box (`y1 − size`).
- **The base-14 fonts.** Every PDF reader ships 14 standard fonts (Helvetica,
  Times, Courier families + Symbol/ZapfDingbats). Referencing one
  (`/BaseFont /Helvetica-Bold`) needs **no font embedding** — which is why B3a
  restricts to those and maps family+bold+italic to the right PostScript name
  (e.g. Times+B+I → `Times-BoldItalic`). Embedding arbitrary fonts is a much
  bigger job (font subsetting) — deferred.
- **A self-contained appearance.** The `/AP` form carries its *own* `/Resources
  /Font`, so display never depends on the document's AcroForm `/DR`. `/DA` is
  still written (the spec wants it; a reader that *regenerates* appearance uses
  it) but is best-effort — `/AP` is the primary path.
- **Escaping a PDF literal string.** Text inside `( … )` must escape `\`, `(`,
  `)` (else the parser miscounts parens). A one-pass `pdf_escape` handles it; the
  cos test feeds `a(b)\c` and asserts `a\(b\)\\c`.
- **Same rendering split as markup, plus a transient editor.** Because the box
  has an `/AP`, the **canvas** draws it (write → epoch reload). So the overlay
  (`FreeTextLayer`) holds *no committed boxes* — only the live drag-preview and
  the `<textarea>` you type into. The editor is throwaway; the PDF is the record.
- **Drag gesture → two coordinate spaces.** The drag is captured in screen px (for
  the preview + the editor's CSS position) and converted to PDF points (for the
  write). A click (sub-threshold drag) is grown to a default box so you always get
  something usable — `withDefaultSize`.
- **Re-applying an earlier lesson.** The editor sets `pointer-events: auto`
  up-front — the same trap that bit the note popup (an overlay child inherits the
  container's `none`). Lessons compound only if you apply them.

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/src/pdf/cos.rs` | `add_free_text` + the `/AP` text stream, `base_font`, `pdf_escape`. |
| `src-tauri/src/pdf/annotation.rs` | `FreeTextEdit` (byte-handoff via `cos_edit`). |
| `src-tauri/src/pdf/actor.rs`, `commands/pdf.rs`, `lib.rs` | `AddFreeText` message + `pdf_add_free_text`. |
| `src/ipc/freetext.ts` | `addFreeText` typed wrapper. |
| `src/tools/free-text/free-text.ts` | Font catalog, CSS mapping, screen-rect math. |
| `src/view/free-text-layer.tsx` | Drag-to-box + the transient `<textarea>` editor; commit via IPC. |
| `src/app/MarkupToolbar.tsx`, `src/state/tool-store.ts`, `types.ts` | The **Text** tool + font/size/B/I controls + `ToolOptions` fields. |
| `src-tauri/tests/free_text.rs`, `tests/cos.rs`, FE `freetext`/`free-text`/`free-text-layer` | Round-trip, font-variant, escaping, overlay-wiring tests. |

#### Further reading

- PDF 32000-1:2008 §12.7.3.3 (`/DA`), §12.5.6.6 (FreeText), §9.6.2 (the standard-14 fonts).
- PDF text-showing operators (`BT`/`Td`/`TL`/`T*`/`Tj`/`Tf`).
- WinAnsi / StandardEncoding — why base-14 fonts only render a Latin range.

---

### P3.D1 — the annotation sidebar (reading them all back)

#### Problem

We could *write* three annotation kinds (markup, notes, free-text) but had no
way to *see what's in a document* — to list, search, filter, and jump to
annotations. D1 is the first feature that reads **every** kind back out and
presents them as a managed list.

#### Concepts learned

- **A unified read over a heterogeneous collection.** `/Annots` mixes subtypes
  (Highlight, Text, FreeText, plus Link/Widget/Popup we *don't* own). The read
  whitelists the six we surface and maps each `/Subtype` to a `kind` tag — turning
  a ragged object graph into one flat, uniform `AnnotationInfo` list the UI can
  treat alike.
- **Parsing a PDF date.** `/M` is `D:YYYYMMDDHHmmSS` + optional offset. Parsing it
  to epoch-ms is the inverse of the `pdf_date_now` we wrote in B2a — same
  Hinnant `days_from_civil` algorithm, run forward. Tolerate junk → `None`.
- **Read model vs. write model, again.** Like B2b's note projection, the panel
  re-reads on `[documentId, epoch]`, so the list always reflects the PDF after
  any edit/undo. The actor query is read-only (no history) — a *query*, not a
  *command*.
- **A stable-enough handle.** Markup/free-text carry no `/NM`, so the list keys
  each row on the lopdf **object id** (`"5 0"`). Stable within a load — enough to
  track selection and draw a highlight — but not across a save, which is exactly
  why per-annotation *delete* (which needs a durable handle) is deferred.
- **Pure filter/group, testable without a DOM.** Search + filter (type/author/
  date) + group-by-page live in `annotation-filter.ts` as pure functions; the
  React panel is a thin shell over them. The 8 helper tests need no render.
- **A cross-cutting selection channel.** The sidebar (top of the tree) and the
  per-page highlight overlay (deep in the page list) don't share a parent, so a
  tiny zustand store (`annotation-selection-store`, like `tool-store`) carries the
  selected annotation — *including its `/Rect`*, so the overlay needs no second
  lookup to draw the box.
- **Reusing the navigation seam.** "Click → scroll to page" is just the existing
  `PageVirtualizerHandle.scrollToPage` that the outline/thumbnail panels already
  call — new feature, no new plumbing.

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/src/pdf/cos.rs` | `read_annotations` + the `AnnotationInfo` DTO + `parse_pdf_date`. |
| `src-tauri/src/pdf/actor.rs`, `commands/pdf.rs`, `lib.rs` | Read-only `ReadAnnotations` + `pdf_read_annotations`. |
| `src/ipc/annotations.ts` | `readAnnotations` + the `AnnotationInfo`/`AnnotationKind` types. |
| `src/panels/annotation-filter.ts` | Pure search / filter / group-by-page / distinct helpers. |
| `src/panels/AnnotationPanel.tsx` | The sidebar: read on epoch, render grouped + filtered, click → jump + select. |
| `src/state/annotation-selection-store.ts` | The selected annotation (carries its `/Rect`). |
| `src/view/selection-highlight-layer.tsx` | Per-page dashed box over the selected annotation. |
| `src/state/view-store.ts`, `ZoomToolbar.tsx`, `PdfViewer.tsx` | The sidebar toggle + mount. |

#### Further reading

- CQRS read models / projections — a query-shaped view over a write-shaped store.
- PDF 32000-1:2008 §12.5.2 (annotation dictionaries, `/Subtype`, `/M`), §7.9.4 (dates).
- React: lifting cross-cutting UI state into a store vs. prop-drilling.

---

### P3.C1a — shape annotations (and the A2 overlay finally persists)

#### Problem

A2 shipped the drawing framework — a pointer-gesture lifecycle, a draft store,
and an SVG renderer — but its commit only added to an in-memory store; nothing
reached the PDF. C1a is where that framework becomes real: drag a rectangle or
ellipse and it's written to the document.

#### Concepts learned

- **PDF shape annotations.** `/Square` (a rectangle) and `/Circle` (an ellipse)
  are bounded by `/Rect`, with `/C` (stroke colour), `/IC` (interior/fill colour),
  `/CA` (opacity), and `/BS << /W >>` (border width). We also generate the `/AP`
  so every reader paints them identically.
- **No ellipse primitive in PDF.** Path drawing has lines (`l`), rectangles
  (`re`), and cubic Béziers (`c`) — but no ellipse. A circle/ellipse is the
  classic **four-Bézier approximation**: four quarter-arcs whose control points
  sit `kappa·r ≈ 0.5523·r` from each axis endpoint. Visually exact to well under a
  pixel.
- **Paint operators encode fill vs stroke.** After building the path you choose:
  `S` (stroke), `f` (fill), or `B` (fill *then* stroke). We pick based on whether
  a fill colour is set and the width is non-zero — and **inset the path by half
  the stroke width** so the stroke stays inside `/Rect` (a stroke straddles its
  path).
- **A factory for near-identical tools.** Rectangle and ellipse are the *same*
  gesture (press-drag-release) differing only in the draft's `type`, so one
  `makeDragRectTool(kind)` produces both — the anchor stays put while dragging,
  the rect normalizes only on commit (so dragging back across the origin still
  works).
- **The A2 → C1 evolution (the real lesson).** A2 deliberately committed to a
  *store* as a placeholder ("persistence arrives in B1/C1"). C1a fulfils it: the
  `annotation-layer`'s commit now **persists via the actor** (`addShape` → `/AP` →
  `bumpEpoch` → the canvas draws it) and the store holds only the *in-progress
  draft*. The committed shape is never in the store — same canvas-vs-overlay split
  as markup/free-text. Recognizing when scaffolding should be *replaced* (not
  extended) is the call here; its one A2 test was updated to assert the IPC
  persist, not a store add.
- **Module-load side effects for registration.** The shape tools register into the
  global tool registry at module-evaluation time (`for (…) registerTool(…)` at the
  top of `annotation-layer`), so they're available the moment the overlay mounts —
  no per-render registration.

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/src/pdf/cos.rs` | `add_shape` (`/Square`/`/Circle` + `/AP`) + the Bézier-ellipse appearance. |
| `src-tauri/src/pdf/annotation.rs` | `ShapeEdit` (byte-handoff via `cos_edit`). |
| `src-tauri/src/pdf/actor.rs`, `commands/pdf.rs`, `lib.rs` | `AddShape` message + `pdf_add_shape`. |
| `src/ipc/shapes.ts` | `addShape` typed wrapper. |
| `src/tools/shapes/shape-tools.ts` | `rectangleTool` + `ellipseTool` (a drag-rect factory). |
| `src/view/annotation-layer.tsx` | Registers the shape tools; commit now persists via IPC. |
| `src/app/MarkupToolbar.tsx`, `src/state/tool-store.ts`, `types.ts` | Rectangle/Ellipse toggles + a fill control + `ToolOptions.fillColor`. |
| `src-tauri/tests/shapes.rs`, `tests/cos.rs`, FE `shapes`/`shape-tools`/`annotation-layer` | Round-trip, fill-vs-stroke, reducer, and commit-persists tests. |

#### Further reading

- Bézier circle approximation and the magic constant kappa (≈ 0.5523).
- PDF 32000-1:2008 §8.5.3 (path-painting operators), §12.5.6.8 (Square/Circle annotations), §12.5.4 (`/BS` border style).
- The "replace the scaffold" instinct — when a placeholder implementation should be swapped out, not built upon.

---

### P3.D1d — select + delete annotations (the payoff of a stable identity)

#### Problem

You could *add* highlights, free-text, and shapes but never *remove* one (only
⌘Z right after). The verification sweep made it the top complaint. Deleting "that
specific annotation" needs a way to **name** it.

#### Concepts learned

- **A stable identity unlocks management.** The whole feature reduced to one
  thing: give every annotation a durable name. PDF's `/NM` (annotation name) is
  exactly that. Notes already had one; stamping a `uuid` `/NM` on markup /
  free-text / shapes made them all addressable — and then *delete already
  existed* (`cos::delete_annotation`, built for notes, finds by `/NM`). The hard
  part wasn't the delete; it was the identity.
- **Server-side vs client-side ids.** Notes' `/NM` is generated on the *frontend*
  because the overlay tracks the note by it before it's saved. Markup/shapes
  aren't tracked client-side, so their `/NM` is generated **server-side** in
  `cos` — which meant *zero* IPC/signature changes to the add paths. Choosing
  where an id is born depends on who needs to know it, and when.
- **A handle with a fallback scheme.** `read_annotations` returns the `/NM` when
  present, else a synthesized `obj:<num> <gen>` (the lopdf object id, prefixed so
  the two are unambiguous). `delete_annotation` branches on the `obj:` prefix.
  This makes *our* annotations robustly deletable (stable `/NM`) while still
  best-effort-deleting foreign ones — a graceful-degradation seam.
- **A test caught a latent gap.** The integration test expected 4 annotations and
  got 3: `annotation_kind` (written in D1, *before* shapes existed in C1a) never
  mapped `/Square`/`/Circle`, so shapes were silently absent from the sidebar.
  Features built in sequence drift; a cross-feature test is what surfaces it.
- **Delete needs no new machinery because of the projection.** One
  `deleteAnnotation` → `bumpEpoch` reloads the canvas (the `/AP` vanishes),
  re-reads the sidebar, and re-syncs the note overlay (`useNotesSync`) — all off
  the existing edit-epoch projection. The pieces composed.
- **Keyboard-delete must respect focus.** A global Delete-key handler has to
  no-op while you're typing in the search box or a popup — gate on
  `document.activeElement` being an input/textarea/contenteditable.

#### Files in this step

| File | Role |
|---|---|
| `docs/02_PRODUCT_SPEC.md` | New **P3-ANN-012** (select + delete). |
| `src-tauri/src/pdf/cos.rs` | `/NM` on markup/free-text/shape; `read_annotations` returns it (+ `obj:` fallback); `delete_annotation` deletes by either; `annotation_kind` += Square/Circle. |
| `src/ipc/annotations.ts` | `AnnotationKind` += rectangle/ellipse; re-export `deleteAnnotation`. |
| `src/panels/AnnotationPanel.tsx` | Row ✕ + Delete-key handler → `deleteAnnotation` → epoch refresh + clear selection. |
| `src-tauri/tests/annotation_delete.rs`, `tests/cos.rs`, FE `AnnotationPanel` | Delete-by-`/NM`, by `obj:`, undo, and the row/key wiring. |

#### Further reading

- PDF 32000-1:2008 §12.5.2 — the `/NM` (annotation name) entry.
- Stable identifiers as the precondition for CRUD on a collection.
- Global keyboard handlers + `document.activeElement` focus-guarding in React.

---

### P3.D1e — editing a free-text box in place (read it back, rewrite it)

#### Problem

You could add and delete a free-text box but not *fix a typo* in one — only
delete-and-retype. Editing means two new things: reading the annotation's current
state back into an editor, and rewriting it without changing its identity.

#### Concepts learned

- **Round-tripping a write.** To pre-fill the editor we parse back what we wrote:
  `/Contents` (trivial), size + colour from the `/DA` string (`/F1 18 Tf r g b
  rg`), and family/bold/italic from the `/AP` font's `/BaseFont` (an *inverse* of
  the `base_font` map). A writer and its reader are a matched pair — the reader is
  only as robust as the format the writer guarantees, so we only promise fidelity
  for boxes *we* authored (foreign ones fall back to defaults).
- **Update in place vs delete-and-re-add.** We chose update-in-place
  (`update_free_text` finds the annot by `/NM` and rewrites `/Contents` + `/Rect`
  + `/DA` + `/AP`) so the **`/NM` survives** — the sidebar selection, any future
  reference, and identity stay stable. Delete-and-re-add would have been simpler
  but would mint a new id every edit.
- **GC the orphan.** Swapping in a new `/AP` stream leaves the old one unreferenced;
  `prune_objects()` collects it so the file doesn't bloat with every edit.
- **Refactor when a second caller appears.** `add_free_text` and `update_free_text`
  share the appearance-stream + font-resource build and the grow-to-fit — so that
  logic moved into `free_text_appearance` / `grow_free_text_rect` helpers. The
  second caller is what justifies the extraction (not speculation).
- **A request channel, not a callback.** The sidebar (top of the tree) and the
  per-page `FreeTextLayer` (deep in it) don't share a parent, so "edit this box"
  travels through a tiny store (`annotation-edit-store`): the sidebar *posts* a
  request; the matching page's layer *claims* it (opens the editor) and *clears*
  it. Same shape as the selection store — a one-shot mailbox.
- **Faithful preview = set the tool to the box's style.** On entering edit mode we
  push the box's font/size/colour into the toolbar options, so the live `<textarea>`
  preview matches the committed result *and* the user can tweak the style from the
  same controls.
- **Reusing the create editor for edit.** The editor grew one field — `editNm`
  (null for a new box, the `/NM` for an edit) — and `commit` branches on it
  (`addFreeText` vs `updateFreeText`). One UI, two intents.

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/src/pdf/cos.rs` | `read_free_text` (parse text+style) + `update_free_text` (rewrite by `/NM`) + the shared `free_text_appearance`/`grow_free_text_rect` + `parse_da`/`font_from_base`. |
| `src-tauri/src/pdf/{annotation,actor}.rs`, `commands/pdf.rs`, `lib.rs` | `UpdateFreeTextEdit` + read-only `ReadFreeText` + `UpdateFreeText` + the two commands. |
| `src/ipc/freetext.ts` | `FreeTextData` + `readFreeText` / `updateFreeText`. |
| `src/state/annotation-edit-store.ts` | The sidebar → `FreeTextLayer` edit-request channel. |
| `src/panels/AnnotationPanel.tsx` | The ✎ pencil: read the box → post the edit request. |
| `src/view/free-text-layer.tsx` | Claim the request → open the editor pre-filled → commit via `updateFreeText`. |
| `src-tauri/tests/free_text_edit.rs`, `tests/cos.rs`, FE `freetext`/`free-text-layer`/`AnnotationPanel` | Style round-trip, `/NM`-preserving update, and the UI wiring. |

#### Further reading

- PDF 32000-1:2008 §12.7.3.3 (`/DA` default-appearance string grammar).
- Round-trip / property-based testing: write-then-read should be the identity.
- Mediator/one-shot request channels between distant React subtrees.

---

### P3.C1b₁ — line + arrow annotations (a points-based shape)

#### Problem

C1a's shapes were bounding-box based (`/Square`, `/Circle` from a `/Rect`). A
line isn't a box — it's two points — and an arrow needs a head. This is the first
*points-based* annotation, and it splits the "shapes" feature by **gesture**.

#### Concepts learned

- **The PDF `/Line` annotation.** `/L [x1 y1 x2 y2]` is the geometry; `/LE` is the
  pair of **line endings** (`[/None /OpenArrow]` puts an open arrowhead at the
  end). We still generate an `/AP` so it looks identical everywhere — the endings
  are advisory; the `/AP` is what draws.
- **Arrowhead geometry.** Given the segment, the unit direction `u` and its
  perpendicular `p` place the two base corners: back off `headLen` along `u` from
  the tip, then ±`headWidth/2` along `p`. The same math runs twice — in Rust for
  the `/AP` *and* in the SVG preview — so the live drag and the committed result
  match.
- **`/BBox` must contain the whole drawing.** A form XObject clips to its BBox, so
  for a line it has to cover the segment **plus** the arrowhead **plus** half the
  stroke width — not just the two endpoints. Forgetting the arrowhead/stroke pad
  is the classic "the tip got clipped" bug.
- **Splitting by interaction, not just data.** Line/arrow are a *drag* (press =
  start, release = end) — the exact lifecycle C1a's rect/ellipse use, so they slot
  in via a `makeLineTool` reducer. Polygon is a *multi-click* (add vertex… finish)
  — a different state machine. Recognizing that the gesture, not the geometry, is
  the dividing line is why C1b split into line/arrow now and polygon later.
- **Extending a draft union end-to-end.** A new `line` draft member rippled
  through: the tool reducer produces it, the `annotation-layer` renders it (a new
  `LineShape` branch, since `Shape` only knows rects) and commits it (`addLine`),
  and the sidebar lists it (`annotation_kind` `/Line → "line"`, `AnnotationKind +=
  "line"`). The compiler walks you through every site — exhaustive unions are a
  to-do list.
- **Reuse compounds.** Persisting (`/NM` + `/AP` + `bumpEpoch` → canvas), listing
  (`read_annotations`), and deleting (the `/NM` handle) all came for free from
  B/C/D — adding a kind is now mostly the new geometry.

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/src/pdf/cos.rs` | `add_line` + `arrowhead_points` + `line_appearance_content`; `/Line → "line"` kind. |
| `src-tauri/src/pdf/{annotation,actor}.rs`, `commands/pdf.rs`, `lib.rs` | `LineEdit` + `AddLine` + `pdf_add_line`. |
| `src/tools/shapes/line-tools.ts` | `lineTool` + `arrowTool` (drag reducers). |
| `src/ipc/lines.ts` | `addLine` wrapper. |
| `src/view/annotation-layer.tsx` | Register the line tools; render the `LineShape` draft; commit via `addLine`. |
| `src/tools/_framework/types.ts`, `MarkupToolbar.tsx`, `ipc/annotations.ts`, `annotation-filter.ts` | `LineAnnotation` + Line/Arrow toggles + the sidebar kind/label. |
| `src-tauri/tests/lines.rs`, `tests/cos.rs`, FE `line-tools`/`lines`/`annotation-layer` | Round-trips, the arrowhead `/LE`+`/AP`, and the drag/commit wiring. |

#### Further reading

- PDF 32000-1:2008 §12.5.6.7 (Line annotations) + Table 176 (`/LE` line-ending styles).
- Arrowhead construction from a direction vector + its perpendicular.
- Discriminated-union exhaustiveness as a refactoring guide.

---

### P3.C1b₂ — polygons (a multi-click gesture, the first non-drag tool)

#### Problem

Every drawing tool so far was a single drag (down→move→up). A polygon is the
first **multi-click** shape — click each vertex, double-click to finish. That
doesn't fit the drag lifecycle at all, which forced an architecture choice.

#### Concepts learned

- **When NOT to generalize the framework.** The generic `stepTool` lifecycle is a
  drag state machine. The tempting move was to extend the tool *contract* with
  multi-click events — a new framework pattern. Instead, following the precedent
  set by notes (click-to-place) and free-text (drag+editor), the polygon got its
  **own self-contained overlay** (`PolygonLayer`) that owns the gesture directly.
  No contract change, no architecture-doc gate, lower risk. Generalize only when a
  *third* multi-click tool actually arrives — not on the first.
- **The `/Polygon` + `/PolyLine` annotations.** `/Vertices [x1 y1 x2 y2 …]` is the
  geometry; a polygon is closed (and fillable), a polyline open. The `/AP` is the
  same path either way — `m` to the first vertex, `l` to each next — with `h`
  (closepath) for the polygon, then `B`/`f`/`S` to fill-and-stroke / fill / stroke.
- **Multi-click gesture mechanics.** Each `pointerdown` appends a vertex; a
  `pointermove` updates a rubber-band edge to the cursor (plus a faint closing edge
  back to the first vertex). The classic trap: a **double-click fires two
  `pointerdown`s**, so the finish would add a stray duplicate vertex — defused by
  *deduping* a click that lands within a few px of the last vertex. Enter finishes,
  Esc cancels, and those key handlers are bound **only while a draft is in
  progress** so they don't hijack the page otherwise.
- **Vertices in document space.** The vertices are stored in PDF points (not screen
  px) so they survive a scroll or zoom mid-draw; only the live cursor is in screen
  space. Convert on click in, on render out.
- **Spec honesty on `/PolyLine`.** P3-ANN-004 says "polygons," not "polylines." The
  backend supports open polylines via a `closed` flag (and it's cos-tested), but
  the **UI exposes only Polygon** — shipping the unspec'd open variant would be
  silently extending the spec. The flag makes it a one-line follow-up once the spec
  gains the word.

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/src/pdf/cos.rs` | `add_polygon` (+ `polygon_appearance_content`); `/Polygon`/`/PolyLine` kinds. |
| `src-tauri/src/pdf/{annotation,actor}.rs`, `commands/pdf.rs`, `lib.rs` | `PolygonEdit` + `AddPolygon` + `pdf_add_polygon`. |
| `src/view/polygon-layer.tsx` | The self-contained multi-click overlay (vertices, rubber-band, Enter/Esc, commit). |
| `src/ipc/polygons.ts` | `addPolygon` wrapper. |
| `src/view/PageVirtualizer.tsx`, `MarkupToolbar.tsx`, `ipc/annotations.ts`, `annotation-filter.ts` | Mount the layer; Polygon toggle + fill; sidebar kind/label. |
| `src-tauri/tests/polygons.rs`, `tests/cos.rs`, FE `polygons`/`polygon-layer` | Round-trips, open-vs-closed, and the gesture wiring. |

#### Further reading

- PDF 32000-1:2008 §12.5.6.9 (Polygon / Polyline annotations) + §8.5.3.1 (closepath `h`).
- "Don't add an abstraction until the third case" — the rule of three for framework changes.
- Pointer vs. double-click event ordering in the DOM (why finish dedupes).

---

### P3.C2 — freehand ink (smoothing + a variable-width ribbon)

#### Problem

A pen tool. The user drags; we capture a stream of pointer samples and store them
as a PDF `/Ink` annotation "with smoothing applied" and pressure support. Two
sub-problems hide here: the raw input is noisy and unevenly spaced (it must be
*smoothed*), and pressure has to become something a PDF can actually draw.

#### Concepts learned

- **Where smoothing belongs: the frontend.** A pointer device emits a jittery,
  variable-rate point stream. The roadmap (and the named test) put smoothing in
  the frontend, which also keeps the Rust `/AP` writer dumb. The pipeline is
  **simplify → resample**: `simplify` drops any sample within ~1pt of the last
  kept one (removing high-frequency tremor while preserving the endpoints), then
  `catmullRomResample` lays an even, dense spline through the survivors.
- **Catmull-Rom splines.** An *interpolating* cubic — the curve passes *through*
  every control point (unlike a Bézier, which only touches its endpoints). Each
  segment uses four controls `P0..P3`; endpoints are clamped by duplicating the
  first/last point. A neat property of the uniform (tension-½) basis: at `t=0` it
  returns `P1` and at `t=1` it returns `P2` *exactly*, so resampling never drifts
  off the captured path. Pressure is interpolated **linearly** (a Catmull-Rom on
  pressure could overshoot out of `[0,1]`).
- **Pressure → geometry: a filled ribbon, not a stroked line.** A PDF stroke
  (`S`) is constant-width; it can't taper. To make pressure visible we instead
  *fill* a ribbon: walk the centreline, offset each point by ±`halfWidth·f(p)`
  along the local **normal** (the unit perpendicular, averaged across a vertex's
  two adjacent segments), and fill the `left… + right(reversed)` outline. Because
  it's just a fill it renders identically in every viewer, and a uniform pressure
  (a mouse reports a constant `0.5`) collapses to a constant-width band — so the
  "ignored otherwise" case falls out for free instead of being a separate path.
- **The BBox-clips-the-AP gotcha, again.** A form `/AP` clips to its `/BBox`. The
  ribbon reaches ±`maxHalfWidth` past the centreline, and at full pressure that
  exceeds the base stroke width — so padding the BBox by `width` (as the line /
  polygon writers do) would shave a hard press. The pad must be the **max**
  half-width actually present in the stroke. A unit test (`heavy` Rect taller than
  `light`) pins this.
- **Degenerate normals.** A zero-length segment has no direction, so no normal —
  dedupe coincident samples first, and carry the last valid segment normal across
  a backtrack so a sharp reversal doesn't drop a point.
- **Drag ≠ the drag lifecycle.** `stepTool` models a drag as *start + end* (rect,
  line). Ink is also a drag, but it needs *every* intermediate sample plus
  per-sample pressure — so, like `PolygonLayer`, it's a self-contained `InkLayer`
  that owns pointer-capture and accumulates the path itself. (The rule of three
  holds: still no shared "stream capture" lifecycle in the framework.)

#### Files in this step

| File | Role |
|---|---|
| `src/tools/ink/ink.ts` | Pure smoothing: `simplify` + `catmullRomResample` + `smoothInk`. |
| `src/view/ink-layer.tsx` | Self-contained drag overlay: capture, dedup, preview, smooth-on-release, commit. |
| `src/ipc/ink.ts` | `addInk` wrapper. |
| `src-tauri/src/pdf/cos.rs` | `add_ink` (+ `ink_appearance_content` ribbon, `ink_half_width`, `segment_normal`, `dedupe_ink_points`); `/Ink` kind. |
| `src-tauri/src/pdf/{annotation,actor}.rs`, `commands/pdf.rs`, `lib.rs` | `InkEdit` + `AddInk` + `pdf_add_ink`. |
| `src/view/PageVirtualizer.tsx`, `MarkupToolbar.tsx`, `ipc/annotations.ts`, `annotation-filter.ts` | Mount the layer; Pen toggle; sidebar kind/label. |
| `src-tauri/tests/ink.rs`, `tests/cos.rs`, FE `smoothing`/`ink`/`ink-layer` | Round-trip, pressure-widens-ribbon, the smoothing maths, and the gesture wiring. |

#### Further reading

- PDF 32000-1:2008 §12.5.6.4 (Ink annotations, `/InkList`).
- Catmull-Rom splines (interpolating cubics) and the tension-½ uniform basis.
- "Stroke outlining" / variable-width strokes as filled ribbons (vs. a constant-width `S`).
- `PointerEvent.pressure` and `setPointerCapture` (why a mouse reads `0.5`).

---

### P3.C2 — the in-app verification sweep (four bug fixes)

#### Problem

C2 shipped green on tests but the human's in-app pass surfaced four real UI bugs
the unit tests couldn't see. Each is a worthwhile lesson on its own.

#### Concepts learned

- **`z-index` defeats DOM order.** PDF.js's text-layer spans carry `z-index: 1`.
  An element with a positive `z-index` paints **above** a sibling with
  `z-index: auto` regardless of who comes later in the DOM — so the transparent
  text spans sat on top of the (later-in-DOM) annotation overlay, stealing the
  cursor and starting a native text selection while you drew. The fix isn't more
  z-index whack-a-mole: while a *drawing* tool is active the text layer just goes
  `pointer-events: none`. (Toggle it via `className`, not an inline `style` prop —
  the layer sets its `width`/`height` imperatively, and a React-managed `style`
  would clobber them on the next render.)
- **React keys must be unique among *siblings*, and a clash is silently
  destructive.** `ThumbnailPanel` and `AnnotationPanel` are siblings and both used
  `key={documentId}`. React's warning is explicit: non-unique keys "may cause
  children to be **duplicated** and/or omitted." It duplicated the Pages sidebar,
  and the orphaned DOM lingered until a full reload — which sent me chasing an
  *HMR ghost* red herring before the console's `two children with the same key`
  pinpointed it. Lesson: when a panel mysteriously duplicates, read the console
  for a key warning before theorising about the bundler.
- **`new Date("YYYY-MM-DD")` is UTC; `new Date(y, m-1, d)` is local.** The date
  filter parsed the picker value as UTC midnight, so in a zone ahead of UTC
  "modified on or after today" excluded annotations stamped earlier *today*.
  Build the date from parts to pin it to the local day the user picked. (And a
  filter you can set but not clear is a trap — add the ✕.)
- **Don't refactor the core render lifecycle blind.** The "refresh flash" on every
  edit is real — PdfViewer `setDoc(null)`s on each epoch bump, unmounting the
  whole page view. The fix (keep the doc mounted and swap a freshly-loaded one in)
  is sound *in principle*, but my attempt skewed the page-geometry/scale timing
  and rendered shapes at the wrong spot/aspect (ovals → circles). With no way to
  watch the result iterate, the right call was to **revert to the known-good
  render path** and defer — a correct-but-flashy page beats a smooth-but-wrong
  one. Geometry-sensitive lifecycle changes need eyes on the pixels.

#### Files in this step

| File | Role |
|---|---|
| `src/view/text-layer.tsx` | `toolUsesTextSelection`; disable the text layer while drawing. |
| `src/view/ink-layer.tsx` | `preventDefault` + touch-action/user-select none on the gesture. |
| `src/view/PdfViewer.tsx` | Distinct sibling keys for the two panels. |
| `src/panels/annotation-filter.ts` | `dateInputToMs` / `msToDateInput` (local-time date parsing). |
| `src/panels/AnnotationPanel.tsx` | Controlled date input + ✕ clear. |
| `src/view/polygon-layer.tsx` | Click-first-vertex to close; abandon on tool switch. |

#### Further reading

- CSS stacking contexts & painting order (why `z-index: 1` beats `auto`).
- React "Rendering Lists" — keys are scoped to siblings; collisions are UB.
- `Date` parsing: ISO date-only strings are UTC, date-time without offset is local.

---

### P3.C3a — stamp library + custom text stamps

#### Problem

A rubber-stamp tool: a library of built-ins (APPROVED, CONFIDENTIAL, …) plus
custom text, dropped on the page with a click. The spec line (P3-ANN-006) also
wants *image* stamps — a separate capability (image embedding) we split into C3b.

#### Concepts learned

- **A `/Stamp` is whatever its `/AP` draws.** PDF defines standard stamp `/Name`s
  (Approved, Draft, …), but viewers render them inconsistently — so, like every
  other annotation we author, we supply our own appearance and treat `/Name` as
  informational. The `/AP` here = a stroked border (`re` `S`) + a centred line of
  bold text (`BT … Tj … ET`), reusing the free-text font-resource pattern (a
  self-contained `/Resources /Font /F1` so display doesn't need an AcroForm `/DR`).
- **Centring text without a metrics table.** Exact glyph widths need the font's
  metrics; for one centred line we don't need them. A single average-advance
  constant (~0.62 em for Helvetica-Bold) is enough to *estimate* the label width,
  pick a font size that fits the box (min of width- and height-fit), and centre it
  — `tx = x0 + (boxW − textW)/2`, baseline at the box mid-line minus ~0.34·size
  (half the cap height). Good enough for a stamp; the `/BBox` clips any overflow.
- **A cross-subtree "armed selection" channel.** The palette (in the toolbar) and
  the placement layer (deep in the page tree) are far apart. Rather than
  prop-drill, a tiny zustand `stamp-store` holds the *armed* stamp: the palette
  writes it, `StampLayer` reads it. Same pattern as the annotation-selection /
  edit stores — a one-slot store as a decoupled request channel.
- **Click-to-place is its own gesture (again).** Like the note layer, a stamp is a
  single click, not a drag — so `StampLayer` is another self-contained overlay
  (the framework's `stepTool` is still drag-only; rule of three holds at *four*
  self-contained layers now: note, polygon, ink, stamp — a shared "click/multi-
  click lifecycle" is overdue and noted in BACKLOG).
- **Disarm on tool-change.** The polygon-rubber-band lesson, applied up front: the
  toolbar clears the armed stamp whenever the active tool leaves `"stamp"`, so a
  later click with another tool can't drop a stale stamp.

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/src/pdf/cos.rs` | `add_stamp` (+ `stamp_appearance_content`, `sanitize_stamp_name`); `/Stamp` kind. |
| `src-tauri/src/pdf/{annotation,actor}.rs`, `commands/pdf.rs`, `lib.rs` | `StampEdit` + `AddStamp` + `pdf_add_stamp`. |
| `src/tools/stamp/stamps.ts` | Built-in library + `stampRectAt` placement maths (pure). |
| `src/tools/stamp/StampPalette.tsx`, `src/state/stamp-store.ts` | Pick/arm a stamp. |
| `src/view/stamp-layer.tsx`, `src/ipc/stamps.ts` | Click-to-place overlay + `addStamp`. |
| `src-tauri/tests/stamp.rs`, `tests/cos.rs`, FE `stamps`/`stamp-layer` | Round-trip, name-sanitize, placement, gesture. |

#### Further reading

- PDF 32000-1:2008 §12.5.6.20 (Rubber-stamp annotations, standard `/Name`s).
- Base-14 font metrics / average character advance (why 0.62 em works for centring).
- Text showing operators: `BT`/`ET`, `Tf`, `Td`, `Tj`.

---

### P3.C4a — measurement tools + calibration

#### Problem

Distance / perimeter / area tools that report a real-world value — which needs a
*calibration* (the page has no inherent scale) and a way to show the number.

#### Concepts learned

- **Calibration is a ratio, captured by example.** A PDF page is just points; to
  report metres you need "this many points = this real length". The usable UX is
  *draw a reference, then type its size* — `scale = realLength / referencePoints`
  (units per point). The headline feature is one division; the rest is plumbing.
- **Area scales by the square of the length scale.** Distance/perimeter scale
  linearly (`points · scale`); area is two-dimensional, so it scales by `scale²`
  (50pt = 1m ⇒ 2500pt² = 1m²). Easy to get wrong; pinned by a test.
- **The shoelace formula** gives a polygon's area from its vertices —
  `½|Σ(xᵢyᵢ₊₁ − xᵢ₊₁yᵢ)|`. It's *signed* (encodes winding), so `abs()` it; it's
  also only valid for a simple (non-self-intersecting) ring — documented as a limit.
- **Reuse the subtype, distinguish by intent.** A measurement isn't a new PDF
  annotation type — it's a `/Line`/`/PolyLine`/`/Polygon` carrying a dimension
  **`/IT`** (`LineDimension`/…). So the geometry + `/AP` reuse the shape writers;
  the only new bits are the `/IT` tag and a value label baked into `/Contents` +
  the `/AP`. Read-back keys off `/IT` to surface it as "measure", not the bare
  shape — the inverse of how we *write* it. (Acrobat's live `/Measure` dict, which
  would let it re-measure from raw scale, is deferred to C4b; our baked label
  renders everywhere meanwhile.)
- **One overlay, two jobs, via a store flag.** Calibration and measurement are
  the *same* 2-click gesture; a `calibrating` flag in the measure-store switches
  `MeasureLayer` between "stash the reference length for the dialog" and "persist
  a measurement". And distance **auto-finishes at 2 clicks** (no double-click) by
  finishing inline once the new vertex count hits 2 — a small per-mode tweak on
  the shared multi-click gesture.

#### Files in this step

| File | Role |
|---|---|
| `src/tools/measure/measure.ts` | Pure maths: calibration + distance/perimeter/area + format. |
| `src/state/measure-store.ts` | Mode + per-doc calibration + the calibrate handshake. |
| `src/view/measure-layer.tsx` | Multi-click overlay (distance auto-finish, area close, calibrate). |
| `src/tools/measure/{MeasureControls,CalibrateDialog}.tsx` | Mode/calibrate UI + the scale dialog. |
| `src-tauri/src/pdf/cos.rs` | `add_measure` (+ `measure_appearance_content`, `is_measurement_intent`). |
| `src-tauri/src/pdf/{annotation,actor}.rs`, `commands/pdf.rs`, `lib.rs` | `MeasureEdit` + `AddMeasure` + `pdf_add_measure`. |
| `src-tauri/tests/measure.rs`, `tests/cos.rs`, FE `calibration`/`measure-layer` | Round-trip, `/IT` read-back, the maths, the gesture. |

#### Further reading

- The shoelace (surveyor's) formula for polygon area.
- PDF 32000-1:2008 §12.5.6.10 + §12.9 (measurement annotations, the `/Measure` dict).
- Why area scales as the square of a linear scale (dimensional analysis).

---

### P3.D2 — reply threads

#### Problem

Reply to any annotation, with the replies threaded under it — the basic comment
collaboration loop, persisted in the PDF so other readers see the thread.

#### Concepts learned

- **A reply isn't a new type — it's a link.** Per the PDF spec a reply is just a
  markup annotation (we use `/Text`, like a note) carrying **`/IRT`** (*in reply
  to* — a reference to the parent) and `/RT /R` (reply-type = reply). So the write
  reuses the note machinery; the only new bits are the two keys. Resolving them on
  read is the inverse: dereference `/IRT` to the parent dict and read *its* handle.
- **Two filters keep replies in their lane.** A reply is a `/Text`, so without
  care it leaks in two places: the page note-overlay (`read_text_notes`) would
  draw it as a stray icon, and the sidebar would list it as a standalone row. Both
  read paths now check `/IRT`: `read_text_notes` skips replies entirely; the
  sidebar nests them under their parent instead of listing them top-level. The fix
  for "where does this annotation belong?" lives at the *read* boundary, once.
- **Threading is tree-walking with guards.** `buildThreads` finds each reply's
  thread **root** by walking `inReplyTo` upward, then flattens all descendants
  under that root (Acrobat shows one indent level, chronological — simpler than
  arbitrary nesting). Real PDFs are hostile: a reply whose parent was deleted
  (**orphan**) must still show (treat it as its own root, don't drop data), and a
  malformed **cycle** (`a→b→a`) must not infinite-loop (a `seen` set caps the
  walk; each node resolves to self-as-root). Pure + unit-tested in isolation.
- **Reuse the handle, don't invent IDs.** The reply's parent is addressed by the
  *same* `/NM`-or-`obj:` handle the sidebar already uses for select/delete — so
  `add_reply` shares the resolver (`resolve_handle`) with `delete_annotation`, and
  the existing ✕ deletes a reply for free (it has its own `/NM`). One identity
  scheme across select / delete / reply.

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/src/pdf/cos.rs` | `add_reply` + `/IRT` resolution on read (`annot_handle`/`irt_handle`); `read_text_notes` skip. |
| `src-tauri/src/pdf/{annotation,actor}.rs`, `commands/pdf.rs`, `lib.rs` | `ReplyEdit` + `AddReply` + `pdf_add_reply`. |
| `src/ipc/replies.ts`, `src/ipc/annotations.ts` | `addReply` + `AnnotationInfo.inReplyTo`. |
| `src/panels/annotation-filter.ts` | `buildThreads` (root-walk + flatten, cycle/orphan-safe). |
| `src/panels/AnnotationPanel.tsx` | Nested replies + inline composer; filters on roots. |
| `src-tauri/tests/reply_thread.rs`, `tests/cos.rs`, FE `buildThreads`/`replies`/`AnnotationPanel` | Link, read-back, thread maths, send. |

#### Further reading

- PDF 32000-1:2008 §12.5.6.2 + Table 174 (`/IRT`, `/RT`, reply markup annotations).
- Tree/graph traversal with a visited-set to survive cycles.

---

### P3.E1 — XFDF import / export (annotation interchange)

#### Problem

Markup is only useful if it can leave the app. **XFDF** (XML Forms Data Format)
is the file Acrobat and other readers use to ship annotations *separately* from
the PDF — a reviewer exports their comments, emails the small XML, and the author
imports them onto their own copy. P3-ANN-010 asks for a round-trip: export every
annotation, re-import it, get it back identically.

#### Concepts learned

- **A sidecar format, not a PDF.** XFDF is plain XML: an `<annots>` list where
  each element (`<highlight>`, `<square>`, `<ink>`, `<text>`, …) carries the
  annotation's geometry + style as attributes (`rect`, `color`, `coords`,
  `vertices`, …) and its text as a `<contents>` child. Same data a PDF annotation
  dict holds, re-encoded as XML. Our exporter walks the raw lopdf dicts; the
  element name is just the lowercased `/Subtype`.

- **"Restored identically" forces a real read model.** The sidebar's
  `AnnotationInfo` is deliberately thin (no colour, no `/QuadPoints`, no
  `/InkList`). You can't round-trip geometry through it. So export reads the
  **raw dicts**, and import had a choice: rebuild dicts by hand, or **reuse the
  canonical `add_*` writers**. We reused them — an imported highlight runs the
  exact same `/AP`-and-`/BBox` code as a drawn one, so it renders identically in
  every reader, and there's no second copy of the appearance maths to drift.

- **Identity is patched, not regenerated.** `add_*` mints a fresh `/NM` and
  doesn't carry `/Contents`/`/T`. After each add we find the *one new* `/Annots`
  entry (set difference of object-ids before/after — exact, since lopdf preserves
  ids across a load→save) and overwrite `/NM`, `/Contents`, `/T`, and the dates
  from the XFDF. Preserving `/NM` is what lets **reply threads survive**: a reply's
  `/IRT` points at its parent *by name*, so a two-pass import (all non-replies
  first, then replies whose parent name now exists, fixed-point for reply-to-a-reply)
  re-wires the threads. The whole import is **one undoable edit** (a single byte
  snapshot inverse) — one ⌘Z reverses the lot.

- **Hand-rolled XML, no dependency.** Rather than add a parser crate (the project
  is dependency-averse), the reader is a ~150-line char-cursor pull parser for the
  XFDF subset: elements, attributes, self-closing tags, the five named entities +
  numeric (`&#65;`/`&#x41;`), comments/PIs/DOCTYPE skipped, CDATA handled. It's
  **lenient on import** (numeric lists split on comma *or* semicolon *or*
  whitespace, so Acrobat's quirks parse) and **strict on output** (we emit clean
  comma-separated lists Acrobat reads). Malformed input fails cleanly — never
  panics — which the unit tests pin down by feeding it garbage.

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/src/pdf/xfdf.rs` | `annotations_to_xfdf` (raw dicts → XML) + `import_xfdf` (parse → reuse `add_*` → patch identity → wire `/IRT`) + the XML reader. |
| `src-tauri/src/pdf/{annotation,actor}.rs`, `commands/pdf.rs`, `lib.rs` | `ImportXfdfEdit` + `ExportAnnotations`/`ImportXfdf` messages + `pdf_export_annotations`/`pdf_import_annotations`. |
| `src/ipc/interchange.ts` | `exportAnnotations` / `importAnnotations` typed wrappers. |
| `src/panels/AnnotationPanel.tsx` | ⬆/⬇ header actions → native save/open dialogs → IPC → epoch reload. |
| `src-tauri/tests/xfdf_roundtrip.rs`, `xfdf.rs` units, FE `interchange`/`AnnotationPanel` | Full byte round-trip, parser edge cases, IPC marshalling, UI wiring. |

#### Further reading

- Adobe XFDF 3.0 spec (the `<xfdf>`/`<annots>` schema + per-subtype attributes).
- PDF 32000-1:2008 §12.7.7 (FDF/XFDF) and §12.5 (annotation dictionaries).
- Recursive-descent / pull parsing of a constrained grammar.

---

### P3.E2 — Flatten annotations (baking markup into the page)

#### Problem

An annotation is a *separate* object layered over the page — editable, deletable,
sometimes ignored by basic viewers or printers. **Flattening** burns it into the
page's own content so it's permanent and universal: a printed-looking PDF where
the highlight is just part of the page, not a thing you can click and delete.
P3-ANN-011 also pins the undo semantics — reversible *in the app*, gone once you
save and reopen.

#### Concepts learned

- **An annotation already carries its own picture.** Every annotation we draw has
  an **appearance stream** (`/AP /N`) — a self-contained **form XObject** (a
  reusable mini content stream with its own `/BBox` + `/Resources`). Flattening
  doesn't *re-draw* anything; it **replays** that existing form into the page. So
  the core move is three operators appended to the page content:
  `q <matrix> cm /Name Do Q` — push state, set the placement transform, **`Do`**
  (paint the named XObject), pop. `add_xobject` registers the existing form under
  the page's `/Resources /XObject` so `/Name` resolves; then we drop the
  annotation from `/Annots`. The form survives `prune_objects` because the page
  resources now reference it.

- **The placement matrix (PDF §12.5.5).** A form draws in its *own* coordinate
  space (its `/BBox`, possibly skewed by its `/Matrix`); the annotation says where
  on the page it goes (its `/Rect`). The `cm` matrix bridges them: transform the
  BBox by the form Matrix, take that box's bounds, then scale+translate it onto
  the Rect. For **our** annotations it's the identity (we author `BBox == Rect`,
  no Matrix) — but doing the general transform means foreign annotations flatten
  correctly too.

- **Don't decode what you don't have to.** We append a *new* content stream
  referencing the forms rather than parsing + re-encoding the page's existing
  content (which can contain operators a decoder chokes on). Same lesson the
  resize edit learned. Each fragment is its own balanced `q…Q`, so one form's
  graphics state can't leak into the next or into the page.

- **Resource inheritance is a trap.** A page can *inherit* `/Resources` from the
  `/Pages` tree instead of owning them. lopdf's `get_or_create_resources` creates
  an **empty** dict when the page has none — which would **shadow** the inherited
  fonts/images and break the existing content. So before adding the XObject we
  clone the effective resources down onto the page when it lacks its own.

- **Undo is the snapshot, not the inverse-op.** Flatten reuses the same
  `cos_edit` wrapper as every byte transform: the inverse is a **pre-flatten copy
  of the bytes** (`RestoreDocEdit`). That's *why* the spec's "undoable in-session,
  not from a saved file" falls out for free — the snapshot lives in the session's
  history, never in the file. `/AP`-less notes have no picture to bake, so they're
  left as live annotations (flatten = bake *visible* markup).

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/src/pdf/flatten.rs` | The COS transform: per page, register `/AP` forms (`add_xobject`), append `q cm Do Q` fragments, drop annots, prune; §12.5.5 matrix; inherited-resource guard. |
| `src-tauri/src/pdf/{annotation,actor}.rs`, `commands/pdf.rs`, `lib.rs` | `FlattenEdit` + `FlattenAnnotations` message + `pdf_flatten_annotations`. |
| `src/ipc/flatten.ts` | `flattenAnnotations` typed wrapper. |
| `src/panels/AnnotationPanel.tsx` | ▦ header action behind an inline confirm (permanent-after-save warning); skips `/AP`-less notes via the client-side count. |
| `src-tauri/tests/flatten_annotations.rs`, `flatten.rs` units, FE `flatten`/`AnnotationPanel` | Structural bake + undo + notes-kept; matrix maths; IPC + UI. |

#### Further reading

- PDF 32000-1:2008 §12.5.5 (appearance streams + the BBox→Rect algorithm) and §8.10 (form XObjects, the `Do` operator).
- PDFium's `FPDFPage_Flatten` — the native equivalent we deliberately didn't use (live-handle + unsafe-FFI, against the cos-on-bytes architecture).

---

### P3.C4b — the `/Measure` dictionary (live re-measure interop)

#### Problem

C4a let you calibrate a scale and drew the measured value as a *label*. But that
label is just baked text — open the PDF in Acrobat and its measuring tool reports
the raw point distance, not "4 m," because nothing in the file says *what one
point is worth*. C4b writes that scale in the form readers understand, and makes
the calibration survive a save/reopen.

#### Concepts learned

- **A `/Measure` dict is the scale, machine-readable.** PDF §12.9: a rectilinear
  (`/Subtype /RL`) `/Measure` dictionary attached to the annotation carries
  `NumberFormat` arrays — `/X` (axis: page-units → real units), `/D` (distance
  display), `/A` (area display). The whole calibration is one number: `/X[0] /C`
  = real units per point. `/D 100` means "round to 1/100" (our 2-dp). Area works
  because the reader squares the X scale — matching our `polygonArea · upp²`. Now
  Acrobat re-measures *live* off the geometry; our baked `/Contents` label is just
  the fallback.

- **Persisting state = writing it into the artifact, then reading it back.** The
  calibration lived only in a zustand store (gone on reopen). The fix isn't a
  separate settings file — it's that **the `/Measure` dict already *is* the
  persisted calibration**. So "persist across reopen" became: thread the scale to
  the writer (`add_measure` gained `units_per_point` + `unit`), and add the
  *inverse* read (`read_measure_calibration` scans for the first `/Measure`,
  returns `/X /C` + `/U`). A small `useCalibrationSync` hook seeds the store from
  it on open — **guarded** so it never clobbers a calibration the user set this
  session (it reads the live store imperatively rather than depending on it, so the
  effect doesn't re-fire on every calibration change).

- **PDF text strings are byte-encoded, so keep labels ASCII.** The on-screen `²`
  lives in our `/AP` (WinAnsi font, where `²` is one byte). But a `/Measure` `/U`
  unit label is a PDF *string*; a UTF-8 `²` (two bytes) would render wrong without
  UTF-16 machinery. So the area unit in `/Measure` is ASCII (`sq ft`) — a
  deliberate, documented divergence from the displayed label.

- **Widening a writer ripples through every caller.** Adding two params to
  `add_measure` broke its callers across the layers (the `MeasureEdit`, the actor
  message + handler, the command, the XFDF importer, and six test call-sites). The
  compiler walks you through each — a reminder that an IPC arg list is a contract
  with N call sites, and "just add a param" is never just one edit.

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/src/pdf/cos.rs` | `add_measure` +`units_per_point`/`unit`; `measure_dict` (RL + NumberFormats); `MeasureCalibration` + `read_measure_calibration`. |
| `src-tauri/src/pdf/{annotation,actor}.rs`, `commands/pdf.rs`, `lib.rs` | `MeasureEdit` fields + `AddMeasure`/`ReadMeasureCalibration` messages + `pdf_read_measure_calibration`. |
| `src/ipc/measure.ts` | `addMeasure` +calibration; `readMeasureCalibration`. |
| `src/view/measure-layer.tsx`, `src/tools/measure/{MeasureControls,use-calibration-sync}` | Pass the calibration at write; re-seed the store on open (no clobber). |
| `measure.rs` / `cos.rs` / `measure` IPC / `use-calibration-sync` tests | `/Measure` shape, calibration round-trip, IPC marshalling, the seed guard. |

#### Further reading

- PDF 32000-1:2008 §12.9 (Measurement Properties — `/Measure`, `/NumberFormat`, the `/X`/`/D`/`/A` arrays) and §7.9.2.1 (string object encoding).

---

### P3.C3b — image stamps (embedding a raster into a PDF)

#### Problem

C3a stamped *text* (a drawn label). C3b stamps an *image* — a signature, a logo, a
company seal — which means getting raw pixels **into** the PDF as something a
renderer can paint, with transparency intact.

#### Concepts learned

- **An image is just another XObject.** Same `Do` operator as flatten's form
  XObjects, but the XObject is `/Subtype /Image` instead of `/Form`: a stream of
  raw pixel bytes plus a dictionary saying `/Width`, `/Height`, `/ColorSpace`,
  `/BitsPerComponent`. The stamp `/AP` paints it with `q <w> 0 0 <h> <x> <y> cm
  /Im0 Do Q` — the `cm` maps the image's **unit square** (the space `Do` draws an
  image into) onto the placement rect.

- **Transparency is a second image: the soft mask.** A PDF image's colour data has
  *no* alpha. Transparency lives in a separate grayscale `/SMask` image (0 =
  transparent, 255 = opaque). So an RGBA PNG is **split**: the RGB bytes become the
  colour image, the A bytes become the `/SMask`. De-interleaving `RGBARGBA…` into
  `RGBRGB…` + `AA…` is the whole trick.

- **The dependency you have is bigger than the one you use.** The plan flagged a
  PNG-decoder dependency — but the `png` crate we'd added *just for the render
  encoder* ships a full `Decoder` in the same crate. The Cargo comment said
  "encoder-only" to describe our *intent*, not a feature gate. Lesson: check what a
  dependency actually exposes before adding another. `normalize_to_color8()`
  (EXPAND | STRIP_16) collapses the PNG zoo — palettes, 1/2/4/16-bit — down to a
  predictable 8-bit RGB(A)/Gray(A), so the embed code handles four cases, not
  twenty.

- **Aspect ratio belongs where the dimensions are known.** The frontend has a file
  *path*, not pixels — so it can't know the image is 2:1. The backend decodes,
  learns the dimensions, and derives the placement rect (height fixed, width =
  height × aspect, clamped to the page). "Aspect-aware placement" = compute the
  rect where you know the aspect, not where you started the gesture.

- **A discriminated union keeps one tool bimodal.** `StampSpec` became
  `{ kind: "text", … } | { kind: "image", … }`; the layer branches on `kind` to
  call `addStamp` (rect) or `addImageStamp` (click point). One tool, one armed
  slot, two commit paths — the type system forces the layer to handle both.

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/src/pdf/image_xobject.rs` | Sniff + decode PNG (via the `png` decoder), build the Image `XObject`, split alpha → `/SMask`. |
| `src-tauri/src/pdf/cos.rs` | `add_image_stamp`: embed, aspect-correct + page-clamped rect, `/AP` (`Do` + optional label), `/Stamp` dict. |
| `src-tauri/src/pdf/{annotation,actor}.rs`, `commands/pdf.rs`, `lib.rs` | `ImageStampEdit` + `AddImageStamp` + `pdf_add_image_stamp` (reads the file path). |
| `src/ipc/stamps.ts`, `src/tools/stamp/stamps.ts`, `StampPalette.tsx`, `src/view/stamp-layer.tsx` | `addImageStamp`; `StampSpec` union + `imageStamp`; the Image… picker; the layer's image branch. |
| `image_xobject` units, `stamp.rs` image cases, FE `stamps`/`stamp-layer` | Decode + SMask split + aspect rect + round-trip + the UI branch. |

#### Further reading

- PDF 32000-1:2008 §8.9.5 (Image XObjects), §11.6.5.2 (soft-mask images / `/SMask`), §8.10.1 (`Do`).
- The `png` crate's `Decoder` + `Transformations::normalize_to_color8`.

---

### P3.B3b — free-text underline, auto-wrap, and double-click re-edit

#### Problem

B3a drew uniform-style free text but only honoured *manual* line breaks and had
no underline, and re-editing meant the sidebar pencil. B3b makes the box behave
like a text box: long lines wrap, you can underline, and double-clicking a box
reopens it.

#### Concepts learned

- **Underline is a drawn rule, not a font property.** Base-14 PDF fonts have no
  "underline" — so after the text (`BT … Tj … ET`) we stroke a thin line under
  each rendered line (`m … l S`, *outside* `BT/ET` since those are path ops). The
  width of each rule is the same estimate used for wrapping. PDF has no standard
  per-annotation "underline" flag either, so it's persisted in a **private
  `/Underline` key** — readers ignore the key but still show the `/AP` rule; only
  *our* re-edit reads the key back. (B3c will move this into the standard `/DS`.)

- **Wrapping needs one source of truth.** The box auto-grows to fit its text — but
  if "how tall" used a different line-count than "what's drawn," the box would clip
  or gap. So `wrap_lines` is computed **once** and consumed by *both*
  `grow_free_text_rect` (height) and the `/AP` content (the drawn lines). Width is
  estimated as `chars × size × em`, with a per-family average em (Courier ≈0.6,
  proportional ≈0.5) — deliberately a slight *under*-estimate so it wraps a hair
  early rather than overflowing the `BBox`-clipped box. Real glyph metrics (AFM)
  would be exact; the estimate is the honest, dependency-free 80%.

- **Hit-testing canvas-drawn annotations.** A committed free-text box renders on
  the PDF.js *canvas*, not as a DOM node — there's nothing to attach a
  double-click to. So the overlay reads the page's free-text rects
  (`read_annotations`, on the edit epoch) and renders a transparent
  `pointer-events:auto` **hit-zone** per box. A child can opt back into pointer
  events even though the parent layer is `pointer-events:none` when idle — so text
  selection still works everywhere except over a box. A double-click on a zone
  posts the *same* edit request the sidebar ✎ already uses — reusing D1e's whole
  read-back → editor flow for free.

- **Widening a 5-call signature is a paper-cut, not a wound, when caught early.**
  Adding `underline` rippled through `add_free_text`/`update_free_text` and their
  edit/actor/command/IPC layers plus ~25 test call-sites. A balanced-paren script
  patched the calls — but **trailing commas in multi-line calls** turned
  `false,\n)` into `false, , false)` (a syntax error the compiler caught
  instantly). The lesson: mechanical edits want a compile gate immediately after,
  not at the end.

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/src/pdf/cos.rs` | `add_free_text`/`update_free_text` +`underline`; `wrap_lines` + `font_avg_em`; underline `/AP` rule; `/Underline` read-back; `FreeTextData` +`underline`. |
| `src-tauri/src/pdf/{annotation,actor}.rs`, `commands/pdf.rs`, `src/ipc/freetext.ts` | Thread `underline` through the edit/actor/command/IPC chain. |
| `src/tools/_framework/types.ts`, `state/tool-store.ts`, `app/MarkupToolbar.tsx` | `options.underline` + the **U** toolbar toggle. |
| `src/view/free-text-layer.tsx` | Underline preview + commit; per-box double-click hit-zones → edit request. |
| `cos.rs` (+2), `freetext` IPC, `free-text-layer` tests | Underline draws + round-trips, long-line wrap + grow, double-click re-edit. |

#### Further reading

- PDF 32000-1:2008 §12.7.4.3 (variable text / `/DA`) and §12.7.3.3 (rich text `/RC`, `/DS` — the deferred B3c).
- Greedy word-wrap; font metrics (AFM widths) vs average-advance estimation.

---

### P4.A1 — text-run extraction (Phase 4 begins)

#### Problem

Phase 4 is *editing existing content* — and you can't edit what you can't locate.
Before "click a typo to fix it," the app must answer: *what text is under this
click, where exactly is it, and how is it styled?* A1 is that read-only lookup —
the foundation the whole text engine (font fallback, redact-and-reflow, the
in-place editor) stands on.

#### Concepts learned

- **A PDF doesn't store "text" — it stores show operators.** There's no
  paragraph model; a page is a stream of `Tf`/`Td`/`Tj`/`TJ` operators that *paint*
  glyphs. PDFium decodes that stream into **text page-objects** — each ≈ one show
  operator — and exposes per-object text, bounds, font, size, colour, and the text
  matrix. So "a text run" = one PDFium text object: the natural, honest edit unit
  (it might be a word, a line, or a glyph, depending on how the authoring tool
  emitted it — we surface PDFium's granularity rather than guessing).

- **A third read pattern.** P3's reads serialized to bytes and parsed with lopdf —
  great for the object model (annotations, the page tree), useless for text, which
  lives *inside* encoded content streams. Decoding that is PDFium's job, so A1
  reads the **live `PdfDocument`** under the shared PDFium lock (the `render.rs`
  pattern), not a byte round-trip. The COS layer now has three read shapes:
  lopdf-byte (structure), render-to-pixels (display), and live-PDFium-structured
  (text). Knowing *which* tool decodes *which* layer is half of working with PDFs.

- **High-level binding > raw FFI.** pdfium-render wraps the C API in safe Rust:
  `page.objects().iter()` → match `PdfPageObject::Text(t)` → `t.text()`,
  `t.bounds()`, `t.font().name()/is_embedded()`, `t.scaled_font_size()`,
  `t.fill_color()`, `t.matrix()`. No `unsafe`, no manual `FPDFText_*` handle
  juggling. Two small frictions: the object iterator yields **owned** wrappers that
  are `Drop`, so you must match by **reference** (`&object`) — moving out of a
  `Drop` type is a compile error; and `pages().get()` wants an `i32` index.

- **Surface honest, useful metadata.** The record carries `embedded` (does the
  file ship the font?) — not needed to *render*, but it's exactly what **A2**'s
  font-fallback decision and the "this edit may not match" warning will hinge on.
  And subset tags (`ABCDEF+Helvetica`, six uppercase letters + `+`) are stripped
  for display — noise from embedded subsetting, not a real family name.

- **Read-only first.** A1 writes nothing — no artifact, no undo, no actor edit;
  just a query. Starting the *hardest* phase with its read half keeps the first
  step low-risk and gives B1 a stable contract to build the lossy write path on.

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/src/pdf/text_extract.rs` | `extract_text_runs(doc, page)` over the live PDFium doc; the `TextRun` struct; subset-tag strip. |
| `src-tauri/src/pdf/{actor}.rs`, `commands/pdf.rs`, `lib.rs` | `ReadTextRuns` read-only query + `pdf_extract_text_runs` (mirrors the render arm — no `save_to_bytes`). |
| `src/ipc/text-runs.ts` | `extractTextRuns` + the `TextRun` type (no UI consumer yet — B1 wires it). |
| `tests/text_extract.rs`, `text_extract.rs` units, `text-runs.test.ts` | Runs from `hello.pdf` (sane bbox/font/size/colour), bad-index error, cross-doc stability; subset-strip + hex; IPC marshalling. |

#### Further reading

- PDF 32000-1:2008 §9.4 (text objects + the text-showing operators) and §9.4.4 (text space / the text matrix).
- pdfium-render's `PdfPageObject` / `PdfPageTextObject` high-level API; PDFium's `FPDFText_*` / `FPDFTextObj_*` C API underneath.

---

### P4.A2 — font fallback resolver (the honesty gate)

#### Problem

Editing text means re-emitting glyphs — but you can only do that *correctly* if you
have the font. A PDF often references a font without **embedding** it (to save
bytes), trusting the reader to already have it. If we edit such a run and that font
isn't on this machine, the new glyphs come out in *some other* font — silently. The
roadmap's hard rule: never silently substitute and pretend it matched. A2 builds the
detector + the warning so editing can be honest before A3 makes editing possible.

#### Concepts learned

- **"Embedded" and "installed" are different guarantees.** Embedded = the glyph
  outlines travel *inside* the file → lossless anywhere. Installed = the face happens
  to be on *this* machine → fine here, maybe not on the next. And a third tier sits
  above both: the **base-14** fonts (Helvetica/Times/Courier + Symbol/ZapfDingbats,
  plus the Arial/Times-New-Roman/Courier-New aliases) every PDF viewer is *required*
  to ship — always safe, never a warning. The resolver buckets each font into
  embedded / standard / system-available / fallback.

- **Pure core, impure shell.** All the *decisions* (`resolve_font`,
  `build_font_report`, the substitute mapping, name normalization) are pure functions
  over an injected `SystemFontIndex` — so every branch is a fast unit test with no
  disk, no PDFium. The one impurity, `load_system_fonts` (a std::fs scan of the OS
  font dirs), is isolated behind a `OnceLock` cache and injected at the boundary.
  This "functional core, imperative shell" split is why the resolver has 8 unit tests
  and the integration test only has to prove the wiring.

- **A heuristic with an honest bias.** Deciding "is this installed?" *precisely* means
  parsing every system font's `name` table for its family — which needs a font crate
  we deliberately don't add (vendor-lock-in > implementation work). Instead we match
  on a **normalized file stem**: lowercase, alphanumerics only, trailing style words
  (`bold`, `mt`, `psmt`, …) peeled off so `Arial-BoldMT` and `Arial Bold.ttf` both
  collapse to `arial`. It's fuzzy, so the bias is chosen: **when unsure, warn** — a
  false warning costs a dismissible banner; a false *all-clear* costs a silent wrong
  glyph. (Gotcha found in testing: `roman`/`book` are family words as often as
  weights — stripping them turned `timesnewroman` into `timesnew`, so they're
  excluded from the suffix list and `Times-Roman` is matched explicitly instead.)

- **Document-level read, once.** Font usage doesn't change when you add an annotation,
  so the banner hook keys on the **document id, not the edit epoch** — fetch the report
  once per open. The actor query (`ReadFontReport`) reuses A1's live-PDFium-under-lock
  path via a lightweight `collect_document_fonts` (distinct `(name, embedded)` only —
  no text/bbox), so it's cheap and never serializes bytes.

- **Ship the *offer*, defer the *action*.** The spec says "offer to re-flow." Re-flow
  is a *write* (A3/B1) that doesn't exist yet — so the banner shows the affordance
  **disabled** with a tooltip. The honest move: surface the capability's existence
  without faking a dead button that does nothing. The step stays `[~]` until B1 wires
  the action and a human eyeballs the banner.

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/src/pdf/font_resolver.rs` | Pure resolver: `FontStatus`/`FontResolution`/`FontReport`, base-14 set, substitute mapping, `normalize_font_key`, `load_system_fonts` (OnceLock dir-scan). |
| `src-tauri/src/pdf/text_extract.rs` | `collect_document_fonts` — distinct `(name, embedded)` across the doc, same live-PDFium read path as A1. |
| `src-tauri/src/pdf/{actor}.rs`, `commands/pdf.rs`, `lib.rs` | `ReadFontReport` read-only query + `pdf_read_font_report` (mirrors the A1 arm). |
| `src/ipc/fonts.ts`, `src/app/use-font-report.ts`, `src/app/FontFallbackBanner.tsx`, `PdfViewer.tsx` | IPC wrapper + once-per-doc hook + the dismissible banner (disabled re-flow affordance), mounted under the toolbars. |
| `tests/font_fallback.rs`, `font_resolver.rs` units, `fonts.test.ts`, `font-fallback-banner.test.tsx` | Hand-built non-embedded Calibri → fallback; `hello.pdf` base-14 → none; cross-doc invariants; 8 resolver units; IPC marshalling; banner render/dismiss. |

#### Further reading

- PDF 32000-1:2008 §9.6.2 (the standard-14 fonts) and §9.8 (font descriptors / `FontFile` embedding).
- "Functional core, imperative shell" (Gary Bernhardt) — the pure-resolver / fs-scan-at-the-edge split.

---

### P4.A3 — in-place text editing (and a library wall)

#### Problem

The headline of Phase 4: change the words in an existing PDF without wrecking it.
A1 located the runs, A2 made font substitution honest — A3 is the first **write**
that actually rewrites text in the page's content stream.

#### Concepts learned

- **A PDF has no "text field" to edit — you rewrite a show operator.** A run is a
  `PDFium` text page-object; editing it means calling `FPDFText_SetText` to swap the
  Unicode the object paints. `PDFium` re-encodes glyphs against the object's font and
  rewrites the content stream for you. That's why the tech stack picks "redact+reflow
  via `PDFium` primitives" over hand-splicing the stream in lopdf — glyph encoding is
  exactly the part you don't want to reimplement.

- **`PDFium` is now a writer — but only between byte snapshots.** Every earlier write
  used lopdf; A3 is the first to mutate via `PDFium`. The catch (learned the hard way
  earlier with `resize_pages`): mutating the actor's **live** document and then dropping
  it SIGSEGVs at teardown. The fix is the same shape as every COS edit — mutate a
  *throwaway* doc loaded from the current bytes, serialize, and swap the live doc to the
  result, with the pre-edit bytes as the undo snapshot (`RestoreDocEdit`). The bytes stay
  the document of record; `PDFium` is just a transform in the middle.

- **Staged regeneration is load-bearing.** `set_text` mutates the object's FFI handle
  but does **not** mark the page dirty, so `save_to_bytes` happily writes the *old*
  content stream — the edit silently vanishes. The cure: set the page's content
  regeneration strategy to `Manual`, stage the change, then call `regenerate_content()`
  exactly once before saving. (The default `AutomaticOnEveryChange` regenerates
  mid-mutation, which is its own source of crashes.) A whole class of "the FFI call
  succeeded but nothing changed" bugs hides in *when* a library flushes your changes.

- **Sometimes the library is the wall — diagnose, then descope honestly.** Delete, true
  redaction, and recreating a run in a substitute font all need `FPDFPage_RemoveObject`.
  In our bundled `PDFium` that FFI call **SIGSEGVs** — confirmed by bracketing it with
  stderr markers (`before remove` prints; `after remove` never does), and reproduced
  with one *and* two page loads, so it's the library, not our borrow dance. The
  disciplined move isn't to thrash a workaround into the design under the radar — it's
  to stop, show the evidence, and let the human choose scope. We shipped the **edit**
  half (which works and is tested) and deferred the **redact** half to a future lopdf
  content-stream approach. Editing a non-embedded-font run still succeeds; A2's warning
  already covers the substitution.

- **`run_index` is a contract between read and write.** A3 counts text objects in the
  *same* order as A1's `extract_text_runs`, so the index a future click-to-edit (B1)
  hands back maps straight to the object A3 mutates. Read and write agreeing on identity
  is what makes "click this, change that" possible.

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/src/pdf/reflow.rs` | `replace_text_run` (throwaway-doc `set_text` + `Manual` regenerate + swap) and `ReplaceTextRunEdit` (undoable, `RestoreDocEdit` inverse). |
| `src-tauri/src/pdf/mod.rs` | `pub mod reflow;`. |
| `src-tauri/tests/reflow.rs` | Replace preserves position + changes text (via A1 re-extraction); round-trips through `PDFium`; edits a non-embedded font; bad index errors; inverse restores; `#[ignore]` artifact writer. |

#### Further reading

- pdfium-render `PdfPageTextObject::set_text`, `PdfPage::set_content_regeneration_strategy` / `regenerate_content`; PDFium's `FPDFText_SetText` / `FPDFPage_GenerateContent` underneath.
- PDF 32000-1:2008 §9.4 (text-showing operators) — what `set_text` ultimately rewrites.

---

### P4.B1 — click-to-edit (the payoff of Track A)

#### Problem

A1 (find runs), A2 (font honesty), A3 (the edit write) were each shipped as `[~]`
infrastructure with no user-facing surface. B1 is the step that makes them a feature:
click a word in the page, edit it, see it change. It's almost entirely *wiring* — and
that's the lesson, because the foundation was built so the wiring is small.

#### Concepts learned

- **A feature can be (mostly) plumbing when the foundation is right.** B1 adds **no new
  write mechanism** — the actor message just calls A3's `ReplaceTextRunEdit`; the layer
  just calls A1's `extractTextRuns` and A3's `replaceTextRun`. The hard parts (locating
  runs, the lossy write, font honesty) were paid for upstream, so the "headline" step is
  a thin actor message + a command + an overlay. When a feature feels small, that's
  usually evidence the earlier decomposition was good.

- **`run_index` is the identity contract between read and write.** The frontend hit-tests
  a click against the bbox A1 returned, and hands the run's *array index* to the write.
  That only works because A1's `extract_text_runs` and A3's `nth_text_object_index` walk
  text objects in the **same order**. Read and write must agree on "which run is run 3,"
  or you edit the wrong word. Identity, not coordinates, is what ties a click to an edit.

- **The overlay pattern, reused.** B1's `TextEditLayer` is the same shape as every other
  annotation layer: an absolutely-positioned div per page, `pointerEvents: none` on the
  container so scrolling passes through, and per-run hit-zones that opt back into
  `pointerEvents: auto` (the WKWebView-friendly approach — no HTML5 DnD). Commit →
  `bumpEpoch` → the canvas reloads from the actor's new bytes and renders the edit. Once
  you have one good layer, the next is a fill-in-the-blanks.

- **Honesty surfaces where the user acts.** A2's banner warns once per document; B1 adds
  a *per-edit* cue (`run.embedded === false` → "may render in a substitute") right in the
  editor. Showing the caveat at the moment of the edit — not just at open — is the
  difference between a warning the user reads and one they actually apply.

- **Cosmetic vs. authoritative styling.** The editor's on-screen font is a *guess*
  (`cssFamilyForFont` buckets the name into serif/mono/sans). It deliberately doesn't try
  to be exact, because it doesn't matter: `set_text` preserves the *real* font in the
  file. Knowing which parts of a UI are authoritative (the saved bytes) and which are
  just preview (the editor chrome) keeps you from over-engineering the preview.

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/src/pdf/actor.rs`, `commands/pdf.rs`, `lib.rs` | `ReplaceTextRun` message + `pdf_replace_text_run` (applies A3's `ReplaceTextRunEdit`; mirrors the `UpdateFreeText` arm). |
| `src/ipc/text-edit.ts` | `replaceTextRun` wrapper → `HistoryState`. |
| `src/tools/text-edit/text-edit.ts` | `cssFamilyForFont` — cosmetic editor-preview font mapping. |
| `src/view/text-edit-layer.tsx` | The overlay: run hit-zones + inline editor + commit; mounts in `PageVirtualizer`. |
| `src/app/MarkupToolbar.tsx`, `tools/_framework/types.ts` | The **Edit Text** toggle + the `edit-text` `ToolId`. |
| `tests/text_edit.rs`, `text-edit.test.ts`, `text-edit-layer.test.tsx`, `tools/text-edit` unit | Actor edit + undo + bad-index + artifact; IPC marshalling; layer interaction; font buckets. |

#### Further reading

- React Testing Library `findBy*` (async) + Vitest `vi.hoisted` — mocking a module whose factory needs a fixture defined in the test file.
- The annotation-layer pattern in this repo (`free-text-layer.tsx`) — the template B1 follows.

---

### P4.B3 — deleting text via content-stream surgery (routing around a library crash)

#### Problem

A3 hit a wall: PDFium's `FPDFPage_RemoveObject` SIGSEGVs in our bundled build, so
delete, redaction, and substitute-baking were all blocked. B3 is the workaround — remove
a run by editing the raw page **content stream** in lopdf instead of asking PDFium to do
it. It's the first time the project deletes content at the COS byte level.

#### Concepts learned

- **A PDF page is a program; deleting text = removing an instruction.** The page content
  stream is a sequence of operators — `BT`/`Tf`/`Td`/`Tj`/`ET`. The glyphs of a run are
  painted by one *text-showing operator* (`Tj` for a string, `TJ` for a kerned array).
  Delete = decode the stream into operations, drop that one operator, re-encode. The
  positioning (`Td`) and font (`Tf`) ops can stay as harmless no-ops; only the show
  operator paints, so removing it is a clean delete. lopdf hands you exactly this:
  `get_and_decode_page_content` → `Vec<Operation>`, and `change_page_content` to write back
  (it transparently handles `/Contents` being a single ref, a 1-array, or an N-array).

- **Two engines, two orderings — and a verification bridge.** A1's `run_index` counts
  *PDFium text objects*; the splice counts *show operators in the content stream*. These
  agree on normal pages (PDFium processes content in stream order), but I can't *assume*
  it — a mismatch would silently delete the wrong sentence. The fix is a **verify-by-
  re-extraction** bridge: extract runs before, splice, extract runs after, and require
  `after == before` with exactly the target index removed — else error, input untouched.
  When two subsystems must agree on an ordering, don't trust the alignment: **assert it,
  and fail safe.** (Bonus: that exact check *is* P6-SEC-010's "verify the text is gone.")

- **Fail-safe beats best-effort for destructive ops.** Editing the wrong run is annoying;
  *deleting* the wrong run is data loss. So the edges all error rather than guess: text
  inside an `XObject` (no show operator in the page stream → out of range), and `'`/`"`
  operators (which advance the line as they show, so removing them would shift following
  text). A hand-built 2-run fixture pins the ordinal guarantee (delete run 0 → run 1
  survives, and vice-versa) — the test that would have caught the scariest bug.

- **Route around the library, keep the contract.** B3 swaps the *mechanism* (lopdf, not
  PDFium) but keeps the exact same shape as every other write: a bytes → bytes transform
  wrapped in a `DeleteTextRunEdit` that snapshots, swaps the live doc, and stores a
  `RestoreDocEdit` inverse. The actor, undo, and command layers didn't notice the engine
  change. Good seams let you replace what's behind them without disturbing the callers.

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/src/pdf/reflow.rs` | `delete_text_run` (lopdf `splice_out_show_operator` + PDFium `extract_runs` verify) + `DeleteTextRunEdit`. |
| `src-tauri/src/pdf/{actor}.rs`, `commands/pdf.rs`, `lib.rs` | `DeleteTextRun` message + `pdf_delete_text_run` (mirrors the `ReplaceTextRun` arm). |
| `src/ipc/text-edit.ts`, `src/view/text-edit-layer.tsx` | `deleteTextRun` wrapper + the **Delete** button in the run editor (completes B3). |
| `tests/text_delete.rs` | Removes from `hello.pdf` (verified by re-extraction); 2-run fixture ordinal correctness; XObject fail-safe; out-of-range; artifact. Plus `text_edit.rs` delete+undo and the frontend tests. |

#### Further reading

- PDF 32000-1:2008 §9.4.3 (text-showing operators `Tj`/`TJ`/`'`/`"`) and §7.8.2 (content streams).
- lopdf `Document::get_and_decode_page_content` / `change_page_content`; `content::{Content, Operation}`.

---

### P4.B2 — adding text as page content (reuse, and a font-resource gotcha)

#### Problem

P3 already lets you "add text" — but as a `/FreeText` *annotation*, which some workflows
strip and which isn't "real" page text. P4-EDIT-003 wants text that's **part of the page**:
content-stream text you can later select, search, edit (B1), and delete (B3). B2 builds that.

#### Concepts learned

- **The same drawing, a different destination.** Free-text already emits a
  `q BT /F1 … Tj … ET … Q` fragment — it just paints it into an annotation's `/AP` XObject.
  B2 paints the *identical* fragment straight into the **page content stream**. The drawing
  code (wrap, colour, underline rule) was reused verbatim by parameterizing one thing: the
  font resource name. Recognizing that "annotation appearance" and "page content" are the
  same PDF graphics, just attached to different objects, is what made B2 small.

- **A shared namespace bites: font resource names.** A page's `/Resources /Font` maps short
  names (`/F1`, `/F2`) to font objects, and a `Tf` operator references one. If I'd blindly
  registered my font as `/F1`, a page that already defined `/F1` for a *different* face would
  suddenly render its existing text in my font — silent corruption. Fix: scan the existing
  keys and pick an unused name (`Fvibe`, `Fvibe1`, …). The test that would've caught the bug
  builds a page with its own `/F1` and asserts both texts survive. Whenever you add an entry
  to a namespace you didn't create, check what's already there.

- **Never mutate a shared object.** A page's `/Resources` can be a *direct* dict, a
  *reference* to a shared dict, or *inherited* from `/Pages`. Adding a font to a referenced or
  inherited dict would change it for *other* pages too. So `register_page_font` **clones**
  whatever it finds into a page-owned direct dict before editing. Aliasing in a document model
  is as dangerous as aliasing in memory — copy-on-write before you touch shared state.

- **Doing it "right" pays compound interest.** Because the text is real content (not an
  annotation), it needs *no* new edit or delete path — B1's Edit Text and B3's Delete already
  work on it. One honest implementation choice (content stream, per the spec) deleted a whole
  category of follow-up work. The cheapest feature is the one the architecture already covers.

- **`cos_edit` is the write chassis.** B2's `TextBoxEdit` is six lines: hand a bytes → bytes
  transform to the shared `cos_edit` helper and undo/redo, dirty-tracking, and the actor swap
  all come for free. By now every COS write is "write the transform, plug into the chassis."

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/src/pdf/cos.rs` | `add_text_box` + `register_page_font` (collision-free, copy-on-write Resources); `free_text_appearance_content` gained a `font_res` param. |
| `src-tauri/src/pdf/{annotation,actor}.rs`, `commands/pdf.rs`, `lib.rs` | `TextBoxEdit` + `AddTextBox` message + `pdf_add_text_box` (mirrors the free-text write path). |
| `src/ipc/text-box.ts`, `src/view/text-box-layer.tsx`, `MarkupToolbar.tsx`, `tools/_framework/types.ts` | `addTextBox` + the drag-to-create overlay + the **Add Text** tool (reusing the free-text style controls). |
| `tests/text_add.rs`, `text-box.test.ts`, `text-box-layer.test.tsx` | Content-run-not-annotation; `F1`-collision fixture; empty rejects; actor add+undo; marshalling; drag→add. |

#### Further reading

- PDF 32000-1:2008 §7.8.3 (Resource dictionaries) and §9.6.2 (the standard-14 fonts).
- This repo's `cos.rs::free_text_appearance_content` (the shared drawing) and `cos_edit` (the write chassis).

---

### P4.C1 — adding an image (the third time's a refactor)

#### Problem

"Add image" sounds like a from-scratch feature, but it's the *third* "put content on the page"
operation (after add-text and the image *stamp*). C1 embeds a PNG/JPEG into the **page content
stream** — and the interesting part is how little new code it needed, plus one genuinely new
trick: embedding JPEG without decoding it.

#### Concepts learned

- **PDF speaks JPEG natively — don't decode it.** A PNG must be decoded to raw samples (PDF has
  no PNG filter), but JPEG *is* a PDF filter: `/DCTDecode`. So `embed_jpeg` stores the original
  JPEG bytes **verbatim** as the stream content and just sets `/Filter /DCTDecode` — no pixel
  work at all. The only thing it parses is the **SOF marker** (Start Of Frame) for width/height
  and component count (→ `DeviceGray`/`RGB`/`CMYK`). Walking JPEG marker segments (`FF xx` +
  2-byte length, skipping the standalone `RST`/`SOI`/`EOI` markers and `FF` fill bytes) to find
  `SOF0`–`SOF15` is the kind of binary-format scan worth knowing: most container formats are a
  tag-length-value walk.

- **The third occurrence is the refactor signal.** add-text (B2) and add-image (C1) both need:
  register a resource on the page under a name that won't collide, cloning a shared/inherited
  `/Resources` first; and append a `q … Q` content fragment. After the second copy, I pulled
  these into `register_page_resource(category, prefix, value)` and `append_page_content` — so
  B2's font registration and C1's XObject registration are now *one* function parameterized by
  `b"Font"` vs `b"XObject"`. The "rule of three" in practice: the first use is a one-off, the
  second is a coincidence, the third earns the abstraction (and the existing tests prove the
  refactor preserved behavior).

- **Aspect-fit is a placement decision, not a resize.** The image draws in the unit square and a
  `cm` matrix maps it onto the page. To avoid distortion, `aspect_fit_rect` shrinks the user's
  drawn box to the image's aspect ratio and centres it — so the `cm` is always a uniform scale.
  Stretching would've been one line *less* code and looked wrong on every non-square image.

- **A real fixture beats a synthetic one for the format you don't control.** The PNG tests build
  images in-memory (we own a PNG encoder). For JPEG we don't — so the unit tests use a
  hand-crafted *header* (enough to exercise the SOF parser), but the integration test embeds a
  **real `sample.jpg`** (generated once via `sips`) so PDFium actually has to *decode and render*
  it. Synthetic inputs test your parser; real inputs test your assumptions.

- **Doing it as content keeps paying.** Like B2, the image is real page content, so C2
  (move/resize/rotate/replace/delete) will manipulate it with the same primitives — no separate
  "image object" model. The `cos_edit` chassis again made the undoable write six lines.

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/src/pdf/image_xobject.rs` | `embed_jpeg` (DCTDecode + SOF parse) + `embed_image` dispatch; reuses `embed_png`. |
| `src-tauri/src/pdf/cos.rs` | `add_image` + `aspect_fit_rect`; `register_page_resource` / `append_page_content` generalized out of B2. |
| `src-tauri/src/pdf/{annotation,actor}.rs`, `commands/pdf.rs`, `lib.rs` | `AddImageEdit` + `AddImage` message + `pdf_add_image` (reads the file, like the stamp command). |
| `src/ipc/image.ts`, `src/state/image-add-store.ts`, `src/view/image-add-layer.tsx`, `MarkupToolbar.tsx` | `addImage` + the pick-then-arm flow + the drag-to-place overlay + the **Add Image** button. |
| `tests/image_add.rs`, `image_xobject.rs` units, `image.test.ts`, `image-add-layer.test.tsx`, `fixtures/basic/sample.jpg` | Content-XObject-not-annotation; real-JPEG DCTDecode round-trip; collision-free names; unsupported-format errors; drag→add. |

#### Further reading

- PDF 32000-1:2008 §7.4.8 (`DCTDecode`) and §8.9 (image XObjects); ITU-T T.81 (JPEG) §B.2 (frame headers / SOF markers).
- The "rule of three" (Martin Fowler, *Refactoring*) — when duplication earns an abstraction.

---

### P4.C2 — editing an image (validate first, and a bug PDFium was hiding)

#### Problem

C2 is the biggest single feature so far — *five* operations on an interactive selection box —
and it carried a real unknown: A3 proved PDFium's `remove_object` SIGSEGVs, but C2's
move/resize/rotate want to *mutate* an object's matrix. Would that crash too? The whole plan
hinged on the answer.

#### Concepts learned

- **De-risk the unknown in increment 1, not at the end.** Before writing the actor, the IPC, or
  a single line of UI, I wrote *one* test: add an image, `reset_matrix` it, re-extract, assert no
  crash. It passed — `reset_matrix` is a *mutate-in-place* FFI (`FPDFPageObj_TransformF`) in the
  same family as `FPDFText_SetText` (which works), not `FPDFPage_RemoveObject` (which crashes).
  Had it crashed, I'd have stopped and reported with one cheap test burned, not a half-built
  feature. When a plan rests on an unknown, spend the first increment proving it.

- **Five gestures, one primitive.** Move, resize, and rotate look like three features but are all
  just *a new placement matrix*. The frontend computes it (drag → translate, corner → new rect,
  button → `rotate90` composed about the centre) and the backend has a single `transform_image`.
  Pushing the variation into pure, testable matrix math (`matrix.ts`) kept the backend tiny and
  the hard part unit-tested.

- **A bug one layer was hiding from another.** The ordinal test failed with a bbox of *exactly*
  `image0_cm × image1_cm` — image 0's transform leaking into image 1. Dumping the re-encoded
  content stream showed the smoking gun: **`ETq`**. PDF concatenates the streams in a `/Contents`
  array, and the spec says insert whitespace between them — **PDFium does, lopdf doesn't**. So a
  page ending `…ET` fused with our appended `q` into one bogus token; PDFium had silently fixed
  it on read, so C1's "add" looked fine, but lopdf's decode→re-encode (the delete path) inherited
  the corruption. Two libraries disagreeing about an implicit rule is a classic source of "works
  here, breaks there." The one-character fix (prepend `\n` to appended content) closed it.

- **Verify destructive edits by their *effect*, not just a count.** My first delete-verify only
  checked "one fewer image" — which *passed* on the corrupted output (still one image, just
  mislocated). Strengthening it to "the survivors' bboxes match the originals minus the target"
  would have caught the corruption as an error. Count is necessary but not sufficient; verify the
  thing you actually care about (geometry unchanged), and the safety net catches more.

- **Read like A1, write like A3, delete like B3.** C2 is mostly *recomposition* of patterns:
  the read mirrors text-run extraction, the transform mirrors `replace_text_run`'s throwaway-doc
  write, the delete mirrors `delete_text_run`'s lopdf splice. The selection-box UI is the only
  genuinely new surface. By Track C, "a new editing op" is mostly choosing which existing chassis
  to bolt it onto.

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/src/pdf/image_extract.rs` | `extract_images` (live-PDFium read) + `extract_images_from_bytes` (the verify helper). |
| `src-tauri/src/pdf/image_edit.rs` | `transform_image` (PDFium `reset_matrix`) + `delete_image` (lopdf `Do`-splice, verified) + the two `Edit` impls. |
| `src-tauri/src/pdf/cos.rs` | `append_page_content` gained a leading `\n` (the `ETq` fix). |
| `src-tauri/src/pdf/{actor}.rs`, `commands/pdf.rs`, `lib.rs` | `ReadImages`/`TransformImage`/`DeleteImage` + three commands. |
| `src/tools/image-edit/matrix.ts`, `src/ipc/image-edit.ts`, `src/view/image-edit-layer.tsx`, `MarkupToolbar.tsx` | Matrix math + IPC + the selection-box overlay + the **Edit Image** tool. |
| `tests/image_edit.rs`, `matrix.test.ts`, `image-edit.test.ts`, `image-edit-layer.test.tsx` | Risk-#1 no-crash; locate; move/resize/rotate; delete + ordinal; actor undo; matrix units; select/delete/rotate UI. |

#### Further reading

- PDF 32000-1:2008 §7.8.2 (content streams; the inter-stream whitespace rule) and §8.3.4 (the `cm` transform matrix).
- pdfium-render `reset_matrix` / `transform` (`FPDFPageObj_TransformF`) vs `remove_object`.

---

### P4.C2b — replace an image (mutate the object, not the references)

#### Problem

C2 did move/resize/rotate/delete; "replace" was the last verb of P4-EDIT-006. The naïve idea —
repoint the resource name or rewrite the `Do` — drags in the copy-on-write `/Resources` problem
and content-stream surgery. There's a cleaner level to act at.

#### Concepts learned

- **Edit the indirection's *target*, not the indirection.** A page draws an image as
  `…/Img Do`, where `/Img → Reference(old_id)` in `/Resources /XObject`, and `old_id` is the
  pixel stream. To replace the pixels, I embed the new image (a new object) and **overwrite
  `old_id`'s contents in place** (`doc.objects.insert(old_id, new_obj)`). The name still points at
  `old_id`; the `cm`, the `Do`, the Resources dict — *nothing* about the references changes, so
  there's no copy-on-write and no content rewrite. When you want to change a thing many places
  point to, change the thing, not every pointer.

- **Reuse compounds.** `replace_image` is `embed_image` (C1) + the image-`Do` ordinal walk (C2's
  delete) + the `image_edit_apply` undo chassis + a one-line object swap. The genuinely new code
  is tiny because each prior step left a reusable seam. By the fifth image operation, "add a verb"
  is mostly wiring.

- **Verify what the op promises.** Replace promises "swap pixels, keep placement." The primitive
  verifies the *placement* (count + every bbox unchanged → no corruption); the *test* asserts the
  XObject's `/Width`/`/Height` changed (the pixels really swapped) and that an RGBA replacement
  carries an `/SMask` (alpha survives). Between them, both halves of the promise are checked.

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/src/pdf/image_edit.rs` | `replace_image` (embed + in-place XObject swap + verify) + `nth_image_xobject_id` + `ReplaceImageEdit`. |
| `src-tauri/src/pdf/{actor}.rs`, `commands/pdf.rs`, `lib.rs` | `ReplaceImage` message + `pdf_replace_image` (reads the file). |
| `src/ipc/image-edit.ts`, `src/view/image-edit-layer.tsx` | `replaceImage` + the **Replace** button (file dialog) in the selection toolbar. |
| `tests/image_replace.rs`, `image-edit.test.ts`, `image-edit-layer.test.tsx` | Swap preserves placement + changes dims; keeps alpha; ordinal correctness; actor replace+undo; marshalling; Replace-button → file pick. |

#### Further reading

- PDF 32000-1:2008 §7.3.10 (indirect objects) — why overwriting the object behind a reference is the surgical edit.

---

## P4.C3 — Hyperlinks (Link annotation)

#### Problem

Make a region of a page clickable: open a URL, jump to another page, follow a named
destination, or start an email. This is the last Track-C feature.

#### Concepts learned

- **A hyperlink is an annotation, not content.** Unlike add-text/add-image (which write into
  the page *content stream*), a link is a `/Link` **annotation** dictionary in the page's
  `/Annots`. So C3 reused the annotation chassis (`cos::add_*` + `append_annotation`, run
  through `annotation::cos_edit`), the exact shape the sticky note already used — almost no new
  machinery. The lesson: classify the feature (content vs. annotation) *first*; it picks your
  whole code path.
- **Four target shapes, one dict.** External URL and email both use an action —
  `/A << /S /URI /URI (…) >>` — differing only by a `mailto:` prefix. An internal page jump is
  a *destination*: `/Dest [pageRef /Fit]`. A named destination is `/Dest (name)` (a string the
  reader looks up in the catalog's `/Names/Dests`). `/Fit` means "show the whole page" — no
  scroll coordinates to compute.
- **Pick the representation that existing code already understands.** The internal-page link
  uses the **array-with-page-ref** form (`[pageRef /Fit]`) precisely because the page
  reorder/delete cleanup written back in P2 (`dest_target_page`, `prune_dangling_destinations`)
  already resolves *that* shape. Choosing it means a link created here is automatically
  fixed-up or pruned when its target page moves or is deleted — zero new code. Had we invented a
  different encoding, we'd have had to teach the cleanup about it.
- **Let the library escape your strings.** A URL can contain `()` and `\`, the exact characters
  that delimit/escape a PDF literal string. Hand-concatenating would corrupt the file;
  `Object::string_literal` escapes for us. The test `url_with_parens_is_escaped` is the guard.
- **Find your own object back in a shared fixture.** `links.pdf` already ships with its own
  `/Link` annotations, so "the first link with a page Dest" matched a *pre-existing* one, not
  ours — the page-target test failed with a confusing off-by-one. Fix: tag what you add with
  something unique (here, a known `/Rect`) and look it back up by that. General rule for
  integration tests against real fixtures: never assume your write is the only one present.
- **1-based for humans, 0-based on the wire.** The popover asks "Page 1–N" (what a person sees);
  `toWireValue` subtracts one before the IPC call, because the Rust command (like every page API
  here) is 0-based. Keeping that conversion in one pure, tested function stops the off-by-one
  from leaking into the UI.

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/src/pdf/cos.rs` | `add_link` (builds the `/Link` dict for url/email/page/named) + `page_id_at`. |
| `src-tauri/src/pdf/annotation.rs` | `AddLinkEdit` (snapshot → `add_link` → reload; inverse `RestoreDocEdit`). |
| `src-tauri/src/pdf/actor.rs`, `commands/pdf.rs`, `lib.rs` | `AddLink` message + `pdf_add_link` command + register. |
| `src/tools/link/target.ts` | Pure `LinkTarget` union, `validateTarget`, `toWireValue` (1-based → 0-based). |
| `src/ipc/links.ts`, `src/view/link-layer.tsx` | `addLink` wrapper + the drag-a-rect + target-popover overlay. |
| `src/app/MarkupToolbar.tsx`, `src/view/PageVirtualizer.tsx`, `src/tools/_framework/types.ts` | "Add Link" button, layer mount, `add-link` tool id. |
| `tests/link.rs`, `src/tools/link/__tests__/target.test.ts` | 8 Rust (roundtrip / mailto / page Dest / named / escaping / range / unknown-kind / actor-undo) + 7 frontend (validation + wire conversion). |

#### Further reading

- PDF 32000-1:2008 §12.5.6.5 (Link annotations) and §12.6.4.7 (URI actions) — the dict shapes.
- PDF 32000-1:2008 §12.3.2 (Destinations) — `/Dest` arrays, `/Fit`, and named destinations.

---

## P4.C3b — Link appearance (invisible / box / underline)

#### Problem

C3 made links work, but they were invisible — you couldn't *see* the clickable region. Verifying
C3 in a reader drove this home: the only rectangle visible was the reader's hover affordance and a
fixture's own bordered link. Add an opt-in appearance: a box, an underline, or invisible, in a color.

#### Concepts learned

- **`/Border` is a hint; `/AP` is the truth.** The cheap way to "show" a link border is
  `/Border [0 0 1]` (width 1) or a `/BS` border-style dict. But readers honor those inconsistently —
  Preview historically draws link borders only on hover. The reliable way to make *anything* render
  identically everywhere is a generated **appearance stream** (`/AP`): a Form XObject whose content
  you draw yourself. We attach an `/AP` that strokes the rectangle (box) or a bottom rule (underline),
  and *also* set `/C` + `/BS` as a fallback hint. This is the project's recurring "draw your own `/AP`"
  pattern (markup, free-text, stamps all do it) — the lesson is: if it must look the same in Acrobat,
  Preview, and Okular, don't rely on the reader's default rendering of a structural key.
- **Appearance streams draw in their own coordinate space.** A Form XObject has a `BBox` and a
  matrix; with `BBox == Rect` and the identity matrix, form space == page space, so the content
  draws in absolute page coordinates — no translation math. (Same trick the markup `/AP` uses.)
- **Stroke inside the box, or it clips.** A 1pt stroke centered on the BBox edge is half outside the
  BBox and gets clipped to a 0.5pt hairline. Inset the path by half the line width (`x0+0.5 …
  w-1`) so the whole stroke lands inside.
- **Keep the no-op path byte-identical.** "Invisible" had to stay exactly what C3 emitted
  (`/Border [0 0 0]`, no `/AP`), so C3's existing tests keep passing untouched. The new code is a
  branch that *adds* keys for the visible styles; the invisible branch is the old code verbatim. A
  test (`invisible_style_has_no_ap`) pins that contract.
- **A visible default is a product decision, not a code one.** The spec line was drafted with
  invisible as the default; the user asked for a visible default, so the default moved to *box* —
  a one-line change in `target.ts` (`DEFAULT_LINK_STYLE`) and the spec parenthetical. Worth noting
  how cheap "change the default" is when the default lives in one named constant, not scattered.
- **Verifying the fix also caught a latent label bug.** Regenerating the artifact with the page
  link as 0-based `"1"` made it correctly target page 2 (PDFKit confirmed) — fixing the
  page-3-vs-2 confusion the C3 verification surfaced (the C3 artifact harness had passed a raw
  0-based `"2"`).

#### Files in this step

| File | Role |
|---|---|
| `docs/02_PRODUCT_SPEC.md` | New spec line **P4-EDIT-007b** (link appearance). |
| `src-tauri/src/pdf/cos.rs` | `add_link` gains `style`+`color`; `apply_link_appearance` + `link_appearance_content` build the `/AP`. |
| `src-tauri/src/pdf/annotation.rs`, `actor.rs`, `commands/pdf.rs` | thread `style`+`color` through `AddLinkEdit` / `AddLink` / `pdf_add_link`. |
| `src/tools/link/target.ts` | `LinkStyle` union, `LINK_STYLE_LABELS`, `DEFAULT_LINK_STYLE` (box) + `DEFAULT_LINK_COLOR`. |
| `src/ipc/links.ts`, `src/view/link-layer.tsx` | `addLink` gains the params; popover gains a Style select + a color input. |
| `tests/link.rs`, `target.test.ts` | 6 new Rust (invisible no-AP / box AP+C+BS / underline U / still-navigates / unknown style / bad color) + 2 frontend (defaults + style list). |

#### Further reading

- PDF 32000-1:2008 §12.5.5 (Appearance streams) — the `/AP` `/N` Form XObject.
- PDF 32000-1:2008 §8.4.3 (Graphics state) + §8.5 (path painting) — `w`, `RG`, `re`, `m`/`l`/`S`.

---

## P4.D2 — Watermark (text / image)

#### Problem

Stamp "DRAFT" (or a logo) across selected pages, faint and rotated, above or behind the
content — and do a 50-page document in under 2 seconds.

#### Concepts learned

- **A watermark is page content, not an annotation.** It goes straight into the page's content
  stream — the same path as add-text/add-image — so it reuses `register_page_resource` +
  `append_page_content`. Classifying it (content vs. annotation) up front picked the whole
  approach, exactly like the link did (annotation) and add-text did (content).
- **`q … Q` saves and restores graphics state.** Every fragment we add is wrapped in `q` (save)
  … `Q` (restore) so the colour, opacity, and transform we set can't leak into — or out of — the
  page's own drawing. This is *the* discipline for splicing content into someone else's stream.
- **Opacity isn't a paint operator — it's graphics state.** You can't "set alpha" inline; you set
  it via an **`/ExtGState`** resource (`/ca` fill alpha, `/CA` stroke alpha) referenced with `gs`.
  So a translucent watermark needs a registered ExtGState, not just a colour.
- **Rotate by transforming the coordinate system, then draw at the origin.** Instead of computing
  rotated glyph positions, set the CTM to `cosθ sinθ -sinθ cosθ cx cy cm` (rotate about origin,
  then move the origin to the page centre) and draw the mark centred on `(0,0)` — offset by half
  its width/height. The matrix does the trig; the content stays simple. An image needs a second
  `cm` (it draws in the unit square, so scale it to `sw×sh` and recentre).
- **On top vs. behind is just stream order.** PDF paints content streams in array order, so
  *appending* the watermark draws it last (on top) and *prepending* draws it first (behind, the
  page's own content paints over it). That's the whole difference — hence the new
  `prepend_page_content`, a mirror of `append_page_content` with the separator newline at the
  **end** (our `…Q` mustn't fuse with the next stream's first token — the same `ETq`-class trap
  C2 hit, just on the other side).
- **Embed shared resources once.** An image watermark on 50 pages embeds the image as a single
  XObject and has every page's `/Resources` *reference* it — not 50 copies. Cheap, and it's why
  the 50-page run is ~0.1 s: it's pure object-adds + one save, no rasterization.
- **A feature earns its own module when a whole track will share it.** Unlike the link (one dict,
  kept in `cos.rs`), watermark got `watermark.rs` because Track D's background / header-footer /
  page-numbers / Bates all want the same place-content-on-pages machinery. That meant promoting a
  few `cos.rs` helpers to `pub(crate)` — a deliberate, minimal widening.

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/src/pdf/watermark.rs` | `WatermarkKind` + `add_watermark` (per-page ExtGState/font/image + rotated `q…Q`) + self-contained `WatermarkEdit`. |
| `src-tauri/src/pdf/cos.rs` | `prepend_page_content` (new) + five helpers promoted to `pub(crate)`. |
| `src-tauri/src/pdf/actor.rs`, `commands/pdf.rs`, `lib.rs` | `AddWatermark` message + `pdf_add_text_watermark` / `pdf_add_image_watermark` (reads the file). |
| `src/tools/watermark/watermark.ts` | `WatermarkSpec` + `parsePageRange` ("all" / "1-3,5" → 0-based) + defaults. |
| `src/ipc/watermark.ts`, `src/app/WatermarkDialog.tsx` | IPC wrappers + the document-wide dialog (mounted in `PdfViewer`, opened from `ZoomToolbar`). |
| `tests/fixtures/basic/many-pages.pdf` (+ `generate-many.py`) | 50-page fixture for the `<2s` acceptance (reused by D3–D5). |
| `tests/watermark.rs`, `watermark.test.ts` | 10 Rust (selected-pages, behind/on-top order, opacity GState, rotation `cm`, image-embeds-once, errors, 50-page `<2s`, undo) + 4 frontend. |

#### Further reading

- PDF 32000-1:2008 §8.4.5 (Graphics state parameter dictionaries) — `/ExtGState`, `/ca`, `/CA`.
- PDF 32000-1:2008 §8.3.4 (Transformation matrices) — the `cm` rotate-about-a-point pattern.

---

## P4.D1a — Background (colour / image)

#### Problem

Fill a colour or an image behind the content of selected pages. The watermark, but always
full-page and always behind — and the first feature to *reuse* Track D's shared machinery rather
than add its own.

#### Concepts learned

- **"Behind" is the easy direction.** A background is just a watermark that's always full-page and
  always prepended. Once D2 had `prepend_page_content`, the colour case was three lines: fill the
  `MediaBox` rect (`x0 y0 w h re f`) inside a `q…Q`, prepend it. Recognising a new feature as an
  existing one with two knobs locked is most of the design.
- **Cover-fit needs a clip.** A background image should *fill* the page (no letterbox gaps), which
  means scaling so the image covers the rect (`cover = max(w/iw, h/ih)`) — and that overflows the
  page on one axis. To stop the overflow painting outside the page you set a **clip path** first:
  `x0 y0 w h re W n` (append the rect to the path, `W` = intersect-clip, `n` = no-op-paint that
  consumes the path), *then* draw. `W n` is the canonical "clip to this rectangle" idiom.
- **Validate before you mutate.** `add_background` parses the colour and embeds the image *before*
  the per-page loop, so a bad `#hex` or a corrupt image fails with the document untouched — never a
  half-applied background on pages 1–3 and an error on page 4. Resolve-then-act.
- **Promote a helper the moment a second caller appears.** `page_media_box` was private to
  `watermark.rs`; background needed the identical logic, so it moved to `cos.rs` as `pub(crate)`
  (one copy, both callers) rather than being duplicated. Same on the frontend: `parsePageRange`
  moved from `tools/watermark/` to a shared `tools/page-range.ts`, re-exported from watermark for
  back-compat so no import broke. Two users → shared module; the relocation is mechanical and the
  moved test proves identical behaviour.
- **Slice by cost, not by spec bullet.** P4-EDIT-008 lists three sources (colour, image, PDF page).
  Colour + image are trivial reuse; the PDF-page source is a whole new capability (cross-document
  page → Form XObject import). Shipping D1a now and D1b later keeps each verifiable — the same call
  as C1/C2/C2b. The spec line stays one; the *work* splits.

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/src/pdf/background.rs` | `BackgroundKind` + `add_background` (full-page colour fill / cover-clip image, prepended) + self-contained `BackgroundEdit`. |
| `src-tauri/src/pdf/cos.rs` | `page_media_box` promoted to `pub(crate)` (moved out of `watermark.rs`). |
| `src-tauri/src/pdf/{actor,commands/pdf,lib}.rs` | `AddBackground` message + `pdf_add_color_background` / `pdf_add_image_background` (reads file). |
| `src/tools/page-range.ts` | `parsePageRange` moved here (shared); `tools/watermark/watermark.ts` re-exports it. |
| `src/ipc/background.ts`, `src/app/BackgroundDialog.tsx` | IPC wrappers + the document-wide dialog (mounted in `PdfViewer`, opened from `ZoomToolbar`). |
| `tests/background.rs`, `tools/__tests__/page-range.test.ts` | 8 Rust (fills-behind, selected-pages, image embeds-once + clips, opacity GState, errors, undo) + 3 frontend (moved range cases). |

#### Further reading

- PDF 32000-1:2008 §8.5.4 (Clipping path operators) — `W` / `W*` and the `n` end-path.
- PDF 32000-1:2008 §8.6.3 (Device colour spaces) — `rg` device-RGB fill.

---

## P4.D1b — Background from a PDF page

#### Problem

Use a page from *another* PDF as the background — e.g. drop a company letterhead behind every page
of a document. The spec's third background source, and the only one that's genuinely new work.

#### Concepts learned

- **You can't reference a page; you embed it as a Form XObject.** A PDF page isn't a thing other
  pages can point at. The standard move is to convert it into a **Form XObject** — a self-contained
  bundle of `(BBox, /Resources, content stream)` that *is* referable and paintable with `Do`. So
  "page as background" = "import page → Form XObject → `Do` it behind content." This is the same
  primitive Acrobat calls a "stamp," and it's the building block for N-up, booklets, and overlays.
- **Cross-document object ids collide; renumber first.** The source and destination each number
  their objects from 1, so copying source object `6 0 R` into a dest that already has a `6 0 R`
  would clobber it. lopdf's `renumber_objects_with(dest.max_id + 1)` shifts the *whole* source above
  the dest's range first, so every copied object lands in free space. (This is exactly what
  `cos::merge_documents` does per source — the same trick, one source.)
- **Copy the closure, not the corpus.** The naive import absorbs the entire source document (every
  page, every font) just to use one page — bloating the file. The right scope is the **transitive
  object closure** of the page's `/Resources`: a BFS that follows every reference (Reference →
  Array → Dictionary → Stream-dict) and copies only what's reachable. A test
  (`…copies_only_the_page_subtree_not_whole_source`) pins this by asserting the output has fewer
  objects than dest + source combined.
- **Resources can be inherited.** A page may rely on a `/Resources` dict declared on its `/Pages`
  parent, not on itself — so resolving "effective resources" means walking the `/Parent` chain
  (same shape as `page_media_box`). Miss this and the imported page renders with no fonts.
- **A Form draws in its own coordinate space; place it with one `cm`.** The Form's `BBox` is the
  source `MediaBox`, so its content draws in *source* coordinates. To put it on a (possibly
  different-sized) target page you set the CTM: contain-fit `scale = min(tw/sw, th/sh)`, then
  translate to centre — `e = tx0 + (tw − sw·scale)/2 − scale·sx0`, likewise for `f`. The `−sx0`
  term handles a source `MediaBox` whose origin isn't `(0,0)`.
- **Apple PDFKit's text extraction *is* a render check.** Extracting the output page's text returned
  "Page 1 (link to page 3) Hello, VibePDF." — the imported page's words *and* the host page's,
  imported-first (behind). One cheap call proved the closure copy brought the font, the content
  copied, and the layering is right — without pixels.

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/src/pdf/background.rs` | `BackgroundKind::PdfPage` + `import_page_as_form` (renumber + closure-copy + build Form) + `collect_refs` BFS + `effective_resources` + `pdf_content` (contain-fit). |
| `src-tauri/src/pdf/commands/pdf.rs`, `lib.rs` | `pdf_add_pdf_background` (reads the source file); no new actor message (reuses `AddBackground`). |
| `src/ipc/background.ts`, `src/app/BackgroundDialog.tsx` | `addPdfBackground` + a "PDF page" source (file picker + 1-based page input, sent 0-based). |
| `tests/background.rs` | +6 (imports-form-behind, copies resources + content, embeds-once, subtree-not-whole-source, source-page out-of-range, actor undo). |

#### Further reading

- PDF 32000-1:2008 §8.10 (Form XObjects) — `BBox`, `/Matrix`, `/Resources`, and `Do`.
- PDF 32000-1:2008 §7.3.9 / §7.3.10 (indirect objects) — why cross-document ids must be renumbered.

---

## P4.D3 — Header / footer with placeholders

#### Problem

Put "Page 3 of 50" in the footer and the date in a corner — text in the top/bottom margin, with
left/centre/right positions and per-page placeholders.

#### Concepts learned

- **It's watermark, un-rotated, in the margin.** Same text machinery (register a base-14 font, a
  `BT … Tj ET` show), just positioned at a margin `y` and an aligned `x` instead of rotated-centred.
  The third Track-D text feature confirmed the pattern is a genuine reusable core — hence promoting
  `escape_pdf_string` to `cos.rs` alongside `page_media_box` and `font_avg_em`.
- **Placeholders that depend on page context live server-side; ones that don't come from the
  client.** `{n}` and `{total}` are page-context (Rust knows each page's number and the count), so
  Rust substitutes them. `{date}` is *not* page-context and the project deliberately ships no date
  library — so the **frontend passes the formatted date string** (`new Date()` → `YYYY-MM-DD`) and
  Rust just splices it in. Result: no dependency, no timezone ambiguity, offline-first for free.
  Splitting a feature across the IPC boundary by *what each side actually knows* is often cleaner
  than forcing it all into one layer.
- **Left/centre/right is three shows, one fill.** A header carries all three positions together
  (any empty skipped). They share one `q…Q`, one fill colour, one font — only the `x` differs:
  left = `x0+margin`, centre = `x0+(w−tw)/2`, right = `x1−margin−tw`, where `tw` is the estimated
  text width (`size·font_avg_em·len`). One undoable edit writes all three.
- **Append vs. prepend encodes layering, again.** A header *overlays* content, so it's
  `append_page_content` (draws last, on top) — the mirror of the background's `prepend`. Same knob,
  opposite setting.
- **Assert your own marks, not "any text."** The first cut asserted "page 2 has no `Tj`" — but every
  fixture page already draws its own `(Page N) Tj`. The fix: tests search for the *distinctive*
  string the feature wrote ("Page 2 of 50", "HEADER", "/Fhf") rather than a generic operator. When
  you inject content into a document that already has content, your assertions must be specific
  enough to tell yours apart.

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/src/pdf/header_footer.rs` | `add_header_footer` (L/C/R, header/footer `y`, per-page placeholders) + pure `substitute` + `HeaderFooterEdit`. |
| `src-tauri/src/pdf/cos.rs` | `escape_pdf_string` promoted to `pub(crate)` (moved out of `watermark.rs`). |
| `src-tauri/src/pdf/{actor,commands/pdf,lib}.rs` | `AddHeaderFooter` message + `pdf_add_header_footer` command. |
| `src/ipc/header-footer.ts`, `src/app/HeaderFooterDialog.tsx` | IPC wrapper + the dialog (formats today's date, three text fields, mounted in `PdfViewer`, opened from `ZoomToolbar`). |
| `tests/header_footer.rs` | 9 (substitute unit; footer page-of-total; header-vs-footer y; L/C/R x order; only-non-empty; errors; actor undo). |

#### Further reading

- PDF 32000-1:2008 §9.4.2 (Text-positioning operators) — `Td` / `Tf` and the text matrix.
- PDF 32000-1:2008 §9.4.3 (Text-showing operators) — `Tj`.

---

## P4.HF — Hardening (the FABLE_REVIEW bug batch)

#### Problem

A full-project audit (`FABLE_REVIEW.md`) found four engine bugs worth fixing before D4 stamps
page numbers onto real-world documents: decorations ignored page `/Rotate` and `/CropBox`, a
legal `/Contents` shape corrupted on append, and the encrypted-save path had never been tested.

#### Concepts learned

- **Draw in visual space; let one matrix translate.** Pages can carry `/Rotate` (the viewer
  displays them turned) and a `/CropBox` (the viewer shows only that region). Instead of teaching
  every writer per-angle math, define **visual space** — the displayed crop box, y-up — lay all
  content out there, and prepend a single compensating `cm` (`visual_transform`) mapping visual →
  page coordinates. Writers stay simple; the transform is derived once, tested per-angle. This is
  the same "move the coordinate system, not the content" idea as the watermark's rotation `cm`,
  one level up.
- **Derive rotation matrices from corners, not intuition.** The 90°-case matrix
  `[0 1 −1 0 x1 y0]` isn't guessable; it falls out of mapping the four page corners under the
  viewer's clockwise rotation and inverting. Doing the corner table on paper first made all four
  cases land on the first test run.
- **A pinning test can out-argue a code review.** The audit predicted encrypted docs would save
  *silently decrypted*. The test proved the opposite failure: PDFium preserves encryption, and
  our own round-trip verifier — re-opening the temp file with **no password** — rejected every
  save (`PasswordRequired`). One test replaced a plausible-sounding wrong diagnosis with the real
  bug and its one-line fix (thread the open password into the verify). Write the pin *before*
  designing the mitigation.
- **"Infallible by design" pays off in strange places.** The save path runs
  `prune_dangling_destinations` on the serialized bytes — which, for an encrypted doc, lopdf
  can't meaningfully parse. Because prune was built to return the input unchanged on *any*
  error, the encrypted-save fix needed no changes there. Designing side-passes to fail open
  (keep bytes) rather than fail the operation is what made the fix one parameter.
- **Handle every legal shape of a spec field, not the shapes you've seen.** `/Contents` can be a
  stream ref, an array, *or a reference to an array*. The third shape never appears in
  PDFium-normalized bytes — which is exactly why it survived 50 test files. The fix derefs and
  flattens; the test *constructs* the exotic shape synthetically instead of hunting for a fixture.

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/src/pdf/cos.rs` | `page_rotation`, `page_effective_box`, `visual_transform` + `visual_cm_line`; `existing_contents` deref/flatten fix. |
| `watermark.rs` / `background.rs` / `header_footer.rs` | lay out in visual space; prepend the compensating `cm`; colour fill stays MediaBox. |
| `src-tauri/src/pdf/document.rs` | `save_document`/`verify_pdf_reopens` take the open password — encrypted docs become saveable, encryption preserved. |
| `tests/fixtures/basic/{rotated,cropped}.pdf` (+ generators) | per-angle `/Rotate` and CropBox⊂MediaBox fixtures. |
| `tests/{watermark,background,header_footer}.rs` + `tests/hardening.rs` | +9 tests: per-angle matrices, crop placement, ref→array `/Contents`, encrypted-save pin. |

#### Further reading

- PDF 32000-1:2008 §14.8.4.2 / Table 30 (`/Rotate`) and §7.7.3.3 Table 30 (`/CropBox`).
- PDF 32000-1:2008 §7.8.2 — `/Contents` "shall be a stream or an array of streams".

---

## P4.HF2 — Marked-content tags on decorations

#### Problem

A saved watermark/background/header becomes an anonymous `q…Q` fragment — removing or
re-stamping it later (Bates re-runs after inserting pages, "remove watermark") would need
heuristics. Lay the identity rail *now*, before D4/D5 stamp page numbers everywhere.

#### Concepts learned

- **Marked content is the content-stream analogue of `/NM`.** Annotations get a stable identity
  via `/NM`; raw content has no such slot — but PDF's *marked content* (`BDC tag props … EMC`,
  §14.6) lets you label a run of operators without changing what renders. Tag `/VibePDF`, inline
  dict `<< /Kind (…) /Id (uuid) >>` — no `/Properties` resource needed when the dict is inline.
- **Prove the rail with the consumer you haven't built.** The tag's only purpose is future
  removal, so the test *performs* the removal: decode operations, find the `/VibePDF BDC`, drain
  through its `EMC`, re-encode — DRAFT gone, "Hello" intact, PDFium reopens. Infrastructure
  shipped with a working proof-of-consumer beats infrastructure shipped on faith.
- **PDFium compresses streams on save — your bytes aren't grep-able.** The tag is plainly visible
  in lopdf-written bytes, but once the actor's save path (PDFium `save_to_bytes`) runs, content
  streams come out Flate-compressed and a raw `grep "/VibePDF"` finds nothing. The tag *is*
  still there — at the operator layer after decoding. Lesson: verify content-stream facts with a
  decoder, never with byte search on a saved file.
- **Consume-and-return beats format-borrow.** clippy's `needless_pass_by_value` on
  `wrap_decoration(kind, content: String)`: `format!("{content}")` only *borrows* content, so
  taking `String` was dishonest. `insert_str(0, header)` + `push_str("EMC\n")` actually consumes
  and reuses the allocation — the signature now tells the truth.

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/src/pdf/cos.rs` | `wrap_decoration(kind, content)` — the `/VibePDF` BDC/EMC wrapper. |
| `watermark.rs` / `background.rs` / `header_footer.rs` | wrap their fragment with kind `watermark` / `background` / `header-footer`. |
| writer test suites + `tests/hardening.rs` | 3 tag tests + `decoration_tag_is_operator_spliceable` (the removal proof). |

#### Further reading

- PDF 32000-1:2008 §14.6 (Marked content) — `BMC`/`BDC`/`EMC`, inline property lists.

---

## P4.HF3 — WinAnsi text + error toasts

#### Problem

Two review findings that pair naturally. 3.2: text writers emitted raw UTF-8 into base-14-font
literal strings, so anything past ASCII rendered as mojibake — silently. 3.5: when a canvas tool's
backend write failed, the only trace was a `console.warn`; to the user the click did nothing.
They ship together because "reject bad text loudly" needs somewhere loud to land.

#### Concepts learned

- **A PDF string's bytes are interpreted in the font's encoding, not UTF-8.** The base-14 fonts
  default to *StandardEncoding*; to render `é` you must (a) declare `/Encoding /WinAnsiEncoding`
  on the font, and (b) put the WinAnsi *byte* (0xE9) in the string. Both, or neither works. The
  transcoder emits `\351` (octal 0xE9) and the font dict now carries the encoding — the two are a
  matched pair, which is why a single `base14_font_dict` builder (not six inline dicts) matters:
  it makes the encoding impossible to forget.
- **CP1252 is WinAnsi's superset of Latin-1.** Beyond U+00A0–U+00FF (Latin-1), the 0x80–0x9F
  block carries the "smart" punctuation users actually type: curly quotes, en/em dashes, €, …, ™.
  Mapping those (U+2019 → 0x92, U+2014 → 0x97, U+20AC → 0x80) covers the overwhelming majority of
  real-world "non-ASCII" text without any font embedding.
- **Reject at the boundary, render past it.** `ensure_winansi` runs at each writer's *entry* and
  fails the whole operation with a character-naming message; by the time `escape_pdf_string` runs
  at emit time, unmappable characters are unreachable (it defensively maps them to `?`). Splitting
  "can we?" (validate, user-facing error) from "do it" (escape, infallible) keeps each simple.
- **Finish the wire you already built.** The typed-error chain (`CommandError` → `invoke.ts` →
  `.code`) existed end-to-end but the frontend dropped it into `console.warn` at the last hop.
  `reportError` is ~10 lines: map the code to copy (our `InvalidInput` messages are already
  user-authored, so show them verbatim; other codes get a context prefix), push a toast, still
  log for devs. The lesson: when errors vanish, look for where a good pipe stops one node short of
  the UI.
- **Know what your verifier does and doesn't see.** Apple PDFKit's `page.string` extracts *page
  content* text — so the watermark and footer "Café résumé / Página … –" proved the transcode
  end-to-end — but not *annotation appearance* text, so the free-text "naïve € 5" simply wasn't in
  the string (not a bug). Reading a null result correctly is as important as reading a positive
  one.

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/src/pdf/cos.rs` | `winansi_byte` + `ensure_winansi` (reject) + `escape_pdf_string` (transcode) + `base14_font_dict` (the one WinAnsi font builder); collapsed the old `pdf_escape`. |
| `watermark.rs` / `header_footer.rs` | `ensure_winansi` at entry; shared font builder. |
| `src/state/toast-store.ts`, `src/app/Toasts.tsx`, `src/app/report-error.ts` | the toast surface + `CommandError`→copy mapper (mounted in `App.tsx`). |
| ~13 view files + `PdfViewer.tsx` | 21 user-action `console.warn` catches → `reportError`. |
| `tests/winansi.rs` + toast/report-error frontend tests | transcode/encoding/reject (9) + toast + mapping (9). |

#### Further reading

- PDF 32000-1:2008 §9.6.6.4 + Annex D (WinAnsiEncoding) — the CP1252 code table.
- PDF 32000-1:2008 §7.3.4.2 — literal strings and octal escapes.

---

## P4.HF4 — Recursion → worklist on the untrusted resource walk

#### Problem

`background.rs::collect_refs` computes the transitive object closure of a page's `/Resources` so
D1b can copy exactly that subtree into the destination. It's the one Track-D code path that parses
**a PDF VibePDF didn't produce** — the user picks the source file. It recursed once per graph edge,
so a hostile source with a deep reference chain (`obj 1 → 2 → … → 100k`) could overflow the actor
thread's stack and kill the document. Low likelihood, but a hard crash on attacker-influenceable
input is worth a ~15-line fix.

#### Concepts learned

- **Never recurse on untrusted structure.** Depth you don't control belongs on the heap, not the
  call stack. The fix is two explicit worklists: `pending: Vec<ObjectId>` walks the *reference*
  chain (resolve an id, enqueue its children's ids), and a per-object `inline: Vec<&Object>` walks
  each object's nested arrays/dicts. Both loops are `while let Some(x) = stack.pop()`. The `acc`
  set still guards cycles, so every id is resolved exactly once — the change is purely *where the
  depth lives*, not what gets visited.
- **A lifetime wrinkle forces the two-list shape.** The naive single `Vec<&Object>` worklist won't
  type-check: the seed object borrows a caller *temporary* (`&Object::Dictionary(resources.clone())`)
  while `doc.get_object(id)` returns a borrow of `doc` — two different lifetimes that can't share
  one `Vec`. Splitting into "ids to resolve" (owned `ObjectId`, no lifetime) and a *fresh, locally
  scoped* inline stack per object sidesteps it and, bonus, keeps the walk allocation-cheap (no
  cloning resolved objects, unlike an owned-object worklist would).
- **lopdf `get_object` transparently collapses bare-reference chains.** This bit the first test:
  `N 0 obj  M 0 R` links resolve straight through to the final object, so a bare-ref chain never
  recurses — my initial 50k-bare-ref fixture reported a closure of *one*. The genuine overflow
  shape (and the correct hostile model) is a chain of **containers**, `<< /Next n+1 0 R >>`, which
  `get_object` returns as-is. Read a "too small" result as a signal that your model of the library
  is wrong, not that the code is.
- **Put the regression where it's fast.** Driving a 100k-deep chain through the whole
  `add_background` round-trip took ~60 s — dominated by lopdf renumber/serialize over 100k objects,
  not by `collect_refs`. Calling the private function directly from a `#[cfg(test)] mod tests`
  (build graph → `collect_refs` → assert `acc.len()`) runs in 0.3 s and targets exactly the
  behavior under change. Test the unit, not the universe around it.

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/src/pdf/background.rs` | `collect_refs` rewritten to `pending`/`inline` worklists + `push_child_refs` helper; inline `mod tests` (deep-chain + cycle). |
| `src-tauri/tests/background.rs` | note pointing at the unit test (removed the slow round-trip attempt); existing suite still guards transitive copy. |

#### Further reading

- PDF 32000-1:2008 §7.3.10 — indirect objects and the `R` reference syntax (why a chain is legal).

---

## P4.HF5 — Font embedding through the engine you already have

#### Problem

HF3 made non-WinAnsi text fail *loudly* instead of corrupting silently, but the built-in base-14
fonts still can't draw CJK / Cyrillic / Greek / … Stage-2 is the real fix: embed a font that
covers the glyphs. The catch: real font embedding means parsing TrueType (cmap, glyf, hmtx,
subsetting) — exactly the dependency `docs/03_TECH_STACK.md` deliberately refuses.

#### Concepts learned

- **The dependency you're avoiding may already be linked.** PDFium — bound for rendering and a few
  mutations — contains a complete font engine. `fonts_mut().load_true_type_from_bytes(bytes, cid)`
  + `objects_mut().create_text_object(...)` embeds a font as a `/Type0` `/CIDFontType2` with a
  `/ToUnicode` map and `/FontFile2`, no Rust font crate needed. Before reaching for a new
  dependency, check whether a heavyweight you already carry exposes the capability. Here it let us
  honour a documented "no font parser" stance *and* ship Unicode.
- **Spike the one thing that can sink the approach, first.** The whole plan rested on "does a
  PDFium-loaded font survive our `save_to_bytes` → reload round-trip, with the text still
  extractable?" That was increment 1, in isolation, ~40 lines. It passed in a minute — and every
  later increment built on proven ground instead of hope. If it had failed, I'd have thrown away
  40 lines, not a feature.
- **Branch at the predicate, don't rip out the working path.** `ensure_winansi` (reject) became
  `winansi_fits` (predicate). WinAnsi text still takes the cheap, unchanged, fully-tested base-14
  lopdf path; only genuinely non-WinAnsi text pays for embedding. A plain "Page 1 of 10" footer is
  byte-identical to before. Two backends, chosen per-string, beat one backend that's worse at both
  jobs.
- **`get_object` transparency and font size were both "read the null result" moments.** As in HF4,
  a first attempt that returned something surprising (here: a 15 MB output) was the library telling
  me how it actually behaves — PDFium embeds the *whole* font, it does not subset on save. That's
  not a bug to fix under deadline; it's a constraint to name loudly (the top follow-up) and design
  around later (self-subsetting, or small per-script faces).
- **Reuse the geometry, not the code.** The lopdf writers lay text in visual space via
  `visual_transform` to survive `/Rotate` and CropBox. The PDFium path is a totally different
  backend, but the *matrix* is portable: compute the run's visual origin, push it through the same
  `[a b c d e f]`, and hand PDFium the composed matrix as the text object's transform. Rotation
  support fell out for free — no second rotation implementation.

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/src/pdf/font_embed.rs` | New. `embed_runs` — the PDFium load-font + place-text-objects primitive (dumb: draws at a caller matrix), under the reflow.rs chassis. Inline round-trip + preserve-existing tests. |
| `src-tauri/src/pdf/cos.rs` | `winansi_fits` predicate (the branch half of `ensure_winansi`). |
| `src-tauri/src/pdf/header_footer.rs` | The tracer consumer: non-WinAnsi → `add_header_footer_embedded` (visual-space matrices → `embed_runs`); WinAnsi keeps the base-14 path. Inline deterministic embed test. |
| `src-tauri/src/pdf/font_resolver.rs` | `covering_font_bytes` — best-effort broad system-face locator (coverage-checking deferred). |
| `tests/fixtures/fonts/NotoSansCoptic-Regular.ttf` (+ OFL notice) | 28 KB committed OFL font for a deterministic, offline embed regression. |
| `src-tauri/tests/winansi.rs` | `non_winansi_header_footer_rejected` → `…_now_embeds` (tracks the intentional behaviour change). |

#### Further reading

- PDF 32000-1:2008 §9.7 — composite (Type 0 / CIDFont) fonts and `/ToUnicode` CMaps.
- PDFium `FPDFText_LoadFont` / `FPDFPageObj_CreateTextObj` — the C API pdfium-render wraps.

---

## P4.HF6 — Font subsetting: the size fix HF5 left behind

#### Problem

HF5 shipped Unicode footers — but a Cyrillic footer came out **15 MB**, because PDFium embeds the
*whole* face and doesn't subset. The fix is to embed only the glyphs the text uses, which means
parsing the font — exactly the dependency `docs/03` had refused.

#### Concepts learned

- **The engine *has* the feature; the binding hides it.** PDFium exposes `FPDF_SUBSET_NEW_FONTS`,
  a save flag that subsets newly-added fonts — the zero-dependency fix. But `pdfium-render` 0.9.1
  hardcodes `flags = 0` in `save_to_writer` and keeps the document handle + the `FPDF_FILEWRITE`
  callback `pub(crate)`, so there's no way to reach it. Lesson: "the capability exists in the C
  library" and "the capability is reachable from safe Rust" are different questions — check the
  *binding's* surface, not just the engine's.
- **When you must add the dependency you swore off, add the smallest one — and verify the tree,
  don't trust the pitch.** I told the user `subsetter` was "tiny," then `cargo add` resolved
  `subsetter 0.2.6`, which drags in the whole **fontations** stack (skrifa/read-fonts/write-fonts/
  kurbo/euclid/…, 11 crates) *and* needs rustc 1.85 > our 1.80 MSRV. Pinning `subsetter 0.1` gave
  the crate I actually meant: **zero transitive dependencies**, MSRV-clean. Always `cargo tree` +
  check `license` + check the MSRV note after adding, before believing your own justification.
- **The subset must stay self-consistent for whoever renders it.** `subsetter`'s PDF profile keeps
  *original* glyph-ids (emptying the unused ones) and preserves the `cmap`. That's the property
  that lets us keep the whole HF5 PDFium path unchanged: PDFium re-runs its Unicode→GID cmap lookup
  on the subset and finds the (same-numbered) kept glyph. Had it renumbered glyphs or dropped the
  cmap, PDFium would render `.notdef` and I'd have been forced into building the CID font by hand.
  The one-line spike (`subset_font` → `embed_runs` → re-extract) settled that before any wiring.
- **Degrade to correct, not to broken.** `subset_font` returns the *full* font on any parse/subset
  error (odd container, `.ttc`, subsetter edge case). A bloated-but-correct embed always beats a
  hard failure or a corrupt font — the same "never silently break a PDF" rule the save path lives
  by, applied one layer down.

Result: the same footer went **15 MB → 60 KB** (~256×), with the render byte-identical.

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/Cargo.toml` | `subsetter = "0.1"`, `ttf-parser = "0.25"` (both MIT/Apache, zero-dep; justified inline + in docs/03). |
| `src-tauri/src/pdf/font_embed.rs` | `subset_font` (codepoints → gids via ttf-parser → `subsetter::subset`); `embed_runs` subsets before `load_true_type_from_bytes`. Spike/regression test: subset shrinks + still round-trips. |

#### Further reading

- `subsetter` docs — `Profile::pdf`, what tables it keeps/drops for embedding.
- PDFium `fpdf_save.h` — `FPDF_SUBSET_NEW_FONTS` and the save-flag bitmask (the road not taken).

---

## P4.HF7 — Second writer onto the embed path: watermark

#### Problem

HF5/HF6 proved font embedding on *one* writer (header/footer). Watermark is the second — but it
has three things header/footer didn't: **opacity**, **arbitrary rotation**, and **behind-vs-on-top**
z-order. The base-14 path did these with an `/ExtGState`, a rotation `cm`, and `prepend` vs
`append`. The PDFium embed path builds *objects*, not a content stream, so each needed a different
lever.

#### Concepts learned

- **A tracer's real payoff is the second consumer.** The first embed writer (header/footer) proved
  the mechanism; the second told me whether the *seam* was right. `EmbedRun` grew two fields
  (`opacity`, `behind`) and the shared primitive absorbed both — no rewrite. That the abstraction
  bent instead of breaking is the signal the tracer-bullet was drawn in the right place. The
  remaining four writers should now be nearly free.
- **Find the object-level lever for each content-stream trick.** Content streams do opacity with an
  `/ExtGState /ca`, z-order by *where* you concatenate, rotation with a `cm`. PDFium objects have
  one-to-one equivalents: fill **alpha** on the colour (`FPDFPageObj_SetFillColor` takes it and
  emits the `/ca` for you), **`insert_object_at_index(0)`** for "behind", and the object's own
  **matrix** for rotation. Same PDF concepts, different API surface — the trick is knowing they map.
- **Bake stacked `cm`/`Td` into one matrix with a compose helper.** The base-14 watermark applies
  `vt cm` · `R@centre cm` · `Td(-w/2,-size/3)` — three transforms. A PDFium object takes exactly one
  matrix, so `cos::compose(a, b)` (apply `a` then `b`, i.e. `p·a·b`) collapses the stack:
  `compose(compose(T, R), vt)`. Deriving it once and unit-testing it (identity is a no-op,
  translate∘translate adds) made both header/footer and watermark share the geometry instead of
  each hand-rolling it.
- **Behind-insert forced a lifetime tie.** Creating a *detached* text object (`PdfPageTextObject::new`,
  needs `&doc`) and inserting it into the page (needs `&mut PdfPage`) made the borrow checker
  demand a shared `'a` across `doc` and `page` — because a `&mut PdfPage<'2>` is invariant. One
  named lifetime on `place_run<'a>` fixed it. Mutable references being invariant over their type
  parameter is the kind of thing you re-learn exactly when a two-borrow function stops compiling.

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/src/pdf/font_embed.rs` | `EmbedRun` + `opacity`/`behind`; `place_run` branches append vs `insert_object_at_index(0)`, alpha from opacity. Spike tests: opacity round-trips, behind precedes content. |
| `src-tauri/src/pdf/cos.rs` | `compose` — shared 2D affine multiply (with a unit test in watermark). |
| `src-tauri/src/pdf/watermark.rs` | `winansi_fits` branch → `add_watermark_embedded` (rotate-about-centre matrix, opacity, behind). WinAnsi keeps the ExtGState path. Inline tests. |
| `src-tauri/tests/winansi.rs` | `non_winansi_watermark_rejected` → `…_now_embeds`; `error_names_the_offending_characters` repointed to a still-rejecting writer (text box). |

#### Further reading

- PDF 32000-1:2008 §8.3.3 — coordinate transforms and `cm` composition order.

---

## P4.HF8 — Third writer: text box (wrapping + underline)

#### Problem

The third text writer, text box, wraps text across multiple lines and can underline it. The
base-14 path did both in one content-stream fragment (a `BT … T* … Tj … ET` with `re`/`S` rules).
The PDFium embed path builds *objects*, and a text object is a single string with no underline —
so multi-line and underline each needed decomposing.

#### Concepts learned

- **Reuse the layout function, not just the idea.** The base-14 text box and free-text share
  `free_text_appearance_content`, and its wrapping lives in a standalone `wrap_lines(text, size,
  em, max_width) -> Vec<String>`. The embed path calls the *same* `wrap_lines` on the *same*
  `free_text_inner_width`, then emits one `EmbedRun` per line at `y_top - i*leading`. Because both
  paths consume the identical layout primitive, a page flips between base-14 and embedded with the
  same line breaks — no "two wrapping implementations that drift" bug waiting to happen.
- **Underline is a path object, and it rides the text matrix.** PDFium has no text-underline
  property, but `create_path_object_line(x1,y1,x2,y2, stroke, width)` draws the rule. Emitting it
  in the run's *local* space — `(0, -size*0.12)` to `(width, -size*0.12)` — then `apply_matrix`ing
  the run's matrix puts it under the glyphs wherever the run lands (and would follow rotation, if a
  future writer rotates underlined text). Modelling underline as `EmbedRun.underline: Option<f32>`
  (the rule width) kept the primitive's shape: text-plus-optional-rule, one object each.
- **Every conversion re-points the "still rejects" test.** `winansi.rs`'s
  `error_names_the_offending_characters` proves `ensure_winansi` names ≤3 offenders — it needs a
  writer that *still* rejects. HF7 pointed it at text box; HF8 makes text box embed, so it moved to
  free-text. There's a lesson in the churn: a test asserting "X still rejects" is really asserting
  a *shrinking* set, and each stage-2 ship must walk it to the next un-converted writer. When the
  last one converts, that test graduates to calling `ensure_winansi` directly.

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/src/pdf/font_embed.rs` | `EmbedRun.underline: Option<f32>`; `place_run` draws the rule via `create_path_object_line` under the matrix. Spike test: underline adds exactly one path object. |
| `src-tauri/src/pdf/cos.rs` | `add_text_box_embedded` — `wrap_lines` → one run per line + underline width; `winansi_fits` branch in `add_text_box`. Inline tests (wrap, underline count, base-14 unchanged). |
| `src-tauri/tests/winansi.rs` | `non_winansi_text_box_rejected` → `…_now_embeds`; `error_names` repointed text-box → free-text. |
| `src-tauri/tests/text_box.rs` | New: the embedded-Unicode artifact (multi-line Russian + underline). |

#### Further reading

- PDF 32000-1:2008 §9.4.3 — text-showing operators and `TL`/`T*` leading (what the per-line runs replace).

---

## P4.HF9 — Building a CID font by hand (the `/AP` class)

#### Problem

Free-text and stamps draw their text inside an annotation's `/AP` appearance stream — a Form
`XObject` with its own `/Resources`. PDFium's font-embedding creates text objects on a *page*, not
inside an `/AP`. So the entire HF5–HF8 approach doesn't reach here. The real fix is to build a
`Type0` / `CIDFontType2` font **by hand in lopdf** and write the appearance content with
GID-indexed strings — the largest single stage-2 piece.

#### Concepts learned

- **When the high-level tool can't reach, drop to the format.** A CID font is a specific object
  graph: `/FontFile2` (the subset bytes) ← `/FontDescriptor` (flags, bbox, ascent/descent/cap from
  the face) ← `CIDFontType2` (`/CIDToGIDMap /Identity`, `/W` widths, `/DW`) ← `Type0`
  (`/Encoding /Identity-H`, `/DescendantFonts`, `/ToUnicode`). Assembling it is fiddly but
  mechanical — the same shape printpdf / typst / pdf-writer all emit. Reading one working example's
  dict layout is worth more than the spec prose.
- **Identity-H means the 2-byte code *is* the glyph id.** With `/Encoding /Identity-H` and
  `/CIDToGIDMap /Identity`, you don't encode characters — you encode *glyphs*: map codepoint → gid
  (`ttf-parser`), write the gid as 4 hex, and the `/ToUnicode` CMap carries the reverse mapping so
  copy/search still works. `subsetter` preserving original gids (HF6) is what makes this valid: the
  gids you write still index the subset's `glyf`.
- **Two things must agree or the text vanishes.** The `<gid> Tj` codes, the subset's glyph table,
  and the `/ToUnicode` entries all key off the *same* glyph ids. Get one out of step and the glyph
  renders but won't copy, or copies but renders `.notdef`. The spike — put the font in page content,
  reopen, extract — is the single test that proves all three agree at once; it passed first try,
  which is the payoff for matching a known-good layout exactly.
- **Spike the risky thing in the *reachable* place.** I couldn't easily extract `/AP` text via
  PDFium (annotation text isn't page text), so the spike put the hand-built font in **page content**
  — same font, same encoding, extractable — proving the font works before wiring the harder `/AP`.
  De-risk where you can observe, then wire where you can't.
- **The plain text lives in `/Contents`, not the appearance.** Re-editing a free-text reads the
  annotation's `/Contents` (the literal Unicode string) and regenerates the `/AP` from scratch — so
  embedding a CID font in the `/AP` never traps the text. Keeping the source-of-truth text separate
  from its rendered form is what makes lossy-appearance embedding safe to re-edit.

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/src/pdf/font_embed_cid.rs` | New. `build_cid_font` — the hand-built `Type0`/`CIDFontType2` + `/ToUnicode`; `encode_hex` / `width`. Spike: PDFium renders + extracts it. |
| `src-tauri/src/pdf/cos.rs` | `free_text_appearance` → `Result` + `winansi_fits` branch; `free_text_appearance_embedded` + `free_text_cid_content` (GID-hex `Tj`, real-advance underline). Callers drop the `ensure_winansi` gate. Inline tests. |
| `src-tauri/tests/winansi.rs` | free-text `…_now_embeds`; `error_names` repointed free-text → stamp (the last reject-path writer). |
| `src-tauri/tests/free_text.rs` | embedded-Unicode artifact. |

#### Further reading

- PDF 32000-1:2008 §9.7.4 (CIDFonts), §9.7.5 (CMaps), §9.10.3 (`/ToUnicode`).

---

## P4.HF10 — The closer: stamp labels, and a test that graduated

#### Problem

The last two text writers — text stamps and image-stamp labels — both draw a single uppercased
label in an `/AP`. With HF9's `build_cid_font` in hand, converting them is small; the interesting
part is *finishing* — what the last conversion in a series reveals.

#### Concepts learned

- **Vary the one thing that differs; a closure is the cleanest seam.** Both stamp appearances
  differed from their base-14 selves in exactly one place: the `Tj` operand (`(LITERAL)` vs.
  `<gidhex>`). Threading a `show: Fn(&str) -> String` closure through the content builder — base-14
  passes `|u| format!("({})", escape_pdf_string(u))`, CID passes `|u| format!("<{}>", cid.encode_hex(u))`
  — kept the uppercasing + auto-size math in one place and touched the base-14 path minimally. When
  two paths share 95% of a function, parameterise the 5%, don't fork it.
- **Uppercasing is script-aware; embed the *rendered* text.** Stamps `to_uppercase()` the label, and
  Cyrillic/Greek have case (е→Е). The CID font must cover the *uppercased* glyphs and the branch must
  test `winansi_fits(&label.to_uppercase())`, not the raw label — otherwise a lowercase-WinAnsi label
  that uppercases out of range would slip to the wrong path. Build for what you draw.
- **A "still rejects" test graduates when the last case converts.** `error_names_the_offending_characters`
  has been chasing the shrinking set of reject-only writers since HF7 (free-text → text-box →
  free-text → stamp). HF10 converts stamp, so *no* writer rejects unconditionally any more — the test
  had nowhere left to point. It graduated to calling `ensure_winansi` **directly** in a cos unit
  test. That migration is the signal the series is complete: the behaviour that used to be observable
  only *through* a writer is now tested at its source.

Stage-2's writer surface is done: **all 7 rendered-text entries embed Unicode.**

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/src/pdf/cos.rs` | `stamp_appearance_content` / `image_stamp_content` take `show: Fn(&str)->String`; `stamp_label_appearance` + `add_stamp` / `add_image_stamp` branch on `winansi_fits`. Inline `stamp_embed_tests` (incl. the graduated `ensure_winansi` naming test). |
| `src-tauri/tests/winansi.rs` | `error_names` → removed (graduated); `non_winansi_stamp_now_embeds` added. |
| `src-tauri/tests/stamp.rs` | image-stamp Unicode-label test + embedded-Unicode artifact. |

#### Further reading

- (Same CID/CMap references as P4.HF9.)

---

## P4.HF11 — Stream compression (investigation → NON-ISSUE, no code shipped)

#### Problem

FABLE_REVIEW §3.12 claimed our new content streams (imported PDF-page backgrounds, `/AP`
appearance streams, embedded `/FontFile2` fonts) were written **uncompressed**, so files grow
faster than they need to. The planned fix was a shared `add_flate_stream` helper.

#### Concepts learned

- **Who compresses a PDF stream?** A PDF stream can carry a `/Filter` (e.g. `/FlateDecode`) that
  says "my bytes are deflated." Two layers could add it: *you*, when you build the `Stream`
  object, or the *serializer*, when it writes the document to disk. We assumed neither did — wrong.
- **lopdf compresses on save by default.** `lopdf::Document::save_to` runs each stream through
  flate2 as it serializes, *unless* a stream opts out. That opt-out is exactly why
  `image_xobject.rs` calls `.with_compression(false)` — an already-deflated JPEG/PNG must NOT be
  re-deflated. That single opt-out was the tell we'd misread the default.
- **Measure before you optimise.** Regenerating a real embedded-font artifact with our explicit
  `compress()` *disabled* still produced `/FontFile2 … /Length 20893 /Length1 332560
  /Filter /FlateDecode` — a 332 KB subset stored as 20 KB (94% smaller) with zero work from us.
  Forcing `Compression::best()` moved 20893 → 20365 B (~2.5%). The "fix" was redundant.
- **Ship discipline: a false premise is a stop, not a pivot.** Rather than quietly re-scope HF11
  to "squeeze 2.5% more," the change was reverted and the finding recorded here + in §3.12. A
  commit whose stated rationale is false is worse than no commit.

#### Files in this step

| File | Role |
|---|---|
| `FABLE_REVIEW.md` | §3.12 marked **NON-ISSUE** with the measured evidence; summary tables updated. |
| `BACKLOG.md` | "`/FontFile2` compression" follow-up struck through — done for free by lopdf. |
| *(no `src/` changes)* | Prototype (`add_flate_stream` + routing) was reverted via `git checkout`. |

#### Further reading

- lopdf `Document::save_to` / `Stream::set_content` + compression (crate docs).
- PDF 32000-1:2008 §7.4 (Filters) — `/FlateDecode` and the `/Filter` entry.

---

## P4.HF12 — Dirty-flag correctness (FABLE_REVIEW §3.11)

#### Problem

The actor tracked "are there unsaved changes?" with a plain `bool`. Two edges
were wrong: undoing back to the exact saved state still reported dirty, and a
save-as never cleared dirty. The visible cost was a **false "Recover unsaved
changes?" prompt** on the next launch (a stale autosave copy) plus needless
file rewrites.

#### Concepts learned

- **Derive state, don't track it in parallel.** A separate `dirty` bool has to
  be poked by hand at every edit/undo/redo/save site (37 sites here) and drifts
  from reality at the edges. Instead we *derive* it from the one thing that
  already knows the document's state — the undo history.
- **A monotonic state id beats a depth counter.** The review suggested a
  `generation` counter that increments on edit and decrements on undo. That's a
  *depth* — and it has a **false-clean** bug: save at depth 3, undo to 2, make a
  new edit, and you're back at depth 3 but on a *different branch*, yet it
  compares equal to the saved depth. The fix is to mint a **unique id per
  state** that is never reused (`next_id` only ever increases). Two states at
  the same stack depth on different branches get different ids.
  - *False-clean* is the dangerous direction: reporting "saved" when you aren't
    can lose work on close. Prefer designs whose failure mode is a *false-dirty*
    (a harmless extra save prompt).
- **Watch the eviction edge.** The undo stack is capped (`MAX_UNDO_DEPTH`). If
  you derive "pristine" from *"undo stack is empty,"* then editing past the cap
  and undoing everything still on the stack falsely reports pristine — the
  evicted edits are still applied. Keeping `current_id` as an explicit field
  (each entry stores the id it *returns to*) makes the floor a real, non-zero
  id, so it stays dirty. This is exactly the case a "derive from `back()`"
  shortcut gets wrong.
- **Command-pattern payoff.** Because undo/redo already move between states,
  once the id lives in `UndoStack`, the actor's undo/redo handlers need *zero*
  dirty bookkeeping — the flag falls out of `current_state_id() != saved_state_id`.

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/src/pdf/undo.rs` | Stacks now carry `(return_to_id, inverse)`; `next_id` + `current_id` fields; new `current_state_id()`. Unit tests for pristine/monotonic/undo-return/redo-restore/branch/eviction. |
| `src-tauri/src/pdf/actor.rs` | `dirty: bool` → `saved_state_id: u64`; deleted 37 `dirty = true`/`false` pokes; save no-op + clear + recovery-discard now key off the state id (and clear on save-as); autosave writes iff `current != saved`. |
| `src-tauri/tests/save_noop.rs` | `undo_to_saved_state_is_true_noop` (bug a), `save_as_then_same_path_save_is_noop` (bug b, with the scoped path-quirk note). |

#### Further reading

- Command pattern for undo/redo (Gamma et al., *Design Patterns*).
- The general "derived vs. duplicated state" idea (single source of truth).

---

## P4.HF13 — Bounding undo memory by a byte budget (FABLE_REVIEW §3.6)

#### Problem

Undo was capped by *count* (`MAX_UNDO_DEPTH = 100`) but not by *memory*. Since
nearly every edit's inverse is a full-document byte snapshot
(`RestoreDocEdit { bytes: Vec<u8> }`), a 150 MB scan × a long session could
pin multiple GB — breaching NFR-PERF-002 (< 1 GB for a 100-page doc) and
NFR-PERF-003 (open 500 MB without exhausting memory).

#### Concepts learned

- **A count cap is not a memory cap.** "Keep the last N actions" bounds memory
  only if each action costs about the same. When one action can retain a whole
  document, the meaningful bound is *bytes*, not *entries*. Cap by the quantity
  you actually care about.
- **Let each item price itself.** Adding `fn heap_bytes(&self) -> usize { 0 }`
  to the `Edit` trait (default `0`, overridden by `RestoreDocEdit`) lets the
  stack sum a running total without knowing anything about concrete edits. A
  **defaulted trait method** is a non-breaking extension — the other ~35 `Edit`
  impls compile untouched.
- **Prove the invariant, then lean on it.** The budget is enforced only in
  `record` (the one path that *grows* total memory). `undo`/`redo` just move an
  inverse between stacks and produce an equal-sized inverse, so they *conserve*
  the total — which means the redo stack never needs its own eviction, and
  total memory stays ≤ budget by construction. Spotting that conservation law
  let the change stay smaller than the plan first assumed (no `VecDeque`
  conversion of the redo stack).
- **Pick a humane failure mode.** Rather than make the newest edit
  non-undoable when a single snapshot exceeds the whole budget, keep ≥1 entry
  always. You bound memory *and* never silently drop the user's most recent
  action.

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/src/pdf/undo.rs` | `MAX_UNDO_BYTES` (256 MiB) + `heap_bytes()` on the `Edit` trait; `UndoStack` tracks `undo_bytes`/`redo_bytes` and evicts oldest undo entries over budget (keeps ≥1). 5 new unit tests. |
| `src-tauri/src/pdf/restore.rs` | `RestoreDocEdit::heap_bytes` → `self.bytes.len()` — the dominant cost. |

#### Further reading

- Cache eviction policies (LRU / size-based) — the same "bound by cost, evict oldest" idea.
- Rust default trait methods (non-breaking API evolution).

---

## P4.HF14 — A strict webview CSP (FABLE_REVIEW §3.8)

#### Problem

The Tauri webview ran with `app.security.csp: null` — **no** Content-Security-
Policy. That webview renders arbitrary PDFs (via PDF.js) *and* can call our Rust
IPC, so a PDF.js escape or an XSS in any rendered string (document title/author
metadata is shown) had an unfenced, IPC-capable page.

#### Concepts learned

- **CSP is an allowlist for where content may come from.** `default-src 'self'`
  means "only load from my own origin," then each directive (`script-src`,
  `img-src`, `connect-src`, `worker-src`, `style-src`, …) narrows or widens a
  class of loads. The skill is granting the *fewest* extra sources that still let
  the app work — every relaxation is attack surface.
- **Earn each relaxation from a real resource.** We mapped every load the
  frontend makes to exactly one directive: same-origin PDF.js worker →
  `worker-src 'self'`; blob thumbnail `<img>` → `img-src blob:`; Tauri IPC →
  `connect-src ipc: http://ipc.localhost`; Tailwind/React inline styles →
  `style-src 'unsafe-inline'`. Nothing speculative.
- **`'wasm-unsafe-eval'` ≠ `'unsafe-eval'`.** PDF.js v5 stopped using JS `eval`
  for PDF functions and moved to QuickJS-in-WASM, and it decodes JBIG2/JPEG2000
  images via WASM. Instantiating WASM needs `'wasm-unsafe-eval'` — a *narrow*
  token that permits WASM compilation only, not arbitrary JS `eval`. Reaching for
  the narrower capability is the whole game in a CSP.
- **Dev tooling fights strict CSP.** Vite's HMR uses a websocket and injects an
  inline preamble script — both blocked by a strict policy. Tauri v2's answer is
  a separate `devCsp` (used only in dev) so production stays locked down while
  `npm run dev` still hot-reloads. If `devCsp` is omitted, the prod `csp` also
  applies in dev and HMR silently breaks.
- **Some correctness is only observable at runtime.** CSP is enforced by the
  webview, not the compiler or the test runner — a wrong policy blanks the app and
  *no* unit test sees it. The honest response: a cheap config-regression guard
  (`csp.test.ts` asserts the policy shape) **plus** an explicit manual in-app
  smoke test. Don't pretend a green suite verifies a runtime control.

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/tauri.conf.json` | `csp: null` → strict `csp` + a dev-relaxed `devCsp`. |
| `src/__tests__/csp.test.ts` | Parses the config and asserts the CSP shape (prod strict, dev adds only HMR sources) — a regression guard, not a runtime check. |

#### Further reading

- MDN: Content-Security-Policy (directives + source keywords).
- Tauri v2 security config (`csp` / `devCsp`).

---

## P4.HF15 — Windows path display + a Windows CI leg (FABLE_REVIEW §3.9)

#### Problem

Filename-for-display used `sourcePath.split("/")`, which on Windows (`C:\…\file.pdf`,
no `/`) returns the *whole path*. And CI only ran on macOS + a Linux throttle
gate, so Windows — a shipped target — was never built or tested, meaning bugs
like this couldn't be caught.

#### Concepts learned

- **`split("/")` is a portability trap.** Path separators are platform-specific
  (`/` POSIX, `\` Windows, both in UNC/mixed). A display base name must split on
  `/[\\/]/` or scan for the rightmost of either separator. The codebase already
  had a correct `basename` in `src/app/paths.ts` — the bug was **copy-paste
  drift**: dialogs re-implemented it inline and one branch got it wrong.
- **Look for the existing helper before writing a new one.** The plan assumed no
  shared util existed and proposed a new `src/tools/path.ts`; mid-implementation
  the canonical `@/app/paths.basename` turned up. Adding a second util would have
  *deepened* the duplication the fix was meant to remove — so I stopped, flagged
  the plan error, and consolidated onto the existing one instead.
- **Consolidation is bounded by layering.** `src/tools/` never imports `@/app`
  (tools is a lower layer than the app shell). So the one `tools` consumer
  (`stamps.ts`) keeps its own correct inline split rather than reach *up* into
  `@/app` — de-duplication stops at the layer boundary, it doesn't justify
  breaking it.
- **Test the platform you can't run.** A `basename` unit test with Windows/UNC
  inputs proves the fix deterministically on any OS. Then a `windows-latest` CI
  job actually runs that test (and compiles all Rust) on the real platform —
  `cargo clippy --all-targets` on Windows is the cheapest way to surface
  Windows-specific compile/`#[cfg]` issues.
- **Scope a CI leg to what's portable.** The full Rust PDF suite couldn't run on
  Windows yet (`fetch-pdfium.sh` has no Windows branch; the render golden is
  macOS-arm64-specific). `check` + frontend tests need neither — and
  `pdfium-render` binds at *runtime*, so clippy compiles without the binary.
  Ship the high-value portable slice now; defer the rest with the blockers named.

#### Files in this step

| File | Role |
|---|---|
| `src/app/WatermarkDialog.tsx`, `BackgroundDialog.tsx` | The 3 buggy `split("/")` → the shared `basename`. |
| `src/app/MergeDialog.tsx`, `InsertFromDialog.tsx`, `src/view/PdfViewer.tsx` | De-duplicated their local `basename` onto `@/app/paths`. |
| `src/app/paths.ts` | Canonical `basename` (unchanged logic; comment updated for the wider consumer set). |
| `src/app/__tests__/paths.test.ts` | New: Windows/UNC/mixed-separator regression guard. |
| `.github/workflows/ci.yml` | New `check-windows` job (`check` + frontend tests; Rust PDF suite deferred). |

#### Further reading

- Node `path.win32` / `path.posix` (why separators are platform-scoped).
- GitHub Actions runner images (`windows-latest`).

---

## P4.HF16 — Exact base-14 glyph metrics for alignment (FABLE_REVIEW §3.10)

#### Problem

Centre/right alignment of header/footer and watermark text estimated string
width as `size × avg_em × char_count` with a flat `avg_em ≈ 0.6`. For a
*proportional* font that's wrong per-string ("WWW" vs "iii"), so right-aligned
dates drifted several points and centred titles sat slightly off.

#### Concepts learned

- **A base-14 font is laid out by the viewer's own metrics.** We never embed
  Helvetica/Times/Courier — we just reference `/Helvetica`. The viewer supplies
  the glyphs *and their advance widths*, which for a spec-compliant font are the
  Adobe **AFM** widths. So to place text where the viewer will, we must use those
  exact advances, not an average.
- **Proportional vs monospaced.** Courier advances every glyph 600/1000 em (a
  constant, no table). Helvetica/Times need a real 256-entry width table per
  face, indexed by the encoding byte (WinAnsi here).
- **Source data from a tool you already ship.** The AFM widths weren't available
  offline and we won't fabricate 2,048 integers. But **PDFium is already bundled**
  and lays base-14 text out using AFM-compatible metrics (its Foxit substitutes).
  A throwaway spike measured a few glyphs (`'A'`→667, `'W'`→944) and they matched
  Adobe AFM exactly — so a small `#[ignore]`d generator measures every glyph and
  writes `font_metrics/tables.rs`. Zero network, zero new deps.
- **Measure advance, not ink.** A glyph's *advance* (how far the pen moves) isn't
  its *ink* bounding box. `glyphs().width()` was a dead end (a non-embedded
  standard font reports 0 enumerable glyph outlines). The trick:
  `advance(c) = width("A" + c + "A") − width("AA")` — bracketing `c` between two
  fixed glyphs makes the bbox grow by exactly one advance, which also works for
  zero-ink glyphs like space.
- **Scope by risk, not by plan-completeness.** The plan also covered free-text
  *wrapping*, but that flows through a `wrap_lines` shared with the embedded-CID
  path — a riskier, isolated change for the lowest-visibility symptom (the box
  clips). Shipped the high-value alignment fix exactly; deferred the wrap with the
  infra (`text_width`) already in place.

#### Files in this step

| File | Role |
|---|---|
| `tests/gen_font_metrics.rs` | `#[ignore]`d generator: measures bundled PDFium, writes the tables. |
| `src/pdf/font_metrics/tables.rs` | GENERATED per-glyph AFM width tables (8 proportional faces). |
| `src/pdf/font_metrics.rs` | `text_width(base, text, size)` + Courier-600 handling + unit tests. |
| `src/pdf/cos.rs` | `winansi_byte` promoted to `pub(crate)` for the width lookup. |
| `src/pdf/header_footer.rs`, `watermark.rs` | Centre/right/centring now use `text_width`. |

#### Further reading

- Adobe Font Metrics (AFM) format + the Core-14 standard fonts.
- PDF 32000-1:2008 §9.2.4 (glyph positioning / advance widths).

---

## P4.HF17 — Assorted cleanups (FABLE_REVIEW §3.15)

#### Problem

The review's §3.15 collected nine small papercuts: a colour parser that rejected
CSS `#rgb`, a couple of TypeScript indirections, a re-export shim, framework
utilities misfiled under a specific tool, and an out-of-order module list.

#### Concepts learned

- **Robust-but-safe parsing.** `parse_hex_color` now accepts `#rgb` by doubling
  each digit — but validate the character set *before* the length branch, so the
  byte-slicing only ever runs on ASCII (a 3-*byte* non-ASCII char isn't 3
  *digits*). Order the guards so later code can assume what it needs.
- **Prefer the named type over a derived one.** `Parameters<typeof setHistory>[1]`
  works, but `HistoryState` (the actual wire type) is clearer and doesn't couple a
  handler's signature to a store method's shape. Import the real type.
- **A re-export shim is debt once its callers can point at the source.** The
  `parsePageRange` shim in `tools/watermark` had exactly one remaining consumer;
  repointing it and deleting the shim removes a layer of indirection.
- **Put utilities at the layer that owns them, not where they were born.**
  `normalizeScreenRect` / `withDefaultSize` are used by *every* rect-drawing layer
  (free-text, text box, link, image), so they belong in `tools/_framework`, not
  `tools/free-text`. A `grep` for one symbol can miss multi-line imports —
  `tsc` is the real safety net for a move refactor (it caught `free-text-layer`).
- **Know when a "small" item isn't small.** Four of the nine were *deferred with a
  reason*: they need other work (marked-content tagging), a UX policy decision, a
  UI restructure, or a feature that hasn't landed (D4). Bundling those into a
  grab-bag cleanup would have smuggled in scope; naming the blocker is the fix.

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/src/pdf/cos.rs` | `parse_hex_color` `#rgb` support + unit tests. |
| `src-tauri/src/pdf/mod.rs` | Alphabetized the module list. |
| `src/app/{Watermark,Background}Dialog.tsx` | `HistoryState` type; direct `parsePageRange` import. |
| `src/tools/watermark/watermark.ts` | Dropped the `parsePageRange` re-export shim. |
| `src/tools/_framework/{coords,index}.ts` | New home for the screen-rect helpers. |
| `src/tools/free-text/free-text.ts` + 4 importers | Removed the helpers; repointed imports. |

#### Further reading

- CSS `#rgb` / `#rrggbb` hex color notation (MDN).
- Module boundaries / "features vs. framework" layering.

---

## P4.HF18 — CID-path unification, phase 1 (header/footer)

#### Problem

Embedded (non-WinAnsi) text had **two** backends: page-content writers
(header/footer, watermark, text box) went through PDFium text *objects*
(`font_embed::embed_runs`), while annotation `/AP` writers used a hand-built
Type0/CIDFontType2 in lopdf (`build_cid_font`). The PDFium page path was worse:
it estimated width with a flat average (§3.10 drift), carried no HF2
marked-content tag (so an embedded footer wasn't splice-removable), and paid a
PDFium round-trip. This phase retires the first writer (header/footer) onto the
CID path.

#### Concepts learned

- **Two mechanisms doing the same job is the smell; converge them.** The `/AP`
  CID path already subsets, exposes exact advances (`cid.width`), and is pure
  lopdf. The page writers already had all the page machinery (`cm` matrices,
  ExtGState opacity, prepend-for-behind, underline paths, `wrap_decoration`).
  The PDFium path was a *parallel* mechanism bypassing all of it. Unifying =
  emit `<hex> Tj` CID content through the page machinery the base-14 path uses.
- **A shared primitive with a data-carrier struct.** `place_cid_run(doc, page,
  font_name, cid, &CidRun{…})` emits one marked-content-wrapped run
  (opacity/matrix/behind/underline). `CidRun` is the page-content analogue of
  the retired `EmbedRun` — phases 2–3 reuse it unchanged.
- **The ordering subtlety of exact metrics.** Alignment used to precompute the
  placement matrix with a font-average *before* the font existed. Exact metrics
  need `cid.width`, which needs the font — so the writer reorders to **build the
  CID font, then compute L/C/R offsets, then place**. A two-pass loop (collect
  segments + all text → build subset → place) falls out of that.
- **Register the shared font once per page.** `build_cid_font` adds one font
  object graph; each page's `/Resources /Font` references it. A
  `HashMap<page, name>` avoids re-registering per segment.
- **Reuse existing proofs.** The HF9 spike already proved CID page content
  renders + extracts; HF2 already proved marked-content splice-removal
  (font-agnostic). So no new make-or-break spike — the migration's own test
  asserts the composition (BDC `/VibePDF` tag + hex `Tj` + `cm`), and the
  existing render+extract test (whose stale `base` arg this caught and fixed)
  covers rendering.

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/src/pdf/font_embed_cid.rs` | New `CidRun` + `place_cid_run` (CID page-content emitter). |
| `src-tauri/src/pdf/header_footer.rs` | Embedded path now builds a CID font + `place_cid_run` per segment (exact width, HF2 tag); dropped the `embed_runs`/`font_avg_em` estimate + the `base` arg. New inline tag test; fixed the render test's signature. |

#### Further reading

- PDF 32000-1:2008 §14.6 (marked content) and §9.7 (composite/Type0 fonts).
- Command/strategy convergence (removing a redundant second implementation).

---

## P4.HF19 — CID-path unification, phase 2 (watermark)

#### Problem

Same as phase 1, for the second page-content writer: the non-WinAnsi watermark
went through PDFium text objects (flat-average centring, no HF2 tag, PDFium
round-trip). Migrate it onto `place_cid_run`.

#### Concepts learned

- **A well-shaped primitive makes the second migration mechanical.** Phase 1's
  `place_cid_run`/`CidRun` already carried opacity, matrix, behind, and the
  marked-content tag — exactly the watermark's feature set. So phase 2 was
  almost entirely *deleting* the PDFium-specific code: build one shared CID
  subset, centre with exact `cid.width`, and hand each page's baked
  `compose(compose(t, r), vt)` matrix to `place_cid_run`.
- **Delete inputs that a rewrite makes irrelevant.** The old path took `base`
  (a base-14 font name) to estimate width with `font_avg_em`. The CID path uses
  the covering font's real advances, so `base` — and the `bold`/`italic`/
  `font_family` it needed — became dead. Destructure `{ text, size, color, .. }`
  and drop the `base_font` call; the compiler's unused-variable warning is the
  cue that a parameter has outlived its purpose.
- **One subset, many pages.** `build_cid_font` runs once on the mark's text; each
  page's `/Resources /Font` references the shared dict (one run per page, so no
  per-page dedup map needed unlike header/footer's three segments).

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/src/pdf/watermark.rs` | Embedded path now builds a CID font + `place_cid_run` per page (exact centring, HF2 tag, opacity via ExtGState, behind); dropped `embed_runs`/`font_avg_em`/`base`. New CID+tag+opacity test. |

#### Further reading

- (Same CID/marked-content references as P4.HF18.)

---

## P4.HF20 — CID-path unification, phases 3 + 4 (text box + delete the PDFium path)

#### Problem

The text box was the last page-content writer on the PDFium embed path. Migrating
it (phase 3) leaves `font_embed.rs` with no users at all — so phase 4, deleting
it, is the *direct consequence* rather than a separate effort.

#### Concepts learned

- **When a migration makes a module dead, delete it in the same breath.** After
  the text-box swap, `cargo build` reported `embed_runs`/`EmbedRun`/`subset_font`
  as unused. Leaving them behind an `#[allow(dead_code)]` to defer deletion to a
  "phase 4 ship" would be worse than just removing the file — a dead module
  suppressed is debt, not tidiness. Fold the deletion in.
- **Exact where it's cheap, estimate where it's safe.** The text box's *underline
  rule length* now uses exact `cid.width` (visible if wrong), but the *wrap point*
  still uses the `font_avg_em` estimate — because the `/AP` box clips an over-wide
  line, and `wrap_lines` is shared with the base-14 and `/AP` CID paths (making it
  width-exact is a separate, wider change). Pick your precision battles.
- **A retired abstraction leaves dangling doc-links.** Deleting `font_embed.rs`
  turned two `[crate::pdf::font_embed]` intra-doc links stale; repoint them to the
  survivor. And a module doc that framed "two backends by necessity" is now a lie
  — rewrite it (and docs/04) to describe the single backend, or the next reader
  inherits the wrong mental model.
- **The payoff of convergence.** One backend (`font_embed_cid`) now serves both
  page content (`place_cid_run`) and annotation `/AP`. Every embedded surface gets
  exact metrics, the HF2 splice tag, and no PDFium round-trip — and there's one
  place to fix a font bug, not two.

#### Files in this step

| File | Role |
|---|---|
| `src-tauri/src/pdf/cos.rs` | Text-box embedded path → `build_cid_font` + `place_cid_run` per wrapped line (exact-`cid.width` underline, HF2 tag); dropped the `font_embed` import + a new CID+tag test. |
| `src-tauri/src/pdf/font_embed.rs` | **Deleted** — the retired PDFium page-object embed path. |
| `src-tauri/src/pdf/mod.rs` | Dropped `pub mod font_embed;`. |
| `font_embed_cid.rs`, `cos.rs`, `font_resolver.rs`, `docs/04` | Doc updates for the single backend; repointed stale intra-doc links. |

#### Further reading

- (Same CID / marked-content references as P4.HF18–19.)

---

## P4.HF21 — Text-tool disambiguation (Phase 0 of "re-editable Add Text")

### Problem

An in-app test (2026-07-22) surfaced three complaints about adding text: a large
block is hard to edit, editing works one line at a time, and added text never
appears in the Annotations panel. Diagnosis traced all three to a single UX trap:
there are **two** text tools whose buttons read almost identically — **"Text"**
(the `free-text` tool, which writes a re-editable `/FreeText` *annotation* listed
in the panel) and **"Add Text"** (the `add-text` tool, which bakes text into the
**page content stream**, per spec P4-EDIT-003). A user reaching for "add text"
naturally clicks the second and then hits every sharp edge of page content:
no re-edit handle (only per-run **Edit Text**), and — correctly, by spec — no
panel entry.

This step ships only the cheap, spec-legal half: make the two tools tell
themselves apart. The real fix (edit an Add-Text box *as a unit*) is planned but
blocked on a new spec line, so it isn't in this commit.

### Concepts learned

- **A UI label is part of the contract.** Two features that behave in opposite
  ways (overlay annotation vs. baked-in content) must not share a near-identical
  name; the disambiguation is as real a fix as any code path. Here: `free-text` →
  **"Text Box"**, `add-text` stays **"Add Text"**, with tooltips that state the
  behavioural difference (re-editable + in the panel vs. permanent page content).
- **Spec-mandated ≠ bug.** "Added text isn't in Annotations" felt like a defect
  but P4-EDIT-003 *requires* page-content text to be a non-annotation. The right
  response is to explain and design *within* the constraint (make the page content
  re-editable), not to violate it by turning it into an annotation.
- **A run is one line.** PDF text is one show-text operator ≈ one line, so
  "edit one line at a time" is the *editing model*, not a missing feature —
  widening the single-run editor to a `<textarea>` would corrupt the run, not
  produce two lines. Multi-line editing only makes sense as *edit-the-whole-box*.

### Files in this step

| File | Role |
|---|---|
| `src/app/MarkupToolbar.tsx` | Relabel + retooltip the two text buttons ("Text" → "Text Box"; clarify "Add Text"). UI-only. |
| `BACKLOG.md` | Record the report, the shipped relabel, and the blocked Phase 1–3 design (incl. the `P4-EDIT-007` ID collision). |

### Further reading

- PDF 32000-1:2008 §9.4 (text objects / show operators — why a run is a line).
- (Marked-content tag design that Phase 1–3 will build on: see P4.HF2 / HF18–20.)

---

## P4.HF22 — Re-editable Add Text, Phase 1 (emit metadata + read)

### Problem

Phase 0 renamed the buttons; this phase lays the Rust groundwork so an "Add Text"
box can later be re-opened and edited *as one unit* (SPEC P4-EDIT-003b, newly
added). The box is baked into the page content stream (not an annotation — that's
spec-mandated), so there's no annotation dict to read style out of. Two problems:
(1) the text is drawn as glyph codes (Identity-H hex for Unicode), so you can't
cheaply recover the *source* text + style from the drawing ops; and (2) a
multi-line box was emitted as one marked-content tag **per line**, each with its
own `/Id`, so there was no single handle for "the box".

### Concepts learned

- **Marked content as a metadata side-channel.** PDF marked content
  (`/Tag << …props… >> BDC … EMC`, §14.6) is inert to renderers but carries an
  arbitrary property dict. We already used it (HF2) as an identity rail; now the
  dict also **stores the box's source** — `/Text /Font /Size /Color /Bold /Italic
  /Underline /Rect`. Re-reading a box becomes plain dict lookup (no glyph decode).
  This is the content-stream analogue of an annotation's `/NM` + fields.
- **Hex strings dodge text-encoding pain.** A PDF *literal* string `(…)` is a
  byte string; our `escape_pdf_string` transcodes to WinAnsi and turns anything
  outside it into `?` — fine for drawing WinAnsi, fatal for *storing* Coptic. A
  **hex string** `<48656C6C6F>` needs no escaping and round-trips arbitrary bytes,
  so `/Text` holds hex-encoded **UTF-8** and any Unicode + newlines survive intact.
- **One tag per box, not per line.** Wrapping *all* the line fragments in a single
  `BDC…EMC` gives the box one `/Id` — the future delete/re-edit key. This drove a
  small refactor: `place_cid_run` (one tag per call, for header/footer + watermark)
  now delegates to `cid_run_fragment` (just the `q…Q` drawing), and the text box
  concatenates many fragments under one tag.
- **lopdf `Content` is a real parser.** `get_and_decode_page_content` returns typed
  operators with typed operands — a `BDC` op's second operand is a parsed
  `Object::Dictionary`. So the reader is a straight walk: find `BDC` with
  `/VibePDF` + `/Kind (text-box)`, pull the metadata. No hand-rolled tokenizer.
- **Skip-don't-fail on foreign content.** A box lacking `/Text` (older per-line
  tags, or non-VibePDF content) is silently skipped by `read_text_boxes`, so it
  falls back to per-run editing (P4-EDIT-001) rather than crashing or lying.

### Files in this step

| File | Role |
|---|---|
| `docs/02_PRODUCT_SPEC.md` | New spec line **P4-EDIT-003b** (re-edit an added text box). |
| `src-tauri/src/pdf/cos.rs` | `text_box_tag_body` + `wrap_text_box`; both add-text paths now wrap the whole box in one metadata tag; new `TextBoxInfo` + `read_text_boxes` + dict extractors; round-trip tests. |
| `src-tauri/src/pdf/font_embed_cid.rs` | Split `place_cid_run` → `cid_run_fragment` (untagged drawing) + thin wrapper, so many lines share one tag. |
| `src-tauri/tests/text_box.rs` | Added an ASCII multi-line re-edit verification artifact alongside the Unicode one. |

### Further reading

- PDF 32000-1:2008 §14.6 (marked content), §7.3.4 (string objects: literal vs hex).
- `lopdf::content::Content` — decoded content-stream operators.

---

## P4.HF23 — Re-editable Add Text, Phase 2 (splice + update + actor + IPC)

### Problem

Phase 1 gave every Add-Text box a metadata tag and a reader. Phase 2 makes the box
actually *changeable*: delete the whole box by its `/Id`, and re-edit it in place
(new text/style, same rectangle) as one undoable operation, exposed to the
frontend through the document actor.

### Concepts learned

- **Splice a marked-content block by walking nesting depth.** To remove one box we
  find its opening `BDC` (the operand dict whose `/Id` matches), then scan forward
  counting `BDC`/`BMC` as +1 and `EMC` as −1; the `EMC` that brings depth back to 0
  is the block's close. Drop `start..=end`, re-encode with `Content::encode`, and
  `change_page_content` — the exact machinery `delete_text_run` uses. Depth-tracking
  (not "the next EMC") is what makes it correct if content is ever nested.
- **Update = read-rect + remove + re-add.** Rather than mutate operators in place,
  `update_text_box` reads the box's stored `/Rect`, splices the old block out, and
  calls `add_text_box` at that same rect. This reuses the whole emit path (so style
  changes, font switches, WinAnsi↔CID crossover all just work) and guarantees
  "position preserved" (P4-EDIT-003b) at the primitive level. The box gets a fresh
  `/Id`; the frontend re-reads on the edit epoch.
- **One Edit = one undo step, even when it's remove+add underneath.** `UpdateTextBoxEdit`
  is a single `cos_edit` closure, so its inverse is one pre-write byte snapshot
  (`RestoreDocEdit`) — undo restores the original box in one step, proven by the
  actor round-trip test (add→read→update→undo).
- **The actor's two shapes, reused.** A *write* (`Message::UpdateTextBox`) applies an
  `Edit` and records the inverse in history; a page-scoped *read*
  (`Message::ReadTextBoxes`) serializes the live doc under the PDFium lock, then runs
  the lopdf reader on the bytes — identical to how `ReadAnnotations` works. Each gets
  a thin `_request` (non-blocking) + `await`-holding handle method, and a
  `#[tauri::command]` wrapper registered in `lib.rs`. `box_id` (not `id`, which is the
  document id) names the box in the command signature.

### Files in this step

| File | Role |
|---|---|
| `src-tauri/src/pdf/cos.rs` | `remove_text_box` (splice by `/Id`) + `update_text_box` (read-rect → remove → re-add); 3 tests. |
| `src-tauri/src/pdf/annotation.rs` | `UpdateTextBoxEdit` (snapshot-undo Edit). |
| `src-tauri/src/pdf/actor.rs` | `Message::UpdateTextBox` + `ReadTextBoxes`; handle methods; handlers. |
| `src-tauri/src/commands/pdf.rs` + `src-tauri/src/lib.rs` | `pdf_update_text_box` + `pdf_read_text_boxes` commands, registered. |
| `src-tauri/tests/text_box.rs` | Actor round-trip integration test (add→read→update→undo). |

### Further reading

- PDF 32000-1:2008 §14.6 (marked-content nesting: BMC/BDC/EMC).
- `lopdf` `Content::encode` / `Document::change_page_content`.

---

## P4.HF24 — Re-editable Add Text, Phase 3 (frontend re-edit layer)

### Problem

Phases 1–2 made the backend able to read + re-edit an added text box. Phase 3 is
the part the user sees: double-click a box on the page, edit the whole block
pre-filled, save. This completes P4-EDIT-003b end-to-end.

### Concepts learned

- **An overlay that's both a creator and an editor.** `text-box-layer.tsx` already
  handled *drawing* a new box (drag → editor → `addTextBox`). Re-edit reuses the
  same transient editor, distinguished by an `editId` on the `Editor` state: `null`
  ⇒ new box (commit → `addTextBox`), a string ⇒ existing box (commit →
  `updateTextBox`). One editor component, two commit paths — no duplicate UI.
- **Read-on-epoch keeps overlays in step with the document.** The layer reads its
  page's boxes with `readTextBoxes` keyed on the edit epoch (`useDocEpoch`), the
  same pattern the annotation panel + free-text layer use: any add/update/delete/
  undo bumps the epoch → the boxes re-read → the double-click hit-zones track
  reality. The layer used to render nothing when idle; now it renders a
  pointer-events-`none` pass-through div whose per-box hit-zones opt back in, so
  scrolling between boxes still works but double-click lands.
- **Arm the tool on re-edit to surface its controls.** The font/size/B-I-U controls
  live in the toolbar and only show while the Add Text tool is active. So a
  double-click both opens the editor *and* `setActiveTool("add-text")` +
  `setOptions(box's style)` — the controls appear pre-filled, giving re-edit full
  parity with creation. `cancel()` clears the tool again.
- **Changing a component's idle contract means updating its test.** The old test
  asserted "renders nothing when inactive"; that's now intentionally false (idle
  renders the re-edit layer). Rewriting it to assert the *new* contract (idle + no
  boxes ⇒ a pass-through layer with no editor/hit-zones) is a correctness update,
  not weakening — the new re-edit test covers idle + boxes.
- **`box_id` vs `id` across the boundary.** The document id is `id`; the box's
  marked-content id is `boxId` in the JS call (Tauri maps it to the Rust `box_id`
  param, the same camelCase→snake_case mapping `fontFamily`→`font_family` uses).

### Files in this step

| File | Role |
|---|---|
| `src/ipc/text-box.ts` | `TextBoxInfo` + `readTextBoxes` + `updateTextBox` typed IPC wrappers. |
| `src/view/text-box-layer.tsx` | Read boxes on epoch; idle double-click hit-zones; re-edit opens the editor pre-filled + armed; commit branches add vs update. |
| `src/view/__tests__/text-box-layer.test.tsx` | Updated idle-contract test + a re-edit test (double-click → pre-filled → `updateTextBox`). |

### Further reading

- Tauri v2 command args (camelCase JS keys ↔ snake_case Rust params).

---

## P4.HF25 Step 1 — Delete-a-text-box primitive + command

### Problem

Testing surfaced two needs that both require *deleting* an added text box: clearing
its text + Save should remove it (item 1), and a text-box list wants a delete
action (item 4). The `remove_text_box` splice already existed (P4.HF23) but was
only reachable *inside* `update_text_box`; nothing exposed a standalone delete.

### Concepts learned

- **Expose an existing primitive as its own undoable edit.** No new PDF logic —
  `RemoveTextBoxEdit` is a three-line `Edit` that wraps `remove_text_box` in the
  same `cos_edit` snapshot pattern the other text-box edits use, so delete is one
  undo step and its inverse restores the pre-delete bytes. The actor message +
  Tauri command + IPC wrapper are pure plumbing mirrored from `pdf_update_text_box`.
- **Build shared infrastructure once.** Both item 1 (empty→delete) and item 4
  (list→delete) consume the same `deleteTextBox` path, so it's Step 1 of the batch
  rather than duplicated later.

### Files in this step

| File | Role |
|---|---|
| `src-tauri/src/pdf/annotation.rs` | `RemoveTextBoxEdit` (snapshot-undo wrapper over `remove_text_box`). |
| `src-tauri/src/pdf/actor.rs` | `Message::RemoveTextBox` + `delete_text_box` handle method + handler. |
| `src-tauri/src/commands/pdf.rs` + `src-tauri/src/lib.rs` | `pdf_delete_text_box` command, registered. |
| `src/ipc/text-box.ts` | `deleteTextBox` typed wrapper. |
| `src-tauri/tests/text_box.rs` | Actor delete round-trip (add→delete→undo). |

---

## P4.HF25 Step 2 — Empty-edit deletes + re-edit unified under Edit Text

### Problem

Two testing bugs: (1) clearing a text run's text and hitting Save *reverted* to the
original instead of removing it; (2) the Add-Text re-edit (a double-click gesture)
fought the Edit Text tool's per-run click zones over the same page-content text, so
double-click "did nothing" while single-clicks opened the per-run editor.

### Concepts learned

- **`set_text("")` is not a delete.** Replacing a run's text with an empty string
  doesn't remove the run — PDFium keeps the object and the reload re-renders the
  original. The right move when the field is emptied is to route to the *delete*
  path (`deleteTextRun` / `deleteTextBox`), which is what the explicit Delete button
  already does. Empty commit ⇒ delete, in both editors.
- **Overlapping hit-layers need one owner per gesture.** Added boxes are page
  content, so both the Edit Text tool (per-run zones) and the box re-edit (whole-box
  zones) targeted them. Two mechanisms over the same pixels is inherently confusing.
  The fix is to pick one: re-edit now lives *inside* the Edit Text tool — a
  single-click box zone that, because `TextBoxLayer` mounts after `TextEditLayer`,
  sits **above** the per-run zones. So a click on added text opens the whole-box
  editor and a click on foreign text falls through to per-run. One tool, one rule.
- **Stacking order is API.** "This layer mounts last, so its zones win" is a real
  contract — worth stating in a comment, because reordering the JSX would silently
  break which tool handles a click.

### Files in this step

| File | Role |
|---|---|
| `src/view/text-edit-layer.tsx` | Empty commit → `deleteTextRun` instead of `replaceTextRun("")`. |
| `src/view/text-box-layer.tsx` | Re-edit triggers on an Edit-Text single-click (was idle double-click); empty re-edit → `deleteTextBox`. |
| `src/view/__tests__/text-edit-layer.test.tsx` | + clear→delete test. |
| `src/view/__tests__/text-box-layer.test.tsx` | Re-edit test → Edit-Text click; + clear→delete test; idle-contract test updated. |

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
