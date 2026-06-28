// SPEC: P4-EDIT-006 (P4.C2) — pure affine-matrix helpers for image edits.
//
// A PDF placement matrix is `[a, b, c, d, e, f]`, mapping a point `(x, y)` to
// `(a·x + c·y + e, b·x + d·y + f)`. The image draws in the unit square, so the
// matrix *is* its placement. These DOM-free helpers compute the new matrix for a
// move / resize / 90° rotation, keeping the math unit-testable.

export type Matrix = [number, number, number, number, number, number];

/** Affine product `m1 · m2` (apply `m2` first, then `m1`). */
export function mul(m1: Matrix, m2: Matrix): Matrix {
  const [a1, b1, c1, d1, e1, f1] = m1;
  const [a2, b2, c2, d2, e2, f2] = m2;
  return [
    a1 * a2 + c1 * b2,
    b1 * a2 + d1 * b2,
    a1 * c2 + c1 * d2,
    b1 * c2 + d1 * d2,
    a1 * e2 + c1 * f2 + e1,
    b1 * e2 + d1 * f2 + f1,
  ];
}

/** The image's centre in PDF space (the unit square's centre, transformed). */
export function center(m: Matrix): [number, number] {
  const [a, b, c, d, e, f] = m;
  return [a * 0.5 + c * 0.5 + e, b * 0.5 + d * 0.5 + f];
}

/** Translate the placement by `(dx, dy)` PDF points (a move). */
export function translate(m: Matrix, dx: number, dy: number): Matrix {
  return [m[0], m[1], m[2], m[3], m[4] + dx, m[5] + dy];
}

/**
 * Rotate the placement 90° counter-clockwise about its own centre. Preserves any
 * existing rotation/scale (composes onto the current matrix).
 */
export function rotate90(m: Matrix): Matrix {
  const [cx, cy] = center(m);
  const toCentre: Matrix = [1, 0, 0, 1, -cx, -cy];
  const r: Matrix = [0, 1, -1, 0, 0, 0]; // 90° CCW
  const fromCentre: Matrix = [1, 0, 0, 1, cx, cy];
  return mul(mul(mul(fromCentre, r), toCentre), m);
}

/**
 * An axis-aligned placement matrix for the PDF rect `[x0, y0, x1, y1]` — used when
 * resizing (the image fills the dragged box; any prior rotation is reset).
 */
export function rectToMatrix(x0: number, y0: number, x1: number, y1: number): Matrix {
  return [x1 - x0, 0, 0, y1 - y0, x0, y0];
}
