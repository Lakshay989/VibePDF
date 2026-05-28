import { invoke as tauriInvoke } from "@tauri-apps/api/core";

// Typed wrapper. We never call tauriInvoke() directly from feature code;
// every command gets its own wrapper in src/ipc/<domain>.ts (see pdf.ts).
//
// Errors from Tauri commands deserialize as `CommandError` objects from
// the Rust side (see src-tauri/src/error.rs). The wrapper throws them as
// typed JS errors so callers can pattern-match on .code.
export interface CommandErrorPayload {
  code:
    | "NotFound"
    | "InvalidInput"
    // SPEC: P1-VIEW-003 — emitted by `pdf_open` when the file is
    // encrypted and the supplied password (or its absence) does not
    // unlock it. `message` carries the absolute path so the dialog
    // can show which file is being prompted for.
    | "PasswordRequired"
    | "PdfError"
    | "IoError"
    | "PermissionDenied"
    | "Internal";
  message: string;
}

export class CommandError extends Error {
  code: CommandErrorPayload["code"];
  constructor(payload: CommandErrorPayload) {
    super(payload.message);
    this.name = "CommandError";
    this.code = payload.code;
  }
}

export async function invoke<T>(
  cmd: string,
  args?: Record<string, unknown>,
): Promise<T> {
  try {
    return await tauriInvoke<T>(cmd, args);
  } catch (raw) {
    if (
      raw &&
      typeof raw === "object" &&
      "code" in raw &&
      "message" in raw
    ) {
      throw new CommandError(raw as CommandErrorPayload);
    }
    throw raw;
  }
}
