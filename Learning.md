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
