import { describe, expect, it } from "vitest";

import { LruCache } from "@/view/page-cache";

describe("LruCache", () => {
  it("round-trips set/get", () => {
    const c = new LruCache<number>(3);
    c.set("a", 1);
    expect(c.get("a")).toBe(1);
    expect(c.has("a")).toBe(true);
    expect(c.size).toBe(1);
  });

  it("evicts the least-recently-used entry past capacity", () => {
    const c = new LruCache<number>(2);
    c.set("a", 1);
    c.set("b", 2);
    c.set("c", 3); // evicts "a"
    expect(c.has("a")).toBe(false);
    expect(c.has("b")).toBe(true);
    expect(c.has("c")).toBe(true);
  });

  it("get() promotes a key to most-recent", () => {
    const c = new LruCache<number>(2);
    c.set("a", 1);
    c.set("b", 2);
    expect(c.get("a")).toBe(1); // "a" is now MRU
    c.set("c", 3); // evicts "b", not "a"
    expect(c.has("a")).toBe(true);
    expect(c.has("b")).toBe(false);
    expect(c.has("c")).toBe(true);
  });

  it("set() of an existing key promotes it (no eviction)", () => {
    const c = new LruCache<number>(2);
    c.set("a", 1);
    c.set("b", 2);
    c.set("a", 99); // updates + promotes
    c.set("c", 3); // evicts "b"
    expect(c.get("a")).toBe(99);
    expect(c.has("b")).toBe(false);
    expect(c.has("c")).toBe(true);
  });

  it("clear() empties the cache", () => {
    const c = new LruCache<number>(2);
    c.set("a", 1);
    c.set("b", 2);
    c.clear();
    expect(c.size).toBe(0);
    expect(c.has("a")).toBe(false);
  });

  it("rejects non-positive capacity", () => {
    expect(() => new LruCache<number>(0)).toThrow();
    expect(() => new LruCache<number>(-1)).toThrow();
  });

  it("keys() returns oldest → newest", () => {
    const c = new LruCache<number>(3);
    c.set("a", 1);
    c.set("b", 2);
    c.set("c", 3);
    c.get("a"); // promote a
    expect(c.keys()).toEqual(["b", "c", "a"]);
  });
});
