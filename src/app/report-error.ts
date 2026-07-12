import { CommandError } from "@/ipc/invoke";
import { useToastStore } from "@/state/toast-store";

// SPEC: FABLE_REVIEW 3.5 — the last hop of the typed-error chain. Backend
// commands return a typed `CommandError` (code + message); canvas tools call
// `reportError(context, err)` in their `.catch` so the failure becomes a visible
// toast instead of a silent `console.warn`.

/**
 * Turn a caught error into user-facing copy. Our `InvalidInput` messages are
 * already written for the user (e.g. the WinAnsi rejection), so they show
 * verbatim; other codes get the `context` prefix so the user knows what failed.
 */
export function toastMessage(context: string, err: unknown): string {
  if (err instanceof CommandError) {
    // InvalidInput messages are authored for the user — show as-is.
    if (err.code === "InvalidInput") return err.message;
    return `${context}: ${err.message}`;
  }
  if (err instanceof Error && err.message) return `${context}: ${err.message}`;
  return context;
}

/**
 * Report a failed user action: push an error toast and log for developers.
 * `context` is a short human phrase like "Couldn't add link".
 */
export function reportError(context: string, err: unknown): void {
  console.warn(context, err);
  useToastStore.getState().push("error", toastMessage(context, err));
}
