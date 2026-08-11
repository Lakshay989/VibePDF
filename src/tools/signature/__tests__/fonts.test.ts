// SPEC: P6-SEC-002 (P6.A3) — which handwriting fonts this machine can render.
//
// The bug being guarded against is silent: asking a canvas for a font that is
// not installed does not error, it falls back. A signature would come out in
// Helvetica having been offered as "Zapfino". So availability is decided by
// measurement, and these tests drive that measurement with a stub — no canvas
// needed, and no dependence on what fonts happen to exist on the test machine.

import { describe, expect, it } from "vitest";

import {
  availableFonts,
  CANDIDATES,
  GENERIC_FALLBACK,
  isFontAvailable,
  type FontCandidate,
  type MeasureFn,
} from "@/tools/signature/fonts";

/**
 * A stub measurer. Families in `installed` measure differently from the
 * generics; everything else returns exactly the sentinel width, which is what a
 * real fallback looks like.
 */
const measurer = (installed: readonly string[]): MeasureFn => {
  return (text, font) => {
    const base = text.length * 10;
    const family = /"([^"]+)"/.exec(font)?.[1];
    if (family && installed.includes(family)) return base + 7; // distinct advance
    return base; // fell through to the sentinel
  };
};

describe("isFontAvailable", () => {
  it("reports a family present when it changes the measurement", () => {
    expect(isFontAvailable("Zapfino", "Ada", measurer(["Zapfino"]))).toBe(true);
  });

  it("reports a family missing when it measures exactly like the fallback", () => {
    // This is the whole point: the canvas answered, it just answered with the
    // generic. Identical width is the tell.
    expect(isFontAvailable("Zapfino", "Ada", measurer([]))).toBe(false);
  });

  it("is false for empty text rather than guessing", () => {
    expect(isFontAvailable("Zapfino", "", measurer(["Zapfino"]))).toBe(false);
  });

  it("measures the text it was given, not a fixed sample", () => {
    const seen: string[] = [];
    const spy: MeasureFn = (text, font) => {
      seen.push(text);
      return /"([^"]+)"/.test(font) ? 1 : 2;
    };
    isFontAvailable("Zapfino", "आदित्य", spy);
    // A Latin-only face has no glyphs for this and will fall back per character;
    // probing with a canned "Signature" would have missed that entirely.
    expect(seen.every((t) => t === "आदित्य")).toBe(true);
  });
});

describe("availableFonts", () => {
  it("returns only the families this machine can render", () => {
    const installed = ["Snell Roundhand", "Bradley Hand"];
    const found = availableFonts("Ada", measurer(installed));
    expect(found.map((f) => f.family)).toEqual(installed);
  });

  it("falls back to the generic script family when nothing is installed", () => {
    const found = availableFonts("Ada", measurer([]));
    // Never an empty picker, and never a silent substitution — the label says
    // it is the default.
    expect(found).toEqual([GENERIC_FALLBACK]);
    expect(GENERIC_FALLBACK.label.toLowerCase()).toContain("default");
  });

  it("keeps candidate order so the picker is stable between runs", () => {
    const installed = ["Segoe Script", "Snell Roundhand"];
    const found = availableFonts("Ada", measurer(installed));
    const order = CANDIDATES.filter((c) => installed.includes(c.family)).map((c) => c.family);
    expect(found.map((f) => f.family)).toEqual(order);
  });

  it("accepts an explicit candidate list", () => {
    const custom: FontCandidate[] = [{ family: "Only One", label: "Only One" }];
    expect(availableFonts("Ada", measurer(["Only One"]), custom)).toEqual(custom);
  });

  it("ships more than one candidate, as the spec asks for several", () => {
    expect(CANDIDATES.length).toBeGreaterThan(2);
    // No duplicate families — a repeated entry would show twice in the picker.
    expect(new Set(CANDIDATES.map((c) => c.family)).size).toBe(CANDIDATES.length);
  });
});
