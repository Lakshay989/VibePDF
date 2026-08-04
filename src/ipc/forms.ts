import { invoke } from "@/ipc/invoke";
import type { DocumentId } from "@/ipc/pdf";

/**
 * A document's interactive-form summary, mirroring `pdf::form::FormSummary`
 * (Rust). `fieldCount` is the number of **terminal (fillable) fields** — a radio
 * group counts once; hierarchical containers are not counted, only their leaves.
 */
export interface FormSummary {
  /** Count of terminal (fillable) AcroForm fields. `0` ⇒ no fillable form. */
  fieldCount: number;
  /** Whether the form carries an `/XFA` entry (XFA fill/convert is P5.A5). */
  hasXfa: boolean;
}

/**
 * SPEC: P5-FORM-001 (P5.A1) — detect the open document's AcroForm and report a
 * field count, so the UI can surface a "Form mode" entry point. Read-only; runs
 * on the Rust document actor — the frontend never touches PDF bytes.
 */
export async function readFormSummary(id: DocumentId): Promise<FormSummary> {
  return invoke<FormSummary>("pdf_read_form_summary", { id });
}
