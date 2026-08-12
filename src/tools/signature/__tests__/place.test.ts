// SPEC: P6-SEC-004 (P6.A5a) — the warning in front of signature placement.
//
// Placing on a `/Sig` field is allowed — it is where the form asks you to sign
// — but until P6.B1 it is a picture and not a signature, and that must be said
// once, plainly. These tests are about detecting the case reliably and wording
// it honestly; a miss here means someone believes they signed something.

import { describe, expect, it } from "vitest";

import type { PageField } from "@/ipc/forms";
import {
  hasSeenPictureWarning,
  notePictureWarningSeen,
  pictureWarning,
  resetPictureWarning,
  SIGNATURE_HEIGHT,
  signatureFieldAt,
} from "@/tools/signature/place";

const field = (
  kind: PageField["kind"],
  rect: [number, number, number, number],
  name: string = kind,
): PageField => ({ name, kind, rect });

const sig = field("signature", [100, 500, 300, 560], "Signature1");

describe("signatureFieldAt", () => {
  it("finds a signature field the click lands inside", () => {
    expect(signatureFieldAt([sig], 200, 530)?.name).toBe("Signature1");
  });

  it("returns null for a click outside every field", () => {
    expect(signatureFieldAt([sig], 50, 530)).toBeNull();
    expect(signatureFieldAt([sig], 200, 400)).toBeNull();
  });

  it("counts the edges as inside", () => {
    // A widget's border is part of it; a click there is a click on the field,
    // and treating it as free page would place a stamp half over the box.
    expect(signatureFieldAt([sig], 100, 500)).not.toBeNull();
    expect(signatureFieldAt([sig], 300, 560)).not.toBeNull();
  });

  it("ignores fields of every other kind", () => {
    // Stamping over a text field or a checkbox is harmless — it is only a
    // signature field that would be misread as a signature.
    const others: PageField[] = [
      field("text", [0, 0, 612, 792]),
      field("checkbox", [0, 0, 612, 792]),
      field("pushbutton", [0, 0, 612, 792]),
    ];
    expect(signatureFieldAt(others, 300, 400)).toBeNull();
  });

  it("handles a rect given corner-reversed", () => {
    // `/Rect` is a pair of opposite corners and the spec does not say which
    // comes first. A field authored bottom-right-first would never register a
    // hit if the numbers were trusted in order — and would silently be stamped.
    const reversed = field("signature", [300, 560, 100, 500], "Backwards");
    expect(signatureFieldAt([reversed], 200, 530)?.name).toBe("Backwards");
  });

  it("returns the field that was hit, not merely a boolean", () => {
    // The name goes into the message, so the user knows which box they hit.
    const many = [field("signature", [0, 0, 50, 50], "A"), sig];
    expect(signatureFieldAt(many, 200, 530)?.name).toBe("Signature1");
    expect(signatureFieldAt(many, 25, 25)?.name).toBe("A");
  });

  it("is null for an empty field list", () => {
    expect(signatureFieldAt([], 200, 530)).toBeNull();
  });
});

describe("pictureWarning", () => {
  it("names the field and states plainly what this is not", () => {
    const msg = pictureWarning("Signature1");
    expect(msg).toContain("Signature1");
    expect(msg).toMatch(/not a digital signature/i);
  });

  it("says the field stays empty, which is the verifiable consequence", () => {
    // "It's only a picture" is abstract; "the field stays empty" is the thing
    // a recipient's reader will actually show them.
    expect(pictureWarning("x")).toMatch(/stays empty/i);
  });

  it("does not describe the act as signing", () => {
    // The whole point is that the word does not apply here. "signature field"
    // is the field's name, so only the verb forms are out.
    expect(pictureWarning("x")).not.toMatch(/\bsigns?\b|\bsigning\b(?! is not built)/i);
  });
});

describe("the once-per-run warning", () => {
  it("starts unseen and latches", () => {
    resetPictureWarning();
    expect(hasSeenPictureWarning()).toBe(false);
    notePictureWarningSeen();
    expect(hasSeenPictureWarning()).toBe(true);
    // Idempotent: a second field in the same form must not un-latch it.
    notePictureWarningSeen();
    expect(hasSeenPictureWarning()).toBe(true);
    resetPictureWarning();
  });
});

describe("SIGNATURE_HEIGHT", () => {
  it("is a plausible height for a signature on a form line", () => {
    // Wrong by an order of magnitude in either direction would be obvious in
    // the app but is worth a cheap guard, since nothing else pins it.
    expect(SIGNATURE_HEIGHT).toBeGreaterThanOrEqual(20);
    expect(SIGNATURE_HEIGHT).toBeLessThanOrEqual(72);
  });
});
