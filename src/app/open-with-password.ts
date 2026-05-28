// SPEC: P1-VIEW-003 — retry loop for encrypted-PDF opens.
//
// Pure async glue: no React, no DOM, no Tauri imports beyond what the
// IPC wrapper already pulls. The caller passes an `askForPassword`
// callback that mounts the UI; this file owns the retry policy only.
//
// Policy: 1 silent attempt with no password, then up to 3 attempts
// with a user-supplied password (= "retry up to 3 times" per the spec
// — the initial silent open is not counted as a retry). On the third
// wrong password, return `{ outcome: "failed" }`; the caller shows a
// terminal toast and does not re-prompt.

import { CommandError } from "@/ipc/invoke";
import { openPdfPath, type OpenedDocument } from "@/ipc/pdf";

export const MAX_PASSWORD_ATTEMPTS = 3;

/** Argument bag passed to the UI on each prompt. */
export interface PasswordPromptRequest {
  /** Absolute path of the file being opened. Shown in the dialog. */
  path: string;
  /** How many attempts the user has left **including** this one. Decreases from 3 to 1. */
  attemptsLeft: number;
  /**
   * Non-null on every prompt after the first one. Always
   * `"Incorrect password."` when set — the spec does not need finer
   * granularity (PDFium does not distinguish wrong-password from
   * missing-password).
   */
  lastError: string | null;
}

export type AskForPassword = (
  request: PasswordPromptRequest,
) => Promise<string | null>;

/**
 * Possible outcomes of an open-with-password attempt.
 *
 * - `opened` — success; `doc` is the freshly opened document.
 * - `cancelled` — user dismissed the dialog before exhausting attempts.
 * - `failed` — user used all 3 attempts without unlocking.
 *
 * Non-`PasswordRequired` errors propagate as exceptions; the caller
 * shows them via the existing toast / log paths.
 */
export type OpenOutcome =
  | { outcome: "opened"; doc: OpenedDocument }
  | { outcome: "cancelled" }
  | { outcome: "failed" };

/**
 * Try `openPdfPath(path)` once; if it returns `PasswordRequired`, ask
 * the UI for a password up to `MAX_PASSWORD_ATTEMPTS` times and retry.
 * The password is held in the closure of this async function and the
 * dialog's local state only — never persisted, never logged.
 */
export async function openWithPasswordPrompt(
  path: string,
  askForPassword: AskForPassword,
): Promise<OpenOutcome> {
  // First attempt: silent, no password. Most PDFs reach this path.
  try {
    return { outcome: "opened", doc: await openPdfPath(path) };
  } catch (err) {
    if (!isPasswordRequired(err)) throw err;
  }

  // Encrypted. Up to MAX_PASSWORD_ATTEMPTS password attempts.
  let lastError: string | null = null;
  for (let i = 0; i < MAX_PASSWORD_ATTEMPTS; i += 1) {
    const attemptsLeft = MAX_PASSWORD_ATTEMPTS - i;
    const pwd = await askForPassword({ path, attemptsLeft, lastError });
    if (pwd === null) return { outcome: "cancelled" };
    try {
      return { outcome: "opened", doc: await openPdfPath(path, pwd) };
    } catch (err) {
      if (!isPasswordRequired(err)) throw err;
      lastError = "Incorrect password.";
    }
  }
  return { outcome: "failed" };
}

function isPasswordRequired(err: unknown): boolean {
  return err instanceof CommandError && err.code === "PasswordRequired";
}
