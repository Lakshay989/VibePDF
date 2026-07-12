import { create } from "zustand";

// SPEC: FABLE_REVIEW 3.5 — a transient error surface. Canvas tools used to drop
// failed writes into `console.warn` only, so a rejected edit looked like a
// no-op. `reportError` (src/app/report-error.ts) pushes here; `<Toasts/>`
// renders them.

export type ToastKind = "error" | "info";

export interface Toast {
  id: number;
  kind: ToastKind;
  message: string;
}

/** Auto-dismiss delay (ms). Kept in the store so tests can drive fake timers. */
export const TOAST_TTL_MS = 6000;

interface ToastState {
  toasts: Toast[];
  /** Add a toast; returns its id. Auto-expires after {@link TOAST_TTL_MS}. */
  push: (kind: ToastKind, message: string) => number;
  dismiss: (id: number) => void;
  clear: () => void;
}

let nextId = 1;

export const useToastStore = create<ToastState>((set, get) => ({
  toasts: [],
  push: (kind, message) => {
    const id = nextId++;
    set((s) => ({ toasts: [...s.toasts, { id, kind, message }] }));
    window.setTimeout(() => get().dismiss(id), TOAST_TTL_MS);
    return id;
  },
  dismiss: (id) => set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })),
  clear: () => set({ toasts: [] }),
}));
