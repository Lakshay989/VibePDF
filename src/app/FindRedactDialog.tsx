// SPEC: P6-SEC-011 (P6.D2b) — pattern redaction: find, review, then apply.
//
// The review step is the requirement, not a courtesy, so this dialog is built
// around making the list *readable* rather than around getting through it:
//
//   - every match is unticked until the user ticks it. "Select all" exists, but
//     the default is nothing, because a list that arrives pre-confirmed is a
//     list nobody reads;
//   - the matched text is shown. A confirm list that will not say what it found
//     cannot be reviewed, and the user is looking at the document anyway;
//   - pages that could not be searched are shown *as gaps*, not omitted. A
//     document reported clean because we could not look at half of it is the
//     failure this whole feature would otherwise invite.

import { useState } from "react";

import { reportError } from "@/app/report-error";
import {
  type DocumentId,
  findRedactionMatches,
  type MatchHit,
  type PatternKind,
  redactRegion,
} from "@/ipc/pdf";
import { useEditEpochStore } from "@/state/edit-epoch-store";
import { useHistoryStore } from "@/state/history-store";

interface Props {
  open: boolean;
  documentId: DocumentId;
  onClose: () => void;
}

const BUILT_INS: ReadonlyArray<{ kind: PatternKind; label: string }> = [
  { kind: "ssn", label: "Social security numbers" },
  { kind: "creditCard", label: "Card numbers" },
  { kind: "email", label: "Email addresses" },
  { kind: "phone", label: "Phone numbers" },
];

const KIND_LABEL: Record<PatternKind, string> = {
  ssn: "Social security number",
  creditCard: "Card number",
  email: "Email address",
  phone: "Phone number",
  custom: "Custom pattern",
};

