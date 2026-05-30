import "fake-indexeddb/auto";
import { beforeEach, describe, expect, it } from "vitest";

import {
  _resetForTests,
  getThumb,
  putThumb,
  type ThumbKey,
} from "@/panels/thumbnail-cache";

beforeEach(() => {
  _resetForTests();
  // fake-indexeddb persists between tests; delete the DB for a clean
  // slate (same approach as view-persistence.test.ts).
  return new Promise<void>((resolve, reject) => {
    const req = indexedDB.deleteDatabase("vibepdf-thumbnails");
    req.onsuccess = () => resolve();
    req.onerror = () => reject(req.error ?? new Error("deleteDatabase"));
    req.onblocked = () => resolve();
  });
});

const key = (over: Partial<ThumbKey> = {}): ThumbKey => ({
  documentId: "doc-a",
  page: 0,
  dpr: 1,
  ...over,
});

describe("thumbnail-cache IDB", () => {
  it("returns null on a cache miss", async () => {
    expect(await getThumb(key())).toBeNull();
  });

  it("round-trips PNG bytes for a key", async () => {
    const png = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 1, 2, 3]);
    await putThumb(key(), png);
    const got = await getThumb(key());
    expect(got).not.toBeNull();
    expect(Array.from(got as Uint8Array)).toEqual(Array.from(png));
  });

  it("keys independently by document, page, and dpr", async () => {
    await putThumb(key({ page: 0, dpr: 1 }), new Uint8Array([1]));
    await putThumb(key({ page: 1, dpr: 1 }), new Uint8Array([2]));
    await putThumb(key({ page: 0, dpr: 2 }), new Uint8Array([3]));
    await putThumb(key({ documentId: "doc-b", page: 0, dpr: 1 }), new Uint8Array([4]));

    expect(Array.from((await getThumb(key({ page: 0, dpr: 1 }))) as Uint8Array)).toEqual([1]);
    expect(Array.from((await getThumb(key({ page: 1, dpr: 1 }))) as Uint8Array)).toEqual([2]);
    expect(Array.from((await getThumb(key({ page: 0, dpr: 2 }))) as Uint8Array)).toEqual([3]);
    expect(
      Array.from((await getThumb(key({ documentId: "doc-b", page: 0, dpr: 1 }))) as Uint8Array),
    ).toEqual([4]);
    // A neighbouring key that was never written stays a miss.
    expect(await getThumb(key({ documentId: "doc-b", page: 1, dpr: 1 }))).toBeNull();
  });

  it("overwrites on a second put", async () => {
    await putThumb(key(), new Uint8Array([1, 1, 1]));
    await putThumb(key(), new Uint8Array([9, 9]));
    expect(Array.from((await getThumb(key())) as Uint8Array)).toEqual([9, 9]);
  });
});
