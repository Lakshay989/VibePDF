// SPEC: P4-EDIT-009 (P4.D2) — pure helpers for the watermark dialog: the spec
// shape and a default. The page-range parser lives in the shared
// `@/tools/page-range` (used by every page-decoration dialog) and is re-exported
// here for back-compat.

export { parsePageRange, type PageRangeResult } from "@/tools/page-range";

/** A watermark's appearance + placement, independent of text-vs-image source. */
export interface WatermarkCommon {
  /** 0-based page indices to stamp. */
  pages: number[];
  /** 0..1. */
  opacity: number;
  /** Degrees, counter-clockwise. */
  rotation: number;
  /** Draw under existing content (vs. on top). */
  behind: boolean;
}

export type WatermarkSpec =
  | (WatermarkCommon & {
      kind: "text";
      text: string;
      fontFamily: string;
      fontSize: number;
      color: string;
    })
  | (WatermarkCommon & { kind: "image"; imagePath: string });

/** Sensible defaults for a fresh watermark: a faint grey "DRAFT" at 45°. */
export const DEFAULT_WATERMARK = {
  text: "DRAFT",
  fontFamily: "Helvetica",
  fontSize: 72,
  color: "#808080",
  opacity: 0.3,
  rotation: 45,
  behind: true,
} as const;
