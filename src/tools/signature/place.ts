// SPEC: P6-SEC-004 (P6.A5a) — the decisions behind placing a signature.
//
// Pure, so the one that matters can be tested: **is the user clicking on a
// signature field?**
//
// P6-SEC-004 has two halves. A signature goes down as a stamp annotation, *or*
// — when a signature field is targeted — as a PKCS#7 digital signature. The
// second needs certificate signing (P6.B1), which does not exist yet.
//
// This originally *refused* on a `/Sig` field, reasoning that a picture over a
// signature widget yields a document that reads as signed and is not. The
// reasoning did not survive contact with a real document. That same picture two
// inches lower, on the ruled line, produces exactly the same document — and
// that was always allowed. The `/Sig` rectangle is not what makes it look
// signed; the picture is. Nothing here ever writes `/V`, so a reader that
// checks sees an unsigned field either way. a platform viewer's signature feature
// does precisely this and calls it signing.
//
// So refusing blocked the most natural action in the document — signing on the
// line marked "Signature:" — without preventing the harm it was justified by.
// What is worth protecting is that nobody is *misled*, which is a labelling
// problem: place it, and say once, unmissably, that it is a picture.

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

/**
 * What to tell someone aiming at a signature field. The wording lives next to
 * the reasoning above rather than buried in a layer component.
 *
 * It says what *will* happen rather than what is forbidden, and it avoids the
 * word "sign" for the thing it is not.
 */
export function pictureWarning(fieldName: string): string {
  return (
    `"${fieldName}" is a signature field.\n\n` +
    `This places a picture of your signature. It is not a digital signature — ` +
    `nothing in the document can be verified, and the field itself stays empty. ` +
    `Certificate signing is not built yet.`
  );
}

/**
 * Whether the warning has been shown yet in this session.
 *
 * Once per run, not once per click: the point is that nobody places one of
 * these *unaware*, and someone filling in a five-signature form already knows
 * by the second field. Deliberately not persisted — a fresh run is a fresh
 * chance to notice, and it costs one dialog.
 */
let warned = false;

export function hasSeenPictureWarning(): boolean {
  return warned;
}

export function notePictureWarningSeen(): void {
  warned = true;
}

/** Test seam — module state would otherwise leak between cases. */
export function resetPictureWarning(): void {
  warned = false;
}
