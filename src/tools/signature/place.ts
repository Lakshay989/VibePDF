// SPEC: P6-SEC-004 (P6.A5a) — the decisions behind placing a signature.
//
// Pure, so the one that matters can be tested: **is the user clicking on a
// signature field?**
//
// P6-SEC-004 has two halves. A signature goes down as a stamp annotation, *or*
// — when a signature field is targeted — as a PKCS#7 digital signature. The
// second needs certificate signing (P6.B1), which does not exist yet.
//
// That leaves a choice about what to do when someone clicks a `/Sig` widget
// today, and only one of the answers is safe. Stamping the picture over the
// field would produce a document that *looks* signed to every human who opens
// it and carries no signature at all — worse than a missing feature, because
// the absence is invisible. So placement declines there and says why.

import type { PageField } from "@/ipc/forms";

/**
 * Default height of a placed signature, in PDF points.
 *
 * A handwritten signature on a printed form line runs roughly 30–50pt tall;
 * 40 sits in the middle and leaves the descenders clear of a ruled line. The
 * width follows the PNG's own aspect ratio, computed on the Rust side, which is
 * why only one number is needed here.
 */
export const SIGNATURE_HEIGHT = 40;

/**
 * The signature field containing `(x, y)` (PDF points), or `null`.
 *
 * Rects are normalised rather than trusted: `/Rect` is a pair of corners and
 * nothing in the spec says which corner comes first, so a field authored
 * bottom-right-first would otherwise never register a hit.
 */
export function signatureFieldAt(
  fields: readonly PageField[],
  x: number,
  y: number,
): PageField | null {
  for (const f of fields) {
    if (f.kind !== "signature") continue;
    const [ax, ay, bx, by] = f.rect;
    if (
      x >= Math.min(ax, bx) &&
      x <= Math.max(ax, bx) &&
      y >= Math.min(ay, by) &&
      y <= Math.max(ay, by)
    ) {
      return f;
    }
  }
  return null;
}

/** What to tell someone who clicked a signature field. Named so the wording
 *  lives next to the reasoning above rather than buried in a layer component. */
export function declineMessage(fieldName: string): string {
  return (
    `"${fieldName}" is a signature field. Signing it needs a certificate, ` +
    `which isn't built yet — and placing a picture over it would look signed ` +
    `without being signed. Click elsewhere on the page to place it as a stamp.`
  );
}
