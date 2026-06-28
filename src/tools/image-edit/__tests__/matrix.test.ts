// SPEC: P4-EDIT-006 (P4.C2) — the affine-matrix helpers for image edits.

import { describe, expect, it } from "vitest";

import { center, type Matrix, mul, rectToMatrix, rotate90, translate } from "@/tools/image-edit/matrix";

const IDENTITY: Matrix = [1, 0, 0, 1, 0, 0];

describe("matrix helpers", () => {
  it("mul with identity is a no-op", () => {
    const m: Matrix = [2, 0, 0, 3, 10, 20];
    expect(mul(IDENTITY, m)).toEqual(m);
    expect(mul(m, IDENTITY)).toEqual(m);
  });

  it("rectToMatrix maps the unit square onto the rect", () => {
    expect(rectToMatrix(10, 20, 110, 80)).toEqual([100, 0, 0, 60, 10, 20]);
  });

  it("translate shifts only e/f", () => {
    expect(translate([5, 0, 0, 7, 1, 2], 10, -3)).toEqual([5, 0, 0, 7, 11, -1]);
  });

  it("rotate90 preserves the centre and swaps to off-diagonal", () => {
    const m: Matrix = [100, 0, 0, 100, 0, 0]; // unit image at [0,0,100,100], centre (50,50)
    const r = rotate90(m);
    const [cx, cy] = center(r);
    expect(cx).toBeCloseTo(50);
    expect(cy).toBeCloseTo(50);
    // 90° rotation → a,d ≈ 0; b,c carry the ±100.
    expect(r[0]).toBeCloseTo(0);
    expect(r[3]).toBeCloseTo(0);
    expect(Math.abs(r[1])).toBeCloseTo(100);
    expect(Math.abs(r[2])).toBeCloseTo(100);
  });
});
