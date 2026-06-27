// SPEC: P4-EDIT-001 (P4.B1) — pure helpers for the click-to-edit text tool.
//
// The committed edit is rewritten in the page content stream and canvas-rendered
// after the epoch reload (the actual font is preserved by PDFium's set_text). So
// these helpers only drive the *editor preview* — a cosmetic approximation of the
// run's font while you type. DOM-free, so they stay unit-testable.

/**
 * A CSS font stack approximating a PDF font name, for the inline editor preview
 * only. Base-14 families map precisely; everything else falls into a
 * serif / monospace / sans-serif bucket by name (the same buckets A2's substitute
 * resolver uses). Never affects the saved file — `set_text` keeps the real font.
 */
export function cssFamilyForFont(fontName: string): string {
  const lower = fontName.toLowerCase();
  const isMono = ["courier", "mono", "consol", "menlo", "code"].some((s) => lower.includes(s));
  if (isMono) return "'Courier New', Courier, monospace";
  const isSerif = ["times", "georgia", "garamond", "serif", "roman", "minion", "book", "century"].some(
    (s) => lower.includes(s),
  );
  if (isSerif) return "'Times New Roman', Times, serif";
  return "Helvetica, Arial, sans-serif";
}
