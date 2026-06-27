// SPEC: P4-EDIT-001 (P4.B1) — the editor-preview font mapping (cosmetic only).

import { describe, expect, it } from "vitest";

import { cssFamilyForFont } from "@/tools/text-edit/text-edit";

describe("cssFamilyForFont", () => {
  it("buckets serif / monospace / sans-serif by name", () => {
    expect(cssFamilyForFont("TimesNewRomanPSMT")).toContain("serif");
    expect(cssFamilyForFont("Georgia")).toContain("serif");
    expect(cssFamilyForFont("Courier")).toContain("monospace");
    expect(cssFamilyForFont("Consolas")).toContain("monospace");
    expect(cssFamilyForFont("Calibri")).toContain("sans-serif");
    expect(cssFamilyForFont("Helvetica")).toContain("sans-serif");
  });
});
