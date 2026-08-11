// SPEC: P6-SEC-002 (P6.A3) — the handwriting fonts a typed signature can use.
//
// These are **system** fonts, referenced by family name; nothing is bundled.
// That means the set differs per machine, so the list below is a set of
// *candidates* and the real question is which of them this machine can actually
// render. Asking the OS is not an option from a WebView, so we measure.
//
// The failure this guards against is specific and silent: `ctx.font = '48px
// "Zapfino"'` on a machine without Zapfino does not error — it quietly falls
// back, and you get a signature in Helvetica that the user never chose. Name
// alone proves nothing.
//
// The test is width comparison. Measure the text asking for `"Family", serif`
// and again for plain `serif`; if the family resolved, the glyph advances almost
// certainly differ. Repeat against `monospace`, since a family could coincide
// with one generic's metrics but not both. Differ from either ⇒ present.
//
// Detection runs against **the text the user typed**, not a fixed sample. A font
// that exists but has no glyphs for the user's script falls back per-character,
// measures like the fallback, and is correctly reported missing — which is the
// behaviour you want for a name in Devanagari against a Latin-only script face.

/** A family we might be able to draw with, and what to call it in the picker. */
export interface FontCandidate {
  /** CSS family name, quoted at use site. */
  family: string;
  /** Shown to the user. */
  label: string;
}

/** Measures `text` under a full CSS `font` shorthand, in px. */
export type MeasureFn = (text: string, font: string) => number;

/**
 * Handwriting-ish families that ship with a mainstream OS. Ordered roughly by
 * how much they look like a signature rather than a casual note.
 *
 * Linux is deliberately thin — most distributions ship no script face at all,
 * which is the known cost of not bundling. `cursive` below is the safety net.
 */
export const CANDIDATES: readonly FontCandidate[] = [
  // macOS
  { family: "Snell Roundhand", label: "Snell Roundhand" },
  { family: "Zapfino", label: "Zapfino" },
  { family: "Bradley Hand", label: "Bradley Hand" },
  { family: "Marker Felt", label: "Marker Felt" },
  // Windows
  { family: "Segoe Script", label: "Segoe Script" },
  { family: "Lucida Handwriting", label: "Lucida Handwriting" },
  { family: "Brush Script MT", label: "Brush Script" },
  { family: "Ink Free", label: "Ink Free" },
  // Common on Linux via the URW/Ghostscript font set
  { family: "URW Chancery L", label: "URW Chancery" },
  { family: "Z003", label: "Chancery" },
];

/**
 * The generic CSS family every engine maps to *something* script-like. Offered
 * only when no named candidate is detected, so the picker is never empty and
 * the user is never silently given a fallback they did not pick.
 */
export const GENERIC_FALLBACK: FontCandidate = {
  family: "cursive",
  label: "Default handwriting",
};

/** Size used for detection. Large enough that small metric differences show. */
const PROBE_PX = 72;

/** Generic families to compare against. Two, because one could coincide. */
const SENTINELS = ["serif", "monospace"] as const;

/**
 * Whether `family` actually resolves on this machine for `text`.
 *
 * True when asking for the family changes the measured width relative to at
 * least one generic. If both measurements match their sentinel exactly, the
 * request fell through to that generic and the family is not present.
 */
export function isFontAvailable(family: string, text: string, measure: MeasureFn): boolean {
  if (text.length === 0) return false;
  return SENTINELS.some((sentinel) => {
    const withFamily = measure(text, `${PROBE_PX}px "${family}", ${sentinel}`);
    const fallbackOnly = measure(text, `${PROBE_PX}px ${sentinel}`);
    return withFamily !== fallbackOnly;
  });
}

/**
 * The candidates this machine can render `text` in. Falls back to the generic
 * script family when none are found, so the picker always offers something and
 * the UI can say plainly that it is a fallback.
 */
export function availableFonts(
  text: string,
  measure: MeasureFn,
  candidates: readonly FontCandidate[] = CANDIDATES,
): FontCandidate[] {
  const found = candidates.filter((c) => isFontAvailable(c.family, text, measure));
  return found.length > 0 ? found : [GENERIC_FALLBACK];
}

/**
 * A `MeasureFn` backed by a real canvas. Split out so callers can inject a stub
 * — the same seam `raster.ts` uses, and for the same reason: jsdom has no
 * canvas, so measuring cannot happen under test.
 */
export function canvasMeasurer(
  createCanvas: () => HTMLCanvasElement = () => document.createElement("canvas"),
): MeasureFn {
  const ctx = createCanvas().getContext("2d");
  return (text, font) => {
    if (!ctx) return 0;
    ctx.font = font;
    return ctx.measureText(text).width;
  };
}
