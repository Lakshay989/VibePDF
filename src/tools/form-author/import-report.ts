// SPEC: P5-FORM-009 (P5.C2) — pure helpers for the import action: picking the
// data format from the chosen file, and turning the backend's report into the
// one line the panel shows. Kept out of the component so both are unit-testable.

import type { FormDataFormat, TypeMismatch } from "@/ipc/forms";

const FORMATS: readonly FormDataFormat[] = ["fdf", "xfdf", "json", "csv"];

/**
 * The form-data format implied by a file path's extension, or `null` when the
 * extension isn't one we read. Case-insensitive; a path with no extension (or a
 * dot only in a directory segment) is `null` rather than a guess.
 */
export function formatFromPath(path: string): FormDataFormat | null {
  const base = path.split(/[\\/]/).pop() ?? "";
  const dot = base.lastIndexOf(".");
  if (dot <= 0) return null;
  const ext = base.slice(dot + 1).toLowerCase();
  return FORMATS.find((f) => f === ext) ?? null;
}

/**
 * The panel's summary line. The spec requires unmatched fields and type
 * mismatches to be *reported*, so they always appear when non-empty — never
 * collapsed into the applied count.
 */
export function describeImport(
  applied: number,
  unmatched: readonly string[],
  mismatched: readonly TypeMismatch[],
): string {
  const parts = [`Filled ${applied} field${applied === 1 ? "" : "s"}`];
  if (unmatched.length > 0) {
    parts.push(`${unmatched.length} not in this form (${unmatched.join(", ")})`);
  }
  for (const m of mismatched) {
    parts.push(`${m.name}: data says ${m.expected || "no type"}, field is ${m.got}`);
  }
  return parts.join(" · ");
}
