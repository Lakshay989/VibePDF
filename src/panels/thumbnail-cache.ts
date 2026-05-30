// SPEC: P1-VIEW-008 (P1.D1) — IndexedDB cache for page thumbnails.
//
// Thumbnails are PNG bytes produced by the backend (PDFium via
// `pdf_render_page`). We cache them keyed by `(documentId, page, dpr)`
// so re-opening a document — or just scrolling back up the sidebar —
// is an instant IDB hit rather than a fresh render round-trip.
//
// Mirrors the single-store + `dbPromise` + `_resetForTests` shape of
// `src/state/view-persistence.ts` (C2) so tests can swap in
// `fake-indexeddb` the same way.

const DB_NAME = "vibepdf-thumbnails";
const STORE = "thumbnails";
const VERSION = 1;

export interface ThumbKey {
  documentId: string;
  page: number;
  /** Device pixel ratio the thumbnail was rendered at (so a retina cache entry isn't served blurry, or vice-versa). */
  dpr: number;
}

/** Stable string key for the object store. */
function keyOf({ documentId, page, dpr }: ThumbKey): string {
  return `${documentId}:${page}:${dpr}`;
}

let dbPromise: Promise<IDBDatabase> | null = null;

function openDb(): Promise<IDBDatabase> {
  if (dbPromise) return dbPromise;
  dbPromise = new Promise((resolve, reject) => {
    const req = indexedDB.open(DB_NAME, VERSION);
    req.onupgradeneeded = () => {
      const db = req.result;
      if (!db.objectStoreNames.contains(STORE)) db.createObjectStore(STORE);
    };
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error ?? new Error("indexedDB open failed"));
  });
  return dbPromise;
}

/** Returns the cached PNG bytes for `key`, or `null` on a miss. */
export async function getThumb(key: ThumbKey): Promise<Uint8Array | null> {
  const db = await openDb();
  return new Promise((resolve, reject) => {
    const tx = db.transaction(STORE, "readonly");
    const req = tx.objectStore(STORE).get(keyOf(key));
    req.onsuccess = () => {
      const v = req.result as Uint8Array | undefined;
      resolve(v ?? null);
    };
    req.onerror = () => reject(req.error ?? new Error("thumb get failed"));
  });
}

/** Stores `png` under `key`, overwriting any prior entry. */
export async function putThumb(key: ThumbKey, png: Uint8Array): Promise<void> {
  const db = await openDb();
  return new Promise((resolve, reject) => {
    const tx = db.transaction(STORE, "readwrite");
    tx.objectStore(STORE).put(png, keyOf(key));
    tx.oncomplete = () => resolve();
    tx.onerror = () => reject(tx.error ?? new Error("thumb put failed"));
  });
}

/** Test-only: drop the cached connection so a swapped-in `indexedDB`
 *  global (fake-indexeddb) is picked up on the next open. */
export function _resetForTests(): void {
  dbPromise = null;
}
