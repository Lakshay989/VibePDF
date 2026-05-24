import { describe, expect, it } from "vitest";

import { invertImageDataForDark } from "@/view/dark-invert";

function pixel(r: number, g: number, b: number, a = 255): Uint8ClampedArray {
  return new Uint8ClampedArray([r, g, b, a]);
}

function read(d: Uint8ClampedArray): [number, number, number, number] {
  return [d[0], d[1], d[2], d[3]];
}

describe("invertImageDataForDark", () => {
  it("inverts pure white → pure black", () => {
    const d = pixel(255, 255, 255);
    invertImageDataForDark(d);
    expect(read(d)).toEqual([0, 0, 0, 255]);
  });

  it("inverts pure black → pure white", () => {
    const d = pixel(0, 0, 0);
    invertImageDataForDark(d);
    expect(read(d)).toEqual([255, 255, 255, 255]);
  });

  it("inverts near-gray pixels", () => {
    const d = pixel(200, 200, 200);
    invertImageDataForDark(d);
    expect(read(d)).toEqual([55, 55, 55, 255]);
  });

  it("inverts slightly-tinted near-gray (low saturation)", () => {
    // saturation = (205-200)/205 ≈ 0.024, below 0.15 threshold
    const d = pixel(200, 205, 200);
    invertImageDataForDark(d);
    expect(read(d)).toEqual([55, 50, 55, 255]);
  });

  it("leaves saturated colors alone (skin-tone-ish)", () => {
    // saturation = (200-120)/200 = 0.4, above threshold
    const d = pixel(200, 150, 120);
    invertImageDataForDark(d);
    expect(read(d)).toEqual([200, 150, 120, 255]);
  });

  it("leaves pure red alone", () => {
    const d = pixel(255, 0, 0);
    invertImageDataForDark(d);
    expect(read(d)).toEqual([255, 0, 0, 255]);
  });

  it("never touches alpha", () => {
    const d = pixel(255, 255, 255, 128);
    invertImageDataForDark(d);
    expect(d[3]).toBe(128);
  });

  it("operates over a multi-pixel buffer in place", () => {
    // 3 pixels: black, red, white
    const d = new Uint8ClampedArray([
      0, 0, 0, 255, 255, 0, 0, 255, 255, 255, 255, 255,
    ]);
    invertImageDataForDark(d);
    expect(Array.from(d)).toEqual([
      255, 255, 255, 255, // black → white
      255, 0, 0, 255, // red unchanged
      0, 0, 0, 255, // white → black
    ]);
  });
});
