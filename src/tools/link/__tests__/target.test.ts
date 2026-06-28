import { describe, expect, it } from "vitest";

import { type LinkTarget, toWireValue, validateTarget } from "@/tools/link/target";

const PAGE_COUNT = 10;
const v = (target: LinkTarget) => validateTarget(target, PAGE_COUNT);

describe("validateTarget", () => {
  it("rejects an empty value for every kind", () => {
    for (const kind of ["url", "email", "page", "named"] as const) {
      expect(v({ kind, value: "   " }).ok).toBe(false);
    }
  });

  it("requires a scheme on a URL", () => {
    expect(v({ kind: "url", value: "https://example.com" }).ok).toBe(true);
    expect(v({ kind: "url", value: "ftp://host/x" }).ok).toBe(true);
    expect(v({ kind: "url", value: "example.com" }).ok).toBe(false);
  });

  it("validates an email address shape", () => {
    expect(v({ kind: "email", value: "ada@example.com" }).ok).toBe(true);
    expect(v({ kind: "email", value: "not-an-email" }).ok).toBe(false);
    expect(v({ kind: "email", value: "a@b" }).ok).toBe(false);
  });

  it("bounds a page target to 1..pageCount", () => {
    expect(v({ kind: "page", value: "1" }).ok).toBe(true);
    expect(v({ kind: "page", value: "10" }).ok).toBe(true);
    expect(v({ kind: "page", value: "0" }).ok).toBe(false);
    expect(v({ kind: "page", value: "11" }).ok).toBe(false);
    expect(v({ kind: "page", value: "2.5" }).ok).toBe(false);
    expect(v({ kind: "page", value: "abc" }).ok).toBe(false);
  });

  it("accepts any non-empty named destination", () => {
    expect(v({ kind: "named", value: "chapter-2" }).ok).toBe(true);
  });
});

describe("toWireValue", () => {
  it("converts a 1-based page to a 0-based index", () => {
    expect(toWireValue({ kind: "page", value: "1" })).toBe("0");
    expect(toWireValue({ kind: "page", value: "3" })).toBe("2");
  });

  it("passes url / email / named through verbatim (mailto added Rust-side)", () => {
    expect(toWireValue({ kind: "url", value: " https://x.com " })).toBe("https://x.com");
    expect(toWireValue({ kind: "email", value: "ada@example.com" })).toBe("ada@example.com");
    expect(toWireValue({ kind: "named", value: "dest" })).toBe("dest");
  });
});
