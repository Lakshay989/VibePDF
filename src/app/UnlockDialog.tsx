// SPEC: P6-SEC-008 (P6.C2) — write an unprotected copy of the document.
//
// One field, because there is one thing to supply. The copy it writes is
// re-opened with no password before the backend returns, so "unlocked" here
// means a reader agreed, not that the call did not error.
//
// A sibling of ProtectDialog rather than a mode inside it: the two share no
// state and the combined version would spend most of its markup deciding which
// half to hide.

import { save as saveFileDialog } from "@tauri-apps/plugin-dialog";
import { useState } from "react";

import { basename } from "@/app/paths";
import { reportError } from "@/app/report-error";
import { type DocumentId, removePdfProtection } from "@/ipc/pdf";

interface Props {
  open: boolean;
  documentId: DocumentId;
  /** Name of the open file, used to suggest an output name. */
  documentName?: string;
  onClose: () => void;
}

export function UnlockDialog({ open, documentId, documentName, onClose }: Props) {
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);

  if (!open) return null;

  const unlock = () => {
    void (async () => {
      setBusy(true);
      try {
        const suggested = documentName ? basename(documentName) : "unlocked.pdf";
        const path = await saveFileDialog({
          defaultPath: suggested.replace(/(\.pdf)?$/i, "-unlocked.pdf"),
          filters: [{ name: "PDF", extensions: ["pdf"] }],
        });
        if (!path) return; // cancelled
        await removePdfProtection(documentId, path, password);
        setPassword("");
        onClose();
      } catch (err) {
        // Wrong-password and unsupported-variant both arrive here, each with
        // its own message; neither contains the password.
        reportError("Couldn't remove the protection", err);
      } finally {
        setBusy(false);
      }
    })();
  };

  return (
    <div
      role="dialog"
      aria-label="Remove password protection"
      className="fixed inset-0 z-40 flex items-center justify-center bg-black/40"
    >
      <div className="w-[460px] rounded-lg bg-white p-4 shadow-xl dark:bg-neutral-900">
        <h2 className="mb-1 text-sm font-medium">Remove password protection</h2>
        <p className="mb-3 text-xs text-neutral-500">
          Writes an unprotected copy. The document you have open is not changed.
        </p>

        <label className="mb-1 flex flex-col gap-0.5">
          <span className="text-xs text-neutral-500">The document&rsquo;s password</span>
          <input
            type="password"
            aria-label="The document's password"
            autoComplete="current-password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            className="w-full rounded border border-neutral-300 px-2 py-1 text-sm dark:border-neutral-700 dark:bg-neutral-900"
          />
        </label>
        <p className="mb-3 text-xs text-neutral-500">
          For AES-256 documents this is the permissions password, not the one that opens
          the file.
        </p>

        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={unlock}
            disabled={password.length === 0 || busy}
            className="rounded bg-blue-600 px-2 py-1 text-xs text-white disabled:opacity-40"
          >
            {busy ? "Unlocking…" : "Unlock…"}
          </button>
          <button
            type="button"
            onClick={() => {
              setPassword("");
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
