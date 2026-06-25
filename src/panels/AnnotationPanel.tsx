// SPEC: P3-ANN-008 (P3.D1) — the annotation sidebar.
//
// Reads every annotation out of the PDF (read-only, via the actor) and lists
// them grouped by page, with search + filter by type / author / date. Clicking
// an entry scrolls to its page and selects it (a dashed highlight is drawn by
// `SelectionHighlightLayer`). Like the note overlay it re-reads on the edit epoch
// so the list stays in step with edits/undo. No PDF bytes are touched here.

import { open as openFileDialog, save as saveFileDialog } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useMemo, useState } from "react";

import { type AnnotationInfo, deleteAnnotation, readAnnotations } from "@/ipc/annotations";
import { readFreeText } from "@/ipc/freetext";
import { flattenAnnotations } from "@/ipc/flatten";
import { exportAnnotations, importAnnotations } from "@/ipc/interchange";
import type { DocumentId } from "@/ipc/pdf";
import { useAnnotationEditStore } from "@/state/annotation-edit-store";
import { addReply } from "@/ipc/replies";
import {
  type AnnotationFilter,
  buildThreads,
  dateInputToMs,
  distinctAuthors,
  distinctKinds,
  EMPTY_FILTER,
  filterAnnotations,
  groupByPage,
  kindLabel,
  msToDateInput,
} from "@/panels/annotation-filter";
import { DEFAULT_NOTE_AUTHOR } from "@/tools/sticky-note/note-tool";
import { useAnnotationSelectionStore } from "@/state/annotation-selection-store";
import { useEditEpochStore } from "@/state/edit-epoch-store";
import { useHistoryStore } from "@/state/history-store";

/** True when keyboard focus is in a text input — so a Delete key edits text,
 *  not the selected annotation. */
function isEditingText(): boolean {
  const el = document.activeElement;
  if (!(el instanceof HTMLElement)) return false;
  const tag = el.tagName;
  return tag === "INPUT" || tag === "TEXTAREA" || el.isContentEditable;
}

interface Props {
  documentId: DocumentId;
  /** Bumped on every reload-edit; drives a re-read so the list tracks edits. */
  epoch: number;
  onJump: (page: number) => void;
}

