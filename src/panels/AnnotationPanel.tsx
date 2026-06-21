// SPEC: P3-ANN-008 (P3.D1) — the annotation sidebar.
//
// Reads every annotation out of the PDF (read-only, via the actor) and lists
// them grouped by page, with search + filter by type / author / date. Clicking
// an entry scrolls to its page and selects it (a dashed highlight is drawn by
// `SelectionHighlightLayer`). Like the note overlay it re-reads on the edit epoch
// so the list stays in step with edits/undo. No PDF bytes are touched here.

import { useCallback, useEffect, useMemo, useState } from "react";

import { type AnnotationInfo, deleteAnnotation, readAnnotations } from "@/ipc/annotations";
import { readFreeText } from "@/ipc/freetext";
import type { DocumentId } from "@/ipc/pdf";
import { useAnnotationEditStore } from "@/state/annotation-edit-store";
import {
  type AnnotationFilter,
  dateInputToMs,
  distinctAuthors,
  distinctKinds,
  EMPTY_FILTER,
  filterAnnotations,
  groupByPage,
  kindLabel,
  msToDateInput,
} from "@/panels/annotation-filter";
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

  const all = useMemo(() => list ?? [], [list]);
  const authors = useMemo(() => distinctAuthors(all), [all]);
  const kinds = useMemo(() => distinctKinds(all), [all]);
  const groups = useMemo(() => groupByPage(filterAnnotations(all, filter)), [all, filter]);
  const shown = groups.reduce((n, g) => n + g.items.length, 0);

  const toggleKind = (kind: (typeof kinds)[number]) =>
    setFilter((f) => ({
      ...f,
      kinds: f.kinds.includes(kind) ? f.kinds.filter((k) => k !== kind) : [...f.kinds, kind],
    }));

  return (
    <aside className="flex h-full w-72 flex-col border-r border-neutral-200 bg-neutral-50 dark:border-neutral-800 dark:bg-neutral-950">
      <header className="border-b border-neutral-200 px-3 py-2 text-xs font-semibold uppercase tracking-wide text-neutral-500 dark:border-neutral-800">
        Annotations
        {list ? <span className="ml-1 text-neutral-400">({shown})</span> : null}
      </header>

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
        ) : all.length === 0 ? (
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
                <ul className="space-y-0.5">
                  {group.items.map((info) => (
                    <li key={info.id} className="group flex items-stretch gap-1">
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
                    </li>
                  ))}
                </ul>
              </li>
            ))}
          </ul>
        )}
      </div>
    </aside>
  );
}
