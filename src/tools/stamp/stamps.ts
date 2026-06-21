// SPEC: P3-ANN-006 (P3.C3a) — the built-in stamp library + placement geometry.
// Pure data/maths so it's testable without React. A stamp is a coloured label
// rendered as a `/Stamp` annotation's `/AP` on the Rust side; here we only carry
// the spec and compute where a click drops the box. Image stamps are C3b.

export interface StampSpec {
  /** A PDF `/Name` token (informational) + the palette id. */
  name: string;
  /** The visible label (also the annotation's `/Contents`). */
  label: string;
  /** Border + text colour, hex. */
  color: string;
}

const RED = "#c0392b";
const GREEN = "#1e8449";
const BLUE = "#1f6feb";
const GRAY = "#555555";

/** The built-in rubber-stamp library. */
export const BUILTIN_STAMPS: readonly StampSpec[] = [
  { name: "Approved", label: "APPROVED", color: GREEN },
  { name: "Reviewed", label: "REVIEWED", color: GREEN },
  { name: "Received", label: "RECEIVED", color: BLUE },
  { name: "Final", label: "FINAL", color: BLUE },
  { name: "Confidential", label: "CONFIDENTIAL", color: RED },
  { name: "NotApproved", label: "NOT APPROVED", color: RED },
  { name: "Void", label: "VOID", color: RED },
  { name: "Draft", label: "DRAFT", color: GRAY },
];

/** Default stamp box size in PDF points. */
export const STAMP_WIDTH = 150;
export const STAMP_HEIGHT = 46;

/** A custom text stamp (default colour red). */
export function customStamp(text: string, color: string = RED): StampSpec {
  return { name: "Custom", label: text, color };
}

/**
 * The placement rect for a stamp centred on `(x, y)` in PDF points, clamped to
 * the page so it doesn't spill past an edge. `[x0, y0, x1, y1]`.
 */
export function stampRectAt(
  x: number,
  y: number,
  pageWidth: number,
  pageHeight: number,
  width: number = STAMP_WIDTH,
  height: number = STAMP_HEIGHT,
): [number, number, number, number] {
  const w = Math.min(width, pageWidth);
  const h = Math.min(height, pageHeight);
  const x0 = Math.max(0, Math.min(x - w / 2, pageWidth - w));
  const y0 = Math.max(0, Math.min(y - h / 2, pageHeight - h));
  return [x0, y0, x0 + w, y0 + h];
}
