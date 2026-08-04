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
  setDetected: (summary: FormSummary | null) => void;
  enterFormMode: () => void;
  exitFormMode: () => void;
}

export const useFormStore = create<FormState>((set) => ({
  detected: null,
  formMode: false,
  setDetected: (summary) => set({ detected: summary }),
  enterFormMode: () => set({ formMode: true }),
  exitFormMode: () => set({ formMode: false }),
}));
