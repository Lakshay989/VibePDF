// SPEC: P3-ANN-006 (P3.C3a/C3b) — the built-in stamp library + placement geometry.
// Pure data/maths so it's testable without React. A stamp is rendered as a
// `/Stamp` annotation's `/AP` on the Rust side; here we only carry the spec and
// compute where a click drops the box. A stamp is either a coloured text label
// (C3a) or an embedded image (C3b, optionally with a label).

/** A coloured text rubber-stamp (C3a). */
export interface TextStampSpec {
  kind: "text";
  /** A PDF `/Name` token (informational) + the palette id. */
  name: string;
  /** The visible label (also the annotation's `/Contents`). */
  label: string;
  /** Border + text colour, hex. */
  color: string;
}

/** An image stamp from a picked PNG (C3b), optionally with an overlaid label. */
export interface ImageStampSpec {
  kind: "image";
  /** Display id (the file's base name). */
  name: string;
  /** Absolute path to the PNG; the backend reads + embeds it. */
  imagePath: string;
  /** Optional label drawn over the image (the "combination" stamp). */
  label?: string;
}

/**
 * SPEC: P6-SEC-004 (P6.A5a) — a signature from the library, armed for placing.
 *
 * Carries an **id**, not bytes and not a path: only the Rust command layer knows
 * where the library lives (P6.A1), so the frontend arms a reference and the
 * backend resolves it.
 */
export interface SignatureStampSpec {
  kind: "signature";
  /** Display id — the library entry's id, which is also its blob filename. */
  name: string;
  /** Library entry id, resolved to PNG bytes by `pdf_place_signature`. */
  signatureId: string;
}

export type StampSpec = TextStampSpec | ImageStampSpec | SignatureStampSpec;

/** Default placement height (PDF points) for an image stamp; width follows the
 *  image's aspect ratio, computed on the Rust side. */
export const IMAGE_STAMP_HEIGHT = 64;

const RED = "#c0392b";
const GREEN = "#1e8449";
const BLUE = "#1f6feb";
const GRAY = "#555555";

/** The built-in rubber-stamp library. */
export const BUILTIN_STAMPS: readonly TextStampSpec[] = [
  { kind: "text", name: "Approved", label: "APPROVED", color: GREEN },
  { kind: "text", name: "Reviewed", label: "REVIEWED", color: GREEN },
  { kind: "text", name: "Received", label: "RECEIVED", color: BLUE },
  { kind: "text", name: "Final", label: "FINAL", color: BLUE },
  { kind: "text", name: "Confidential", label: "CONFIDENTIAL", color: RED },
  { kind: "text", name: "NotApproved", label: "NOT APPROVED", color: RED },
  { kind: "text", name: "Void", label: "VOID", color: RED },
  { kind: "text", name: "Draft", label: "DRAFT", color: GRAY },
];

/** Default stamp box size in PDF points. */
export const STAMP_WIDTH = 150;
export const STAMP_HEIGHT = 46;

/** A custom text stamp (default colour red). */
export function customStamp(text: string, color: string = RED): TextStampSpec {
  return { kind: "text", name: "Custom", label: text, color };
}

/** An image stamp from a picked PNG `path`, with an optional overlaid `label`.
 *  `name` defaults to the file's base name. */
export function imageStamp(path: string, label?: string): ImageStampSpec {
  const base = path.split(/[\\/]/).pop() ?? path;
  return { kind: "image", name: base, imagePath: path, ...(label ? { label } : {}) };
}

/** SPEC: P6-SEC-004 (P6.A5a) — arm a stored signature for placement. */
export function signatureStamp(signatureId: string): SignatureStampSpec {
  return { kind: "signature", name: signatureId, signatureId };
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
