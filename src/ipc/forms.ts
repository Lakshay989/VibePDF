import { type HistoryState } from "@/ipc/history";
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

/**
 * One fillable text field on a page, mirroring `pdf::form::FormField` (Rust).
 * `rect` is `[x0, y0, x1, y1]` in PDF points (page space, origin bottom-left).
 */
export interface FormField {
  /** Fully-qualified field name — the handle `fillTextField` addresses. */
  name: string;
  rect: [number, number, number, number];
  /** Current value (`/V`). */
  value: string;
  /** Character cap (`/MaxLen`), or null. */
  maxLen: number | null;
  /** Multi-line text field (`/Ff` bit 13). */
  multiline: boolean;
}

/**
 * SPEC: P5-FORM-002 (P5.A2) — read the fillable text fields on `page` (0-based),
 * with geometry + current value, to place the fill overlay. Read-only.
 */
export async function readTextFields(id: DocumentId, page: number): Promise<FormField[]> {
  return invoke<FormField[]>("pdf_read_text_fields", { id, page });
}

/**
 * SPEC: P5-FORM-002 — set the text field `name` to `value` (the backend truncates
 * to the field's `/MaxLen`). Undoable; runs on the document actor.
 */
export async function fillTextField(
  id: DocumentId,
  name: string,
  value: string,
): Promise<HistoryState> {
  return invoke<HistoryState>("pdf_fill_text_field", { id, name, value });
}

/**
 * One clickable button widget (a checkbox, or one option of a radio group),
 * mirroring `pdf::form::ButtonField` (Rust). `rect` is `[x0, y0, x1, y1]` in PDF
 * points. `onState` is this widget's "on" appearance-state name.
 */
export interface ButtonField {
  /** Fully-qualified name of the owning field (the group, for a radio option). */
  fieldName: string;
  kind: "checkbox" | "radio";
  rect: [number, number, number, number];
  onState: string;
  checked: boolean;
}

/**
 * SPEC: P5-FORM-003 (P5.A3) — read the checkbox/radio widgets on `page`
 * (0-based), with geometry + on/off state, to place the button overlay.
 */
export async function readButtonFields(id: DocumentId, page: number): Promise<ButtonField[]> {
  return invoke<ButtonField[]>("pdf_read_button_fields", { id, page });
}

/**
 * SPEC: P5-FORM-003 — toggle/select a button field: set `name` to `onState`
 * (checked) or `/Off` (unchecked). Undoable; runs on the document actor.
 */
export async function setButtonField(
  id: DocumentId,
  name: string,
  onState: string,
  checked: boolean,
): Promise<HistoryState> {
  return invoke<HistoryState>("pdf_set_button_field", { id, name, onState, checked });
}

/** One option of a choice field, mirroring `pdf::form::ChoiceOption`. */
export interface ChoiceOption {
  /** The value stored in `/V`. */
  export: string;
  /** The label shown to the user (equals `export` for a bare-string option). */
  label: string;
}

/**
 * A combo box or list box, mirroring `pdf::form::ChoiceField`. `rect` is
 * `[x0, y0, x1, y1]` in PDF points.
 */
export interface ChoiceField {
  name: string;
  kind: "combo" | "list";
  rect: [number, number, number, number];
  options: ChoiceOption[];
  /** Currently-selected export values. */
  selected: string[];
  /** Multi-select (list boxes only). */
  multi: boolean;
}

/**
 * SPEC: P5-FORM-004 (P5.A4) — read the choice fields on `page` (0-based) with
 * their options + current selection, to place the choice overlay.
 */
export async function readChoiceFields(id: DocumentId, page: number): Promise<ChoiceField[]> {
  return invoke<ChoiceField[]>("pdf_read_choice_fields", { id, page });
}

/**
 * SPEC: P5-FORM-004 — set a choice field's selection to `values` (declared
 * export values). Undoable; runs on the document actor.
 */
export async function setChoiceField(
  id: DocumentId,
  name: string,
  values: string[],
): Promise<HistoryState> {
  return invoke<HistoryState>("pdf_set_choice_field", { id, name, values });
}

/**
 * SPEC: P5-FORM-005 (P5.A5) — drop the dynamic XFA layer of an XFA-only document
 * (remove `/XFA` + set `/NeedAppearances`) so its static content renders. VibePDF
 * doesn't render XFA; this is the "convert to flat content (read-only)" action.
 * Undoable; runs on the document actor.
 */
export async function stripXfa(id: DocumentId): Promise<HistoryState> {
  return invoke<HistoryState>("pdf_strip_xfa", { id });
}

/** Configuration for a new text field (P5.B1). */
export interface NewTextField {
  name: string;
  defaultValue: string;
  /** Character cap, or null for none. */
  maxLen: number | null;
  multiline: boolean;
  required: boolean;
}

/**
 * SPEC: P5-FORM-006 (P5.B1) — create a text field on `page` (0-based) at `rect`
 * (`[x0,y0,x1,y1]` PDF points). Rejects a duplicate name. Undoable; runs on the
 * document actor.
 */
export async function addTextField(
  id: DocumentId,
  page: number,
  rect: [number, number, number, number],
  field: NewTextField,
): Promise<HistoryState> {
  return invoke<HistoryState>("pdf_add_text_field", {
    id,
    page,
    rect,
    name: field.name,
    defaultValue: field.defaultValue,
    maxLen: field.maxLen,
    multiline: field.multiline,
    required: field.required,
  });
}

/** Spec for a non-text field to create (P5.B2). Text uses `addTextField`. */
export type NewFieldSpec =
  | { kind: "checkbox"; required: boolean }
  | { kind: "radio"; options: string[] }
  | { kind: "combo"; options: string[]; defaultValue: string }
  | { kind: "list"; options: string[]; multi: boolean }
  | { kind: "signature" }
  | { kind: "pushbutton"; caption: string };

/**
 * SPEC: P5-FORM-007 (P5.B2) — create a checkbox / radio / combo / list /
 * signature / push-button field on `page` (0-based) at `rect`. Undoable; runs on
 * the document actor.
 */
export async function addField(
  id: DocumentId,
  page: number,
  rect: [number, number, number, number],
  name: string,
  spec: NewFieldSpec,
): Promise<HistoryState> {
  return invoke<HistoryState>("pdf_add_field", {
    id,
    page,
    rect,
    name,
    kind: spec.kind,
    options: "options" in spec ? spec.options : [],
    defaultValue: spec.kind === "combo" ? spec.defaultValue : "",
    multi: spec.kind === "list" ? spec.multi : false,
    required: spec.kind === "checkbox" ? spec.required : false,
    caption: spec.kind === "pushbutton" ? spec.caption : "",
  });
}
