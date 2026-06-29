// SPEC: P4-EDIT-007 (P4.C3) — the target of a hyperlink, its validation, and the
// wire encoding. Pure (no PDF, no IPC) so it can be unit-tested directly.

/** The four hyperlink target kinds the spec requires. */
export type LinkKind = "url" | "email" | "page" | "named";

/**
 * SPEC: P4-EDIT-007b — a link's on-page appearance. `box` and `underline` are
 * stored as a generated `/AP` in {@link DEFAULT_LINK_COLOR}; `invisible` is a
 * borderless hot-zone.
 */
export type LinkStyle = "box" | "underline" | "invisible";

/** Human labels for the appearance selector. `box` is the default. */
export const LINK_STYLE_LABELS: Record<LinkStyle, string> = {
  box: "Box",
  underline: "Underline",
  invisible: "Invisible",
};

/** Default appearance + colour for a new link: a blue box. */
export const DEFAULT_LINK_STYLE: LinkStyle = "box";
export const DEFAULT_LINK_COLOR = "#0000ff";

export interface LinkTarget {
  kind: LinkKind;
  /**
   * As typed by the user: a URL, an email address, a **1-based** page number
   * (human-facing), or a named-destination string. Converted to the wire form
   * by {@link toWireValue}.
   */
  value: string;
}

export type Validation = { ok: true } | { ok: false; reason: string };

/** Human label for each kind, for the target-type selector. */
export const LINK_KIND_LABELS: Record<LinkKind, string> = {
  url: "Web URL",
  email: "Email",
  page: "Page in document",
  named: "Named destination",
};

/**
 * Validate a target against its kind. `pageCount` bounds a `page` target (the
 * user types a 1-based number). A `url` must carry a scheme so the reference is
 * unambiguous to a reader; `mailto:` is added for `email` at the wire boundary,
 * never by the user.
 */
export function validateTarget(target: LinkTarget, pageCount: number): Validation {
  const value = target.value.trim();
  if (value === "") return { ok: false, reason: "Enter a value." };
  switch (target.kind) {
    case "url":
      return /^[a-z][a-z0-9+.-]*:\/\//i.test(value)
        ? { ok: true }
        : { ok: false, reason: "URL needs a scheme, e.g. https://" };
    case "email":
      return /^[^@\s]+@[^@\s]+\.[^@\s]+$/.test(value)
        ? { ok: true }
        : { ok: false, reason: "Enter a valid email address." };
    case "page": {
      const n = Number(value);
      return Number.isInteger(n) && n >= 1 && n <= pageCount
        ? { ok: true }
        : { ok: false, reason: `Page must be between 1 and ${pageCount}.` };
    }
    case "named":
      return { ok: true };
  }
}

/**
 * The `value` string passed to `pdf_add_link`. A `page` target is converted from
 * the user's 1-based number to the 0-based index the Rust command expects;
 * everything else is sent verbatim (the `mailto:` prefix is added Rust-side).
 */
export function toWireValue(target: LinkTarget): string {
  const value = target.value.trim();
  if (target.kind === "page") return String(Number(value) - 1);
  return value;
}