export function AnnotationPanel({ documentId, epoch, onJump }: Props) {
  const [list, setList] = useState<AnnotationInfo[] | null>(null);
  const [filter, setFilter] = useState<AnnotationFilter>(EMPTY_FILTER);
  const selected = useAnnotationSelectionStore((s) => s.selected);
  const select = useAnnotationSelectionStore((s) => s.select);
  const setHistory = useHistoryStore((s) => s.setHistory);
  const bumpEpoch = useEditEpochStore((s) => s.bumpEpoch);
  const requestEdit = useAnnotationEditStore((s) => s.requestEdit);
  const selectedId = selected?.id ?? null;

  // SPEC: P3-ANN-013 — re-edit a free-text box: read its text + style, scroll to
  // it, and post an edit request the page's FreeTextLayer opens pre-filled.
  const edit = useCallback(
    (info: AnnotationInfo) => {
      readFreeText(documentId, info.id)
        .then((data) => {
          if (!data) return;
          onJump(info.page + 1);
          requestEdit({ nm: info.id, page: info.page, data });
        })
        .catch((err: unknown) => console.warn("read free-text failed", documentId, err));
    },
    [documentId, onJump, requestEdit],
  );

  // SPEC: P3-ANN-012 — delete an annotation by its handle, clear the selection,
  // and refresh: bumping the epoch reloads the canvas (its /AP is gone) and
  // re-reads this list; a note's overlay icon clears via its own epoch re-sync.
  const remove = useCallback(
    (info: AnnotationInfo) => {
      select(null);
      deleteAnnotation(documentId, info.id)
        .then((h) => {
          setHistory(documentId, h);
          bumpEpoch(documentId);
        })
        .catch((err: unknown) => console.warn("delete annotation failed", documentId, err));
    },
    [documentId, select, setHistory, bumpEpoch],
  );

  // Delete / Backspace removes the selected annotation (unless typing in a field).
  useEffect(() => {
    if (!selected) return undefined;
    const onKey = (e: KeyboardEvent) => {
      if ((e.key === "Delete" || e.key === "Backspace") && !isEditingText()) {
        e.preventDefault();
        remove(selected);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [selected, remove]);

  useEffect(() => {
    let cancelled = false;
    readAnnotations(documentId)
      .then((rows) => {
        if (!cancelled) setList(rows);
      })
      .catch((err: unknown) => {
        if (!cancelled) {
          setList([]);
          console.warn("read annotations failed", documentId, err);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [documentId, epoch]);

  // SPEC: P3-ANN-009 — group into threads; the filter/list operate on the
  // top-level roots, with replies nested under their parent.
  const all = useMemo(() => list ?? [], [list]);
  const threads = useMemo(() => buildThreads(all), [all]);
  const roots = useMemo(() => threads.map((t) => t.root), [threads]);
  const repliesByRoot = useMemo(
    () => new Map(threads.map((t) => [t.root.id, t.replies] as const)),
    [threads],
  );
  const authors = useMemo(() => distinctAuthors(roots), [roots]);
  const kinds = useMemo(() => distinctKinds(roots), [roots]);
  const groups = useMemo(() => groupByPage(filterAnnotations(roots, filter)), [roots, filter]);
  const shown = groups.reduce((n, g) => n + g.items.length, 0);

  // SPEC: P3-ANN-009 — the row whose reply composer is open + its draft text.
  const [replyingTo, setReplyingTo] = useState<string | null>(null);
  const [replyText, setReplyText] = useState("");
  const sendReply = useCallback(
    (parentId: string, text: string) => {
      const content = text.trim();
      setReplyingTo(null);
      setReplyText("");
      if (!content) return;
      addReply(documentId, parentId, DEFAULT_NOTE_AUTHOR, content)
        .then((h) => {
          setHistory(documentId, h);
          bumpEpoch(documentId);
        })
        .catch((err: unknown) => console.warn("add reply failed", documentId, err));
    },
    [documentId, setHistory, bumpEpoch],
  );

  // SPEC: P3-ANN-010 — export every annotation to an XFDF sidecar, or import one
  // back. A transient `note` reports the outcome; import bumps the epoch so the
  // canvas + this list pick up the recreated annotations.
  const [note, setNote] = useState<string | null>(null);
  useEffect(() => {
    if (!note) return undefined;
    const t = window.setTimeout(() => setNote(null), 3000);
    return () => window.clearTimeout(t);
  }, [note]);

  const doExport = useCallback(() => {
    void (async () => {
      try {
        const path = await saveFileDialog({
          defaultPath: "annotations.xfdf",
          filters: [{ name: "XFDF annotations", extensions: ["xfdf"] }],
        });
        if (!path) return; // cancelled
        const n = await exportAnnotations(documentId, path);
        setNote(`Exported ${n} annotation${n === 1 ? "" : "s"}`);
      } catch (err) {
        console.warn("export annotations failed", documentId, err);
        setNote(err instanceof Error ? err.message : "Export failed");
      }
    })();
  }, [documentId]);

  const doImport = useCallback(() => {
    void (async () => {
      try {
        const selected = await openFileDialog({
          multiple: false,
          filters: [{ name: "XFDF annotations", extensions: ["xfdf"] }],
        });
        if (selected === null || Array.isArray(selected)) return; // cancelled
        const h = await importAnnotations(documentId, selected);
        setHistory(documentId, h);
        bumpEpoch(documentId);
        setNote("Annotations imported");
      } catch (err) {
        console.warn("import annotations failed", documentId, err);
        setNote(err instanceof Error ? err.message : "Import failed");
      }
    })();
  }, [documentId, setHistory, bumpEpoch]);

  // SPEC: P3-ANN-011 — flatten bakes appearance-bearing annotations (everything
  // except /AP-less notes/replies) into the page. Permanent after save, so it's
  // gated behind an inline confirm.
  const flattenable = useMemo(() => all.filter((a) => a.kind !== "note").length, [all]);
  const [confirmFlatten, setConfirmFlatten] = useState(false);
  const doFlatten = useCallback(() => {
    setConfirmFlatten(false);
    select(null);
    flattenAnnotations(documentId)
      .then((h) => {
        setHistory(documentId, h);
        bumpEpoch(documentId);
        setNote("Annotations flattened");
      })
      .catch((err: unknown) => {
        console.warn("flatten annotations failed", documentId, err);
        setNote(err instanceof Error ? err.message : "Flatten failed");
      });
  }, [documentId, select, setHistory, bumpEpoch]);

  const toggleKind = (kind: (typeof kinds)[number]) =>
    setFilter((f) => ({
      ...f,
      kinds: f.kinds.includes(kind) ? f.kinds.filter((k) => k !== kind) : [...f.kinds, kind],
    }));

  return (
    <aside className="flex h-full w-72 flex-col border-r border-neutral-200 bg-neutral-50 dark:border-neutral-800 dark:bg-neutral-950">
      <header className="flex items-center gap-2 border-b border-neutral-200 px-3 py-2 text-xs font-semibold uppercase tracking-wide text-neutral-500 dark:border-neutral-800">
        <span>
          Annotations
          {list ? <span className="ml-1 text-neutral-400">({shown})</span> : null}
        </span>
        {/* SPEC: P3-ANN-010 — XFDF interchange. */}
        <div className="ml-auto flex items-center gap-1">
          <button
            type="button"
            onClick={doExport}
            title="Export annotations to XFDF"
            aria-label="Export annotations"
            className="rounded px-1.5 py-0.5 text-sm font-normal normal-case hover:bg-neutral-200 dark:hover:bg-neutral-800"
          >
            ⬆
          </button>
          <button
            type="button"
            onClick={doImport}
            title="Import annotations from XFDF"
            aria-label="Import annotations"
            className="rounded px-1.5 py-0.5 text-sm font-normal normal-case hover:bg-neutral-200 dark:hover:bg-neutral-800"
          >
            ⬇
          </button>
          {/* SPEC: P3-ANN-011 — flatten visible markup into the page. */}
          <button
            type="button"
            onClick={() => setConfirmFlatten(true)}
            disabled={flattenable === 0}
            title="Flatten annotations into the page (permanent after save)"
            aria-label="Flatten annotations"
            className="rounded px-1.5 py-0.5 text-sm font-normal normal-case hover:bg-neutral-200 disabled:opacity-30 dark:hover:bg-neutral-800"
          >
            ▦
          </button>
        </div>
      </header>
      {confirmFlatten ? (
        <div className="space-y-1 border-b border-amber-300 bg-amber-50 px-3 py-2 text-xs text-amber-900 dark:border-amber-700/60 dark:bg-amber-950/40 dark:text-amber-200">
          <p>
            Flatten {flattenable} annotation{flattenable === 1 ? "" : "s"} into the page? They
            become part of the content and can't be edited after you save.
          </p>
          <div className="flex justify-end gap-2">
            <button
              type="button"
              onClick={() => setConfirmFlatten(false)}
              className="rounded border border-amber-300 px-2 py-0.5 hover:bg-amber-100 dark:border-amber-700 dark:hover:bg-amber-900/40"
            >
              Cancel
            </button>
            <button
              type="button"
              onClick={doFlatten}
              className="rounded bg-amber-600 px-2 py-0.5 text-white hover:bg-amber-700"
            >
              Flatten
            </button>
          </div>
        </div>
      ) : null}
      {note ? (
        <div className="border-b border-neutral-200 bg-neutral-100 px-3 py-1 text-xs text-neutral-600 dark:border-neutral-800 dark:bg-neutral-900 dark:text-neutral-300">
          {note}
        </div>
      ) : null}

      <div className="space-y-2 border-b border-neutral-200 p-2 dark:border-neutral-800">
        <input
          type="search"
          aria-label="Search annotations"
          placeholder="Search…"
          value={filter.search}
          onChange={(e) => setFilter((f) => ({ ...f, search: e.target.value }))}
          className="w-full rounded border border-neutral-300 bg-transparent px-2 py-1 text-sm dark:border-neutral-600"
        />
        {kinds.length > 0 ? (
          <div className="flex flex-wrap gap-1">
            {kinds.map((kind) => {
              const on = filter.kinds.includes(kind);
              return (
                <button
                  key={kind}
                  type="button"
                  onClick={() => toggleKind(kind)}
                  aria-pressed={on}
                  className={
                    "rounded-full border px-2 py-0.5 text-xs " +
                    (on
                      ? "border-blue-500 bg-blue-100 text-blue-800 dark:bg-blue-900/40 dark:text-blue-200"
                      : "border-neutral-300 text-neutral-600 dark:border-neutral-600 dark:text-neutral-300")
                  }
                >
                  {kindLabel(kind)}
                </button>
              );
            })}
          </div>
        ) : null}
        <div className="flex items-center gap-1">
          <select
            aria-label="Filter by author"
            value={filter.author ?? ""}
            onChange={(e) => setFilter((f) => ({ ...f, author: e.target.value || null }))}
            className="min-w-0 flex-1 rounded border border-neutral-300 bg-transparent px-1 py-0.5 text-xs dark:border-neutral-600"
          >
            <option value="">All authors</option>
            {authors.map((a) => (
              <option key={a} value={a}>
                {a}
              </option>
            ))}
          </select>
          <input
            type="date"
            aria-label="Modified on or after"
            title="Modified on or after"
            value={msToDateInput(filter.modifiedAfter)}
            onChange={(e) =>
              setFilter((f) => ({ ...f, modifiedAfter: dateInputToMs(e.target.value) }))
            }
            className="rounded border border-neutral-300 bg-transparent px-1 py-0.5 text-xs dark:border-neutral-600"
          />
          {filter.modifiedAfter !== null ? (
            <button
              type="button"
              onClick={() => setFilter((f) => ({ ...f, modifiedAfter: null }))}
              aria-label="Clear date filter"
              title="Clear date filter"
              className="shrink-0 rounded px-1.5 py-0.5 text-xs text-neutral-500 hover:bg-neutral-200 hover:text-neutral-800 dark:hover:bg-neutral-800 dark:hover:text-neutral-100"
            >
              ✕
            </button>
          ) : null}
        </div>
      </div>

      <div className="flex-1 overflow-auto p-2 text-sm">
        {list === null ? (
          <div className="text-neutral-500">Loading…</div>
        ) : roots.length === 0 ? (
          <div className="text-neutral-500">No annotations.</div>
        ) : shown === 0 ? (
          <div className="text-neutral-500">No matches.</div>
        ) : (
          <ul className="space-y-2">
            {groups.map((group) => (
              <li key={group.page}>
                <div className="mb-1 text-xs font-medium uppercase tracking-wide text-neutral-400">
                  Page {group.page + 1}
                </div>
                <ul className="space-y-2">
                  {group.items.map((info) => {
                    const replies = repliesByRoot.get(info.id) ?? [];
                    return (
                      <li key={info.id} className="space-y-1">
                        <div className="group flex items-stretch gap-1">
                          <button
                            type="button"
                            onClick={() => {
                              select(info);
                              onJump(info.page + 1);
                            }}
                            aria-label={`${kindLabel(info.kind)} on page ${info.page + 1}`}
                            aria-current={info.id === selectedId}
                            className={
                              "min-w-0 flex-1 rounded px-2 py-1 text-left hover:bg-neutral-100 dark:hover:bg-neutral-900 " +
                              (info.id === selectedId ? "bg-blue-100 dark:bg-blue-900/40" : "")
                            }
                          >
                            <div className="flex items-baseline justify-between gap-2">
                              <span className="text-xs font-medium text-neutral-500">
                                {kindLabel(info.kind)}
                              </span>
                              {info.modified !== null ? (
                                <span className="shrink-0 text-[10px] tabular-nums text-neutral-400">
                                  {new Date(info.modified).toLocaleDateString()}
                                </span>
                              ) : null}
                            </div>
                            <div className="truncate text-neutral-800 dark:text-neutral-200">
                              {info.contents || <span className="italic text-neutral-400">(no text)</span>}
                            </div>
                            {info.author ? (
                              <div className="truncate text-[11px] text-neutral-500">{info.author}</div>
                            ) : null}
                          </button>
                          {info.kind === "freetext" ? (
                            <button
                              type="button"
                              onClick={() => edit(info)}
                              aria-label={`Edit free text on page ${info.page + 1}`}
                              title="Edit text"
                              className={
                                "shrink-0 rounded px-2 text-neutral-400 hover:bg-blue-100 hover:text-blue-600 " +
                                "focus:opacity-100 group-hover:opacity-100 dark:hover:bg-blue-900/40 " +
                                (info.id === selectedId ? "opacity-100" : "opacity-0")
                              }
                            >
                              ✎
                            </button>
                          ) : null}
                          <button
                            type="button"
                            onClick={() => remove(info)}
                            aria-label={`Delete ${kindLabel(info.kind)} on page ${info.page + 1}`}
                            title="Delete annotation"
                            className={
                              "shrink-0 rounded px-2 text-neutral-400 hover:bg-red-100 hover:text-red-600 " +
                              "focus:opacity-100 group-hover:opacity-100 dark:hover:bg-red-900/40 " +
                              (info.id === selectedId ? "opacity-100" : "opacity-0")
                            }
                          >
                            ✕
                          </button>
                        </div>

                        {/* SPEC: P3-ANN-009 — replies, nested under the root. */}
                        {replies.map((r) => (
                          <div
                            key={r.id}
                            className="group ml-3 flex items-stretch gap-1 border-l-2 border-neutral-200 pl-2 dark:border-neutral-700"
                          >
                            <div className="min-w-0 flex-1 px-1 py-0.5">
                              <div className="flex items-baseline justify-between gap-2">
                                <span className="text-[11px] font-medium text-neutral-500">
                                  ↳ {r.author || "Reply"}
                                </span>
                                {r.modified !== null ? (
                                  <span className="shrink-0 text-[10px] tabular-nums text-neutral-400">
                                    {new Date(r.modified).toLocaleDateString()}
                                  </span>
                                ) : null}
                              </div>
                              <div className="whitespace-pre-wrap break-words text-[13px] text-neutral-800 dark:text-neutral-200">
                                {r.contents || <span className="italic text-neutral-400">(no text)</span>}
                              </div>
                            </div>
                            <button
                              type="button"
                              onClick={() => remove(r)}
                              aria-label={`Delete reply on page ${r.page + 1}`}
                              title="Delete reply"
                              className="shrink-0 rounded px-2 text-neutral-400 opacity-0 hover:bg-red-100 hover:text-red-600 focus:opacity-100 group-hover:opacity-100 dark:hover:bg-red-900/40"
                            >
                              ✕
                            </button>
                          </div>
                        ))}

                        {/* SPEC: P3-ANN-009 — reply composer / affordance. */}
                        {replyingTo === info.id ? (
                          <div className="ml-3 flex flex-col gap-1 pl-2">
                            <textarea
                              value={replyText}
                              onChange={(e) => setReplyText(e.target.value)}
                              aria-label="Reply text"
                              placeholder="Write a reply…"
                              rows={2}
                              autoFocus
                              className="w-full rounded border border-neutral-300 bg-transparent px-2 py-1 text-xs dark:border-neutral-600"
                            />
                            <div className="flex gap-1">
                              <button
                                type="button"
                                onClick={() => sendReply(info.id, replyText)}
                                disabled={replyText.trim().length === 0}
                                className="rounded bg-blue-600 px-2 py-0.5 text-xs text-white hover:bg-blue-700 disabled:opacity-40"
                              >
                                Reply
                              </button>
                              <button
                                type="button"
                                onClick={() => {
                                  setReplyingTo(null);
                                  setReplyText("");
                                }}
                                className="rounded px-2 py-0.5 text-xs hover:bg-neutral-100 dark:hover:bg-neutral-800"
                              >
                                Cancel
                              </button>
                            </div>
                          </div>
                        ) : (
                          <button
                            type="button"
                            onClick={() => {
                              setReplyingTo(info.id);
                              setReplyText("");
                            }}
                            aria-label={`Reply to ${kindLabel(info.kind)} on page ${info.page + 1}`}
                            title="Add a reply (threads under this annotation)"
                            className="ml-3 rounded border border-neutral-300 px-2 py-0.5 text-[11px] font-medium text-blue-600 hover:bg-blue-50 dark:border-neutral-600 dark:text-blue-300 dark:hover:bg-blue-950/40"
                          >
                            💬 {replies.length > 0 ? `${replies.length} repl${replies.length === 1 ? "y" : "ies"}` : "Reply"}
                          </button>
                        )}
                      </li>
                    );
                  })}
                </ul>
              </li>
            ))}
          </ul>
        )}
      </div>
    </aside>
  );
}
