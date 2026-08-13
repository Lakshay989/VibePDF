// SPEC: P6-SEC-012 (P6.D3) — remove everything from the document that is not
// its visible content.
//
// Seven checkboxes, all off. Nothing here is a safe default: each one deletes
// something the document currently has, so the dialog opens doing nothing and
// the user opts in. (This is the mirror image of `ProtectDialog`, where a
// ticked box *grants* a permission — worth keeping straight, since both are
// seven booleans in a modal.)
//
// This edits the open document rather than writing a copy, so the result is
// visible immediately and Undo brings it back. That undo lasts until the file
// is saved and reopened, which the dialog says out loud: "clean" is exactly the
// operation people run right before sending a file somewhere, and it is worth
// being clear about the point of no return.

import { useState } from "react";

import { reportError } from "@/app/report-error";
import {
  CLEAN_NOTHING,
  type CleanOptions,
  type CleanReport,
  cleanDocument,
  type DocumentId,
} from "@/ipc/pdf";

interface Props {
  open: boolean;
  documentId: DocumentId;
  onClose: () => void;
}

/** The seven categories P6-SEC-012 names, with what each actually removes. */
const CATEGORIES: ReadonlyArray<{
  key: keyof CleanOptions;
  label: string;
  detail: string;
}> = [
  {
    key: "metadata",
    label: "Document metadata",
    detail: "Title, author, creator, producer and any custom keys — from both /Info and XMP.",
  },
  {
    key: "comments",
    label: "Comments and markup",
    detail: "Notes, highlights, ink, stamps. Links and form fields stay.",
  },
  {
    key: "formData",
    label: "Form data",
    detail: "Clears every field's value. The form itself stays, ready to fill in again.",
  },
  {
    key: "bookmarks",
    label: "Bookmarks",
    detail: "The whole outline tree.",
  },
  {
    key: "attachments",
    label: "Attachments",
    detail: "File-attachment annotations placed on a page.",
  },
  {
    key: "embeddedFiles",
    label: "Embedded files",
    detail: "Files attached to the document rather than to a page.",
  },
  {
    key: "hiddenText",
    label: "Hidden text",
    detail:
      "Text that is in the file but never drawn. This includes the searchable layer of a " +
      "scanned page — removing it makes that scan an image again.",
  },
];

/** Report fields worth showing, in the order the categories appear. */
const COUNTS: ReadonlyArray<[keyof CleanReport, string]> = [
  ["infoKeys", "metadata entries"],
  ["xmpPackets", "XMP packets"],
  ["comments", "comments"],
  ["formFields", "form fields cleared"],
  ["bookmarks", "bookmarks"],
  ["attachments", "attachments"],
  ["embeddedFiles", "embedded files"],
  ["hiddenTextRuns", "hidden text runs"],
];

export function CleanDialog({ open, documentId, onClose }: Props) {
  const [opts, setOpts] = useState<CleanOptions>(CLEAN_NOTHING);
  const [busy, setBusy] = useState(false);
  const [done, setDone] = useState<CleanReport | null>(null);

  if (!open) return null;

  const nothingChosen = CATEGORIES.every(({ key }) => !opts[key]);

  const close = () => {
    setOpts(CLEAN_NOTHING);
    setDone(null);
    onClose();
  };

  const clean = () => {
    void (async () => {
      setBusy(true);
      try {
        // Kept open on success, showing the counts. The page looks identical
        // afterwards, so closing straight away would leave no evidence that
        // anything happened at all.
        setDone(await cleanDocument(documentId, opts));
      } catch (err) {
        reportError("Couldn't clean the document", err);
      } finally {
        setBusy(false);
      }
    })();
  };

  const removed = done
    ? COUNTS.filter(([key]) => typeof done[key] === "number" && (done[key] as number) > 0)
    : [];

  return (
    <div
      role="dialog"
      aria-label="Clean document"
      className="fixed inset-0 z-40 flex items-center justify-center bg-black/40"
    >
      <div className="w-[520px] rounded-lg bg-white p-4 shadow-xl dark:bg-neutral-900">
        <h2 className="mb-1 text-sm font-medium">Clean document</h2>
        <p className="mb-3 text-xs text-neutral-500">
          Removes what you tick from the open document. Undo works until you save and
          reopen the file.
        </p>

        {done ? (
          <div className="mb-3">
            {removed.length > 0 ? (
              <>
                <p className="mb-1 text-xs font-medium">Removed:</p>
                <ul className="text-xs text-neutral-600 dark:text-neutral-400">
                  {removed.map(([key, noun]) => (
                    <li key={key}>
                      {String(done[key])} {noun}
                    </li>
                  ))}
                </ul>
              </>
            ) : (
              <p className="text-xs text-neutral-500">
                Nothing to remove — the document had none of what you selected.
              </p>
            )}
          </div>
        ) : (
          <fieldset className="mb-3 flex flex-col gap-2">
            {CATEGORIES.map(({ key, label, detail }) => (
              <label key={key} className="flex gap-2 text-xs">
                <input
                  type="checkbox"
                  aria-label={label}
                  className="mt-0.5"
                  checked={opts[key]}
                  onChange={(e) => setOpts((prev) => ({ ...prev, [key]: e.target.checked }))}
                />
                <span>
                  <span className="font-medium">{label}</span>
                  <span className="block text-neutral-500">{detail}</span>
                </span>
              </label>
            ))}
          </fieldset>
        )}

        {!done && opts.hiddenText ? (
          <p className="mb-2 text-xs text-amber-700 dark:text-amber-500">
            Removing hidden text un-searches any scanned page in this document. The
            words are still on screen as part of the picture; nothing will find them.
          </p>
        ) : null}

        <div className="flex items-center gap-2">
          {done ? null : (
            <button
              type="button"
              onClick={clean}
              disabled={nothingChosen || busy}
              className="rounded bg-blue-600 px-2 py-1 text-xs text-white disabled:opacity-40"
            >
              {busy ? "Cleaning…" : "Clean"}
            </button>
          )}
          <button
            type="button"
            onClick={close}
            className="ml-auto rounded border border-neutral-300 px-2 py-1 text-xs dark:border-neutral-700"
          >
            {done ? "Close" : "Cancel"}
          </button>
        </div>
      </div>
    </div>
  );
}
