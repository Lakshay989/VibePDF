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
import { ALL_PERMISSIONS, type DocumentId, type DocumentPermissions, protectPdf } from "@/ipc/pdf";

/**
 * SPEC: P6-SEC-009 — the seven the spec names, in the order it names them.
 *
 * Labelled as what they *allow*, matching the checked-means-permitted sense of
 * the boxes. Phrasing them as restrictions would invert the checkbox and make
 * "all boxes ticked" the most locked-down document, which is the opposite of
 * what a glance suggests.
 */
const PERMISSION_LABELS: ReadonlyArray<[keyof DocumentPermissions, string]> = [
  ["print", "Printing"],
  ["copy", "Copying text and graphics"],
  ["modify", "Changing the document"],
  ["fillForms", "Filling in form fields"],
  ["annotate", "Adding comments and annotations"],
  ["extract", "Extracting for accessibility"],
  ["assemble", "Assembling pages"],
];

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
  const [permissions, setPermissions] = useState<DocumentPermissions>(ALL_PERMISSIONS);
  const [busy, setBusy] = useState(false);

  if (!open) return null;

  const mismatch = userPassword.length > 0 && confirmPassword !== userPassword;
  const nothingSet = userPassword.length === 0 && ownerPassword.length === 0;
  const restricted = PERMISSION_LABELS.some(([key]) => !permissions[key]);
  // Only a permissions password: the copy opens for anyone, so the restrictions
  // are all it has. With none of them set it would limit nothing while every
  // reader treats it as protected, and `security/encrypt.rs` refuses it too.
  // Written since 2026-10-21, when Unlock became able to remove it.
  const ownerOnly = userPassword.length === 0 && ownerPassword.length > 0;
  const restrictsNothing = ownerOnly && !restricted;
  const canProtect = !nothingSet && !restrictsNothing && !mismatch && !busy;

  // Restrictions with no distinct owner password are close to meaningless: an
  // omitted one becomes the open password, so everyone who can read the file
  // can also change what it permits. Worth saying plainly at the moment the
  // boxes are unticked, rather than leaving the user with a false impression of
  // what they just set.
  const restrictionsAreAdvisory = restricted && ownerPassword.length === 0;

  const reset = () => {
    // Passwords do not outlive the dialog. Nothing here is stored, and a stale
    // one sitting in state through the next open would be both a surprise and
    // an unnecessary thing to keep in memory.
    setUserPassword("");
    setOwnerPassword("");
    setConfirmPassword("");
    setPermissions(ALL_PERMISSIONS);
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
          permissions,
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
          Optional. Restricts changing permissions; it does not gate opening.
        </p>

        {/* SPEC: P6-SEC-009 — permissions. */}
        <fieldset className="mb-3">
          <legend className="mb-1 text-xs text-neutral-500">Allow the reader to</legend>
          <div className="grid grid-cols-2 gap-x-3 gap-y-1">
            {PERMISSION_LABELS.map(([key, label]) => (
              <label key={key} className="flex items-center gap-1.5 text-xs">
                <input
                  type="checkbox"
                  aria-label={label}
                  checked={permissions[key]}
                  onChange={(e) =>
                    setPermissions((prev) => ({ ...prev, [key]: e.target.checked }))
                  }
                />
                <span>{label}</span>
              </label>
            ))}
          </div>
          {restrictionsAreAdvisory ? (
            <p className="mt-1.5 text-xs text-amber-700 dark:text-amber-500">
              Without a separate permissions password, anyone who can open the document
              can also lift these restrictions. Readers are not required to enforce them
              in any case.
            </p>
          ) : null}
        </fieldset>

        {mismatch ? (
          <p className="mb-2 text-xs text-red-600 dark:text-red-400">
            The two open passwords do not match.
          </p>
        ) : null}
        {nothingSet ? (
          <p className="mb-2 text-xs text-amber-700 dark:text-amber-500">
            Set a password to open the document, a password to change its permissions,
            or both.
          </p>
        ) : null}
        {ownerOnly ? (
          <p className="mb-2 text-xs text-neutral-500">
            Anyone can open this copy. The permissions password only controls what
            readers may do, and readers aren&rsquo;t required to enforce it.
          </p>
        ) : null}
        {restrictsNothing ? (
          <p className="mb-2 text-xs text-amber-700 dark:text-amber-500">
            Untick at least one permission, or set a password to open.
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