export function FindRedactDialog({ open, documentId, onClose }: Props) {
  const setHistory = useHistoryStore((s) => s.setHistory);
  const bumpEpoch = useEditEpochStore((s) => s.bumpEpoch);

  const [kinds, setKinds] = useState<PatternKind[]>(["ssn", "creditCard"]);
  const [custom, setCustom] = useState("");
  const [hits, setHits] = useState<MatchHit[] | null>(null);
  const [chosen, setChosen] = useState<Set<number>>(new Set());
  const [busy, setBusy] = useState(false);

  if (!open) return null;

  const reset = () => {
    setHits(null);
    setChosen(new Set());
  };

  const close = () => {
    reset();
    onClose();
  };

  const find = () => {
    setBusy(true);
    void findRedactionMatches(documentId, {
      kinds,
      custom: custom.trim().length > 0 ? [custom.trim()] : [],
    })
      .then((found) => {
        setHits(found);
        // Nothing is pre-selected. See the note at the top of the file.
        setChosen(new Set());
      })
      .catch((err: unknown) => reportError("Couldn't search the document", err))
      .finally(() => setBusy(false));
  };

  const matches = (hits ?? []).filter((h) => !h.unreadable);
  const gaps = (hits ?? []).filter((h) => h.unreadable);

  const apply = () => {
    const picked = matches.filter((_, i) => chosen.has(i));
    if (picked.length === 0) return;
    setBusy(true);

    // Sequentially: each redaction rewrites the document, so they cannot be
    // applied against the same starting bytes in parallel.
    void picked
      .reduce<Promise<unknown>>(
        (chain, hit) =>
          chain.then(() =>
            redactRegion(documentId, hit.page, hit.rect, { removeMetadata: false }).then(
              (report) => {
                bumpEpoch(documentId);
                setHistory(documentId, report.history);
              },
            ),
          ),
        Promise.resolve(),
      )
      .then(close)
      .catch((err: unknown) => reportError("Couldn't apply the redactions", err))
      .finally(() => setBusy(false));
  };

  const toggle = (i: number) =>
    setChosen((prev) => {
      const next = new Set(prev);
      if (next.has(i)) next.delete(i);
      else next.add(i);
      return next;
    });

  return (
    <div
      role="dialog"
      aria-label="Find and redact"
      className="fixed inset-0 z-40 flex items-center justify-center bg-black/40"
    >
      <div className="flex max-h-[80vh] w-[560px] flex-col rounded-lg bg-white p-4 shadow-xl dark:bg-neutral-900">
        <h2 className="mb-1 text-sm font-medium">Find and redact</h2>
        <p className="mb-3 text-xs text-neutral-500">
          Searches the document and lists what it finds. Nothing is removed until you
          choose it here.
        </p>

        <fieldset className="mb-2 grid grid-cols-2 gap-1">
          {BUILT_INS.map(({ kind, label }) => (
            <label key={kind} className="flex items-center gap-1.5 text-xs">
              <input
                type="checkbox"
                aria-label={label}
                checked={kinds.includes(kind)}
                onChange={(e) =>
                  setKinds((prev) =>
                    e.target.checked ? [...prev, kind] : prev.filter((k) => k !== kind),
                  )
                }
              />
              <span>{label}</span>
            </label>
          ))}
        </fieldset>

        <label className="mb-3 flex flex-col gap-0.5">
          <span className="text-xs text-neutral-500">Also match this pattern (optional)</span>
          <input
            type="text"
            aria-label="Custom pattern"
            value={custom}
            onChange={(e) => setCustom(e.target.value)}
            placeholder="e.g. ACCT-\d{6}"
            className="w-full rounded border border-neutral-300 px-2 py-1 font-mono text-xs dark:border-neutral-700 dark:bg-neutral-900"
          />
        </label>

        {hits === null ? null : (
          <div className="mb-3 flex-1 overflow-y-auto border-t border-neutral-200 pt-2 dark:border-neutral-800">
            {matches.length === 0 ? (
              <p className="text-xs text-neutral-500">Nothing matched.</p>
            ) : (
              <>
                <div className="mb-1 flex items-center gap-2">
                  <span className="text-xs font-medium">
                    {matches.length} match{matches.length === 1 ? "" : "es"}
                  </span>
                  <button
                    type="button"
                    onClick={() =>
                      setChosen(
                        chosen.size === matches.length
                          ? new Set()
                          : new Set(matches.map((_, i) => i)),
                      )
                    }
                    className="ml-auto rounded border border-neutral-300 px-1.5 py-0.5 text-xs dark:border-neutral-700"
                  >
                    {chosen.size === matches.length ? "Select none" : "Select all"}
                  </button>
                </div>
                <ul className="flex flex-col gap-1">
                  {matches.map((hit, i) => (
                    <li key={`${hit.page}-${hit.rect.join(",")}-${hit.preview}`}>
                      <label className="flex items-start gap-1.5 text-xs">
                        <input
                          type="checkbox"
                          aria-label={`Redact ${hit.preview} on page ${hit.page + 1}`}
                          className="mt-0.5"
                          checked={chosen.has(i)}
                          onChange={() => toggle(i)}
                        />
                        <span>
                          <span className="font-mono">{hit.preview}</span>
                          <span className="text-neutral-500">
                            {" "}
                            — {KIND_LABEL[hit.kind]}, page {hit.page + 1}
                          </span>
                          {hit.coversWholeRun ? (
                            <span className="block text-amber-700 dark:text-amber-500">
                              This font can&rsquo;t be measured, so the whole line goes, not
                              just the match.
                            </span>
                          ) : null}
                        </span>
                      </label>
                    </li>
                  ))}
                </ul>
              </>
            )}

            {gaps.length > 0 ? (
              <p className="mt-2 text-xs text-amber-700 dark:text-amber-500">
                {gaps.length} page{gaps.length === 1 ? "" : "s"} could not be searched
                (page {gaps.map((g) => g.page + 1).join(", ")}) — the text there is drawn
                through a form. Anything on {gaps.length === 1 ? "it" : "them"} was not
                checked.
              </p>
            ) : null}
          </div>
        )}

        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={find}
            disabled={busy || (kinds.length === 0 && custom.trim().length === 0)}
            className="rounded border border-neutral-300 px-2 py-1 text-xs disabled:opacity-40 dark:border-neutral-700"
          >
            {busy && hits === null ? "Searching…" : "Find"}
          </button>
          {hits !== null && matches.length > 0 ? (
            <button
              type="button"
              onClick={apply}
              disabled={busy || chosen.size === 0}
              className="rounded bg-red-600 px-2 py-1 text-xs text-white disabled:opacity-40"
            >
              {busy ? "Redacting…" : `Redact ${chosen.size} selected`}
            </button>
          ) : null}
          <button
            type="button"
            onClick={close}
            className="ml-auto rounded border border-neutral-300 px-2 py-1 text-xs dark:border-neutral-700"
          >
            Cancel
          </button>
        </div>
      </div>
    </div>
  );
}
