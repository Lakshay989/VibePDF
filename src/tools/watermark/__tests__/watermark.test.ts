import { describe, expect, it } from "vitest";

import { DEFAULT_WATERMARK } from "@/tools/watermark/watermark";

// parsePageRange's cases live in src/tools/__tests__/page-range.test.ts (shared).

describe("DEFAULT_WATERMARK", () => {
  it("is a faint DRAFT behind content at 45°", () => {
    expect(DEFAULT_WATERMARK.text).toBe("DRAFT");
    expect(DEFAULT_WATERMARK.behind).toBe(true);
    expect(DEFAULT_WATERMARK.rotation).toBe(45);
    expect(DEFAULT_WATERMARK.opacity).toBeLessThan(1);
  });
});
