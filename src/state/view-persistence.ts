// SPEC: P1-VIEW-006 (P1.C2) — per-document view settings.
//
// Each document's last zoom / fit-mode is persisted under the SHA-256
// of its filesystem path, in IndexedDB. We use a single-store DB and
// a tiny promise wrapper so tests can swap in fake-indexeddb without
// touching call sites.

import type { FitMode } from "@/state/view-store";

export interface ViewSettings {
  zoom: number;
  fitMode: FitMode | null;
}

const DB_NAME = "vibepdf";
const STORE = "view-settings";
const VERSION = 1;

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

/** Hex SHA-256 of `path`. Used as the IDB key. */
export async function pathHash(path: string): Promise<string> {
  const bytes = new TextEncoder().encode(path);
  const digest = await crypto.subtle.digest("SHA-256", bytes);
  return Array.from(new Uint8Array(digest))
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
}

export async function loadViewSettings(
  hash: string,
): Promise<ViewSettings | null> {
  const db = await openDb();
  return new Promise((resolve, reject) => {
    const tx = db.transaction(STORE, "readonly");
    const req = tx.objectStore(STORE).get(hash);
    req.onsuccess = () => {
      const v = req.result as ViewSettings | undefined;
      resolve(v ?? null);
    };
    req.onerror = () => reject(req.error ?? new Error("get failed"));
  });
}

export async function saveViewSettings(
  hash: string,
  settings: ViewSettings,
): Promise<void> {
  const db = await openDb();
  return new Promise((resolve, reject) => {
    const tx = db.transaction(STORE, "readwrite");
    tx.objectStore(STORE).put(settings, hash);
    tx.oncomplete = () => resolve();
    tx.onerror = () => reject(tx.error ?? new Error("put failed"));
  });
}

/** Test-only helper. Discards the cached connection so a fresh open
 *  picks up a swapped-in `indexedDB` global (e.g. fake-indexeddb). */
export function _resetForTests(): void {
  dbPromise = null;
}
