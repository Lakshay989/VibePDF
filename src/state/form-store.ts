// SPEC: P5-FORM-001 (P5.A1) — the interactive-form detection state + the
// "Form mode" toggle. `detected` is populated once per open document by
// `useFormDetect`; `formMode` is the entry point the later fill steps (A2–A4)
// build their field overlay on. A1 only detects and toggles — no fill yet.

import { create } from "zustand";

import type { FormSummary } from "@/ipc/forms";

interface FormState {
  /** The open document's form summary, or `null` until detected / if none. */
  detected: FormSummary | null;
  /** Whether the user has entered form-fill mode for the current document. */
  formMode: boolean;
  /**
   * True while a form input holds focus with an uncommitted edit in it.
   *
   * A text field's value lives in the overlay's local state until blur, and the
   * idle-bake backstop (`PdfViewer`) reloads the document — remounting the
   * overlay and destroying that buffer. The backstop measures "idle" by the edit
   * epoch, which typing does *not* bump, so a long note could be wiped mid-word
   * 8s after the previous field was committed. This is the signal that says
   * someone is still typing.
   */
  editing: boolean;
  setDetected: (summary: FormSummary | null) => void;
  enterFormMode: () => void;
  exitFormMode: () => void;
  setEditing: (editing: boolean) => void;
}

export const useFormStore = create<FormState>((set) => ({
  detected: null,
  formMode: false,
  editing: false,
  setDetected: (summary) => set({ detected: summary }),
  enterFormMode: () => set({ formMode: true }),
  // Leaving form mode unmounts the overlays, so nothing can still be mid-edit.
  exitFormMode: () => set({ formMode: false, editing: false }),
  setEditing: (editing) => set({ editing }),
}));
