import { describe, expect, it } from "vitest";

import { DEFAULT_WATERMARK, parsePageRange } from "@/tools/watermark/watermark";

const ok = (r: ReturnType<typeof parsePageRange>) => {
  if ("error" in r) throw new Error(`expected pages, got error: ${r.error}`);
  return r.pages;
};

describe("parsePageRange", () => {
  it("'all' / empty expands to every 0-based page", () => {
    expect(ok(parsePageRange("all", 3))).toEqual([0, 1, 2]);
    expect(ok(parsePageRange("", 3))).toEqual([0, 1, 2]);
    expect(ok(parsePageRange("  ALL ", 2))).toEqual([0, 1]);
  });

  it("parses singles and ranges to 0-based, sorted + de-duplicated", () => {
    expect(ok(parsePageRange("1-3, 5", 10))).toEqual([0, 1, 2, 4]);
    expect(ok(parsePageRange("5, 1, 5, 2", 10))).toEqual([0, 1, 4]);
    expect(ok(parsePageRange("8-10", 10))).toEqual([7, 8, 9]);
  });

  it("rejects out-of-range, reversed, and malformed input", () => {
    expect("error" in parsePageRange("0", 5)).toBe(true);
    expect("error" in parsePageRange("6", 5)).toBe(true);
    expect("error" in parsePageRange("3-1", 5)).toBe(true);
    expect("error" in parsePageRange("1-99", 5)).toBe(true);
    expect("error" in parsePageRange("a, b", 5)).toBe(true);
  });
});

describe("DEFAULT_WATERMARK", () => {
  it("is a faint DRAFT behind content at 45°", () => {
    expect(DEFAULT_WATERMARK.text).toBe("DRAFT");
    expect(DEFAULT_WATERMARK.behind).toBe(true);
    expect(DEFAULT_WATERMARK.rotation).toBe(45);
    expect(DEFAULT_WATERMARK.opacity).toBeLessThan(1);
  });
});
