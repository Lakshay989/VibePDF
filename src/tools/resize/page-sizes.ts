// SPEC: P2-PAGE-010 — standard page sizes for the resize dialog, in PDF points
// (1/72"). Portrait orientation; the user swaps width/height for landscape.
// Kept as a pure, unit-tested module so the dialog stays presentational.

export interface PageSize {
  readonly name: string;
  readonly width: number;
  readonly height: number;
}

/** PDFium's practical maximum page side (200" = 14400pt). */
export const MAX_DIMENSION_PT = 14_400;

export const STANDARD_SIZES: readonly PageSize[] = [
  { name: "Letter", width: 612, height: 792 },
  { name: "Legal", width: 612, height: 1008 },
  { name: "Tabloid", width: 792, height: 1224 },
  { name: "A3", width: 841.89, height: 1190.55 },
  { name: "A4", width: 595.28, height: 841.89 },
  { name: "A5", width: 419.53, height: 595.28 },
];

/** The preset matching `name`, or `undefined` (e.g. for the "Custom" option). */
export function findStandardSize(name: string): PageSize | undefined {
  return STANDARD_SIZES.find((s) => s.name === name);
}

/**
 * The name of the standard size whose dimensions match `width` × `height`
 * within `tol` points, or `undefined` (→ a custom size). Lets the dialog open
 * pre-selected to the page's *current* size instead of a fixed default.
 */
export function matchStandardSize(
  width: number,
  height: number,
  tol = 1,
): string | undefined {
  return STANDARD_SIZES.find(
    (s) => Math.abs(s.width - width) <= tol && Math.abs(s.height - height) <= tol,
  )?.name;
}

/** A page side must be a positive, finite number of points within the limit. */
export function isValidDimension(pt: number): boolean {
  return Number.isFinite(pt) && pt > 0 && pt <= MAX_DIMENSION_PT;
}
