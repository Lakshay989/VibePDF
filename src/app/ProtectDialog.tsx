// SPEC: P6-SEC-007 (P6.C1) — password-protect a copy of the document.
//
// Two passwords that do different jobs, and the difference is the whole of this
// dialog's copy: the **user** password decides whether the file opens at all,
// the **owner** password decides who may change its permissions. Either alone
// is a legitimate choice, so neither field is required — but one of them is,
// and the backend refuses a request with neither rather than writing a file
// that announces itself as encrypted and opens for anyone.
//
// This writes a *copy*. The open document keeps its own password, undo history
// and unsaved edits; nothing here changes what you are looking at.

import { save as saveFileDialog } from "@tauri-apps/plugin-dialog";
import { useState } from "react";

import { basename } from "@/app/paths";
import { reportError } from "@/app/report-error";
import { type DocumentId, protectPdf } from "@/ipc/pdf";

interface Props {
  open: boolean;
  documentId: DocumentId;
  /** Name of the open file, used to suggest an output name. */
  documentName?: string;
  onClose: () => void;
}

export function ProtectDialog({ open, documentId, documentName, onClose }: Props) {
  const [userPassword, setUserPassword] = useState("");
  const [ownerPassword, setOwnerPassword] = useState("");
  const [confirmPassword, setConfirmPassword] = useState("");
  const [busy, setBusy] = useState(false);

  if (!open) return null;

  const mismatch = userPassword.length > 0 && confirmPassword !== userPassword;
  const nothingSet = userPassword.length === 0 && ownerPassword.length === 0;
  const canProtect = !nothingSet && !mismatch && !busy;

  const reset = () => {
    // Passwords do not outlive the dialog. Nothing here is stored, and a stale
    // one sitting in state through the next open would be both a surprise and
    // an unnecessary thing to keep in memory.
    setUserPassword("");
    setOwnerPassword("");
    setConfirmPassword("");
  };

  const protect = () => {
    void (async () => {
      setBusy(true);
      try {
        const suggested = documentName ? basename(documentName) : "protected.pdf";
        const path = await saveFileDialog({
          defaultPath: suggested.replace(/(\.pdf)?$/i, "-protected.pdf"),
          filters: [{ name: "PDF", extensions: ["pdf"] }],
        });
        if (!path) return; // cancelled
        await protectPdf(
          documentId,
          path,
          userPassword.length > 0 ? userPassword : null,
          ownerPassword.length > 0 ? ownerPassword : null,
        );
        reset();
        onClose();
      } catch (err) {
        // The message never contains a password — see `pdf_protect`.
        reportError("Couldn't protect the document", err);
      } finally {
        setBusy(false);
      }
    })();
  };

  return (
    <div
      role="dialog"
      aria-label="Password protect"
      className="fixed inset-0 z-40 flex items-center justify-center bg-black/40"
    >
      <div className="w-[460px] rounded-lg bg-white p-4 shadow-xl dark:bg-neutral-900">
        <h2 className="mb-1 text-sm font-medium">Password protect a copy</h2>
        <p className="mb-3 text-xs text-neutral-500">
          Writes an AES-256 encrypted copy. The document you have open is not changed.
        </p>

        <label className="mb-2 flex flex-col gap-0.5">
          <span className="text-xs text-neutral-500">Password to open (optional)</span>
          <input
            type="password"
            aria-label="Password to open"
            autoComplete="new-password"
            value={userPassword}
            onChange={(e) => setUserPassword(e.target.value)}
            className="w-full rounded border border-neutral-300 px-2 py-1 text-sm dark:border-neutral-700 dark:bg-neutral-900"
          />
        </label>

        {userPassword.length > 0 ? (
          <label className="mb-2 flex flex-col gap-0.5">
            <span className="text-xs text-neutral-500">Confirm password to open</span>
            <input
              type="password"
              aria-label="Confirm password to open"
              autoComplete="new-password"
              value={confirmPassword}
              onChange={(e) => setConfirmPassword(e.target.value)}
              className="w-full rounded border border-neutral-300 px-2 py-1 text-sm dark:border-neutral-700 dark:bg-neutral-900"
            />
          </label>
        ) : null}

        <label className="mb-1 flex flex-col gap-0.5">
          <span className="text-xs text-neutral-500">
            Password to change permissions (optional)
          </span>
          <input
            type="password"
            aria-label="Password to change permissions"
            autoComplete="new-password"
            value={ownerPassword}
            onChange={(e) => setOwnerPassword(e.target.value)}
            className="w-full rounded border border-neutral-300 px-2 py-1 text-sm dark:border-neutral-700 dark:bg-neutral-900"
          />
        </label>
        <p className="mb-3 text-xs text-neutral-500">
          With only this one set, the document opens for anyone but is restricted.
        </p>

        {mismatch ? (
          <p className="mb-2 text-xs text-red-600 dark:text-red-400">
            The two open passwords do not match.
          </p>
        ) : null}
        {nothingSet ? (
          <p className="mb-2 text-xs text-amber-700 dark:text-amber-500">
            Set at least one password. A file with neither would say it was encrypted and
            open for anyone.
          </p>
        ) : null}

        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={protect}
            disabled={!canProtect}
            className="rounded bg-blue-600 px-2 py-1 text-xs text-white disabled:opacity-40"
          >
            {busy ? "Protecting…" : "Protect…"}
          </button>
          <button
            type="button"
            onClick={() => {
              reset();
              onClose();
            }}
            className="ml-auto rounded border border-neutral-300 px-2 py-1 text-xs dark:border-neutral-700"
          >
            Cancel
          </button>
        </div>
      </div>
    </div>
  );
}
