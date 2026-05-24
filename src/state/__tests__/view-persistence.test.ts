import "fake-indexeddb/auto";
import { beforeEach, describe, expect, it } from "vitest";

import {
  _resetForTests,
  loadViewSettings,
  pathHash,
  saveViewSettings,
} from "@/state/view-persistence";

beforeEach(() => {
  _resetForTests();
  // fake-indexeddb persists between tests; recreate a clean DB by
  // wiping its registry. The simplest portable way is to delete the
  // database before each test.
  return new Promise<void>((resolve, reject) => {
    const req = indexedDB.deleteDatabase("vibepdf");
    req.onsuccess = () => resolve();
    req.onerror = () => reject(req.error ?? new Error("deleteDatabase"));
    req.onblocked = () => resolve();
  });
});

describe("pathHash", () => {
  it("produces 64-char hex SHA-256", async () => {
    const h = await pathHash("/Users/me/Documents/report.pdf");
    expect(h).toMatch(/^[0-9a-f]{64}$/);
  });

  it("is stable for the same input", async () => {
    const a = await pathHash("/x.pdf");
    const b = await pathHash("/x.pdf");
    expect(a).toBe(b);
  });

  it("differs between different paths", async () => {
    const a = await pathHash("/a.pdf");
    const b = await pathHash("/b.pdf");
    expect(a).not.toBe(b);
  });
});

describe("view-settings IDB round-trip", () => {
  it("returns null for an unknown hash", async () => {
    const r = await loadViewSettings("deadbeef".repeat(8));
    expect(r).toBeNull();
  });

  it("round-trips zoom + fit-mode", async () => {
    const hash = await pathHash("/A.pdf");
    await saveViewSettings(hash, { zoom: 1.75, fitMode: null });
    const r = await loadViewSettings(hash);
    expect(r).toEqual({ zoom: 1.75, fitMode: null });
  });

  it("overwrites on second save", async () => {
    const hash = await pathHash("/B.pdf");
    await saveViewSettings(hash, { zoom: 1.0, fitMode: "fit-width" });
    await saveViewSettings(hash, { zoom: 2.0, fitMode: null });
    const r = await loadViewSettings(hash);
    expect(r).toEqual({ zoom: 2.0, fitMode: null });
  });

  it("keys are independent per document", async () => {
    const a = await pathHash("/A.pdf");
    const b = await pathHash("/B.pdf");
    await saveViewSettings(a, { zoom: 1.5, fitMode: null });
    await saveViewSettings(b, { zoom: 0.5, fitMode: "fit-page" });
    expect(await loadViewSettings(a)).toEqual({ zoom: 1.5, fitMode: null });
    expect(await loadViewSettings(b)).toEqual({
      zoom: 0.5,
      fitMode: "fit-page",
    });
  });
});
