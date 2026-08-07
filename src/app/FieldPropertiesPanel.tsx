// SPEC: P5-FORM-006b/006c (P5.B3) — the form-field properties + tab-order panel.
//
// Shown in form mode: lists the current page's fields in tab order. Selecting one
// opens its properties (name, default, max length, multi-line, required, tooltip);
// ↑/↓ reorder the tab sequence (written on drop via setTabOrder); Delete removes
// the field. Every write is undoable and re-reads the list.

import { reportError } from "@/app/report-error";
import { useEffect, useState } from "react";
import { open as openFileDialog, save as saveFileDialog } from "@tauri-apps/plugin-dialog";

import {
  deleteField,
  exportFormData,
  flattenForm,
  importFormData,
  readFormSummary,
  readPageFields,
  setTabOrder,
  updateFieldProperties,
  type FieldPropertyPatch,
  type FormDataFormat,
  type PageField,
} from "@/ipc/forms";
import { useDocEpoch, useEditEpochStore } from "@/state/edit-epoch-store";
import { useFormStore } from "@/state/form-store";
import { useHistoryStore } from "@/state/history-store";
import { describeImport, formatFromPath } from "@/tools/form-author/import-report";
import { moveDown, moveUp } from "@/tools/form-author/tab-order";

interface Props {
  documentId: string;
  /** 0-based page index whose fields are listed. */
  page: number;
}

export function FieldPropertiesPanel({ documentId, page }: Props) {
  const formMode = useFormStore((s) => s.formMode);
  const setDetected = useFormStore((s) => s.setDetected);
  const setHistory = useHistoryStore((s) => s.setHistory);
  const bumpEpoch = useEditEpochStore((s) => s.bumpEpoch);
  const epoch = useDocEpoch(documentId);

  const [fields, setFields] = useState<PageField[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [name, setName] = useState("");
  const [tooltip, setTooltip] = useState("");
  const [maxLen, setMaxLen] = useState("");
  const [multiline, setMultiline] = useState(false);
  const [required, setRequired] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const [confirmFlatten, setConfirmFlatten] = useState(false);

  useEffect(() => {
    if (!formMode) {
      setFields([]);
      setSelected(null);
      return;
    }
    let cancelled = false;
    readPageFields(documentId, page)
      .then((f) => {
        if (!cancelled) setFields(f);
      })
      .catch((err: unknown) => console.warn("read page fields failed", documentId, page, err));
    return () => {
      cancelled = true;
    };
  }, [formMode, documentId, page, epoch]);

  if (!formMode || fields.length === 0) return null;

  const select = (f: PageField) => {
    setSelected(f.name);
    setName(f.name);
    setTooltip("");
    setMaxLen("");
    setMultiline(false);
    setRequired(false);
  };

  /** Run a write, then refresh history + the field list + the form summary. */
  const run = (work: Promise<{ canUndo: boolean; canRedo: boolean }>, what: string) => {
    work
      .then((h) => {
        setHistory(documentId, h);
        bumpEpoch(documentId);
        return Promise.all([
          readPageFields(documentId, page).then(setFields),
          readFormSummary(documentId).then(setDetected),
        ]);
      })
      .catch((err: unknown) => reportError(what, err));
  };

  const applyProps = () => {
    if (!selected) return;
    const parsed = Number.parseInt(maxLen, 10);
    const patch: FieldPropertyPatch = {
      newName: name.trim(),
      tooltip,
      multiline,
      required,
      maxLen: maxLen.trim() === "" ? null : Number.isFinite(parsed) && parsed > 0 ? parsed : null,
    };
    const target = selected;
    setSelected(name.trim());
    run(updateFieldProperties(documentId, target, patch), "Couldn't update the field");
  };

  const reorder = (next: PageField[]) => {
    setFields(next); // optimistic; the write re-reads
    run(
      setTabOrder(
        documentId,
        page,
        next.map((f) => f.name),
      ),
      "Couldn't set the tab order",
    );
  };

  const remove = (fieldName: string) => {
    if (selected === fieldName) setSelected(null);
    run(deleteField(documentId, fieldName), "Couldn't delete the field");
  };

  // SPEC: P5-FORM-008 (P5.C1) — export the document's form data. Read-only, so
  // it's not an undoable edit: pick a file, write it, report the count.
  const doExport = (format: FormDataFormat) => {
    void (async () => {
      try {
        const path = await saveFileDialog({
          defaultPath: `form-data.${format}`,
          filters: [{ name: `${format.toUpperCase()} form data`, extensions: [format] }],
        });
        if (!path) return; // cancelled
        const n = await exportFormData(documentId, format, path);
        setNote(`Exported ${n} field${n === 1 ? "" : "s"}`);
      } catch (err) {
        reportError("Couldn't export form data", err);
      }
    })();
  };

  // SPEC: P5-FORM-009 (P5.C2) — import fills matching fields by name. The format
  // comes from the chosen file's extension; the report's two "reported, not
  // coerced" lists are surfaced verbatim rather than folded into a count.
  const doImport = () => {
    void (async () => {
      try {
        const path = await openFileDialog({
          multiple: false,
          filters: [{ name: "Form data", extensions: ["fdf", "xfdf", "json", "csv"] }],
        });
        if (typeof path !== "string") return; // cancelled
        const format = formatFromPath(path);
        if (!format) {
          setNote("Unrecognised form-data file — expected .fdf, .xfdf, .json or .csv");
          return;
        }
        const report = await importFormData(documentId, format, path);
        setHistory(documentId, report.history);
        bumpEpoch(documentId);
        await Promise.all([
          readPageFields(documentId, page).then(setFields),
          readFormSummary(documentId).then(setDetected),
        ]);
        setNote(describeImport(report.applied, report.unmatched, report.mismatched));
      } catch (err) {
        reportError("Couldn't import form data", err);
      }
    })();
  };

  // SPEC: P5-FORM-010 (P5.C2) — flatten bakes each field's current appearance
  // into the page and drops the interactivity. Undoable in-session only, so it's
  // gated behind an inline confirm (the same shape as the annotation flatten).
  const doFlatten = () => {
    setConfirmFlatten(false);
    setSelected(null);
    flattenForm(documentId)
      .then((h) => {
        setHistory(documentId, h);
        bumpEpoch(documentId);
        setNote("Form flattened — fields are now page content");
        return Promise.all([
          readPageFields(documentId, page).then(setFields),
          readFormSummary(documentId).then(setDetected),
        ]);
      })
      .catch((err: unknown) => reportError("Couldn't flatten the form", err));
  };

  const input = "w-full rounded border border-neutral-300 px-2 py-1 text-sm dark:border-neutral-700 dark:bg-neutral-900";

  return (
    <aside
      aria-label="Form fields"
      className="w-64 shrink-0 overflow-y-auto border-l border-neutral-200 p-3 text-sm dark:border-neutral-800"
    >
      <h2 className="mb-2 text-xs font-medium uppercase tracking-wide text-neutral-400">
        Form fields (tab order)
      </h2>
      {/* SPEC: P5-FORM-008 — export the whole document's data. */}
      <div className="mb-3 flex flex-wrap items-center gap-1">
        <span className="text-xs text-neutral-400">Export</span>
        {(["fdf", "xfdf", "json", "csv"] as const).map((f) => (
          <button
            key={f}
            type="button"
            onClick={() => doExport(f)}
            aria-label={`Export form data as ${f.toUpperCase()}`}
            className="rounded border border-neutral-300 px-1.5 py-0.5 text-xs hover:bg-neutral-100 dark:border-neutral-700 dark:hover:bg-neutral-800"
          >
            {f.toUpperCase()}
          </button>
        ))}
      </div>
      {/* SPEC: P5-FORM-009 / P5-FORM-010 — import data, or bake the form flat. */}
      <div className="mb-3 flex flex-wrap items-center gap-1">
        <button
          type="button"
          onClick={doImport}
          aria-label="Import form data"
          className="rounded border border-neutral-300 px-1.5 py-0.5 text-xs hover:bg-neutral-100 dark:border-neutral-700 dark:hover:bg-neutral-800"
        >
          Import…
        </button>
        {confirmFlatten ? (
          <>
            <span className="text-xs text-neutral-500">Flatten? Not undoable once saved.</span>
            <button
              type="button"
              onClick={doFlatten}
              aria-label="Confirm flatten form"
              className="rounded bg-red-600 px-1.5 py-0.5 text-xs text-white hover:bg-red-700"
            >
              Flatten
            </button>
            <button
              type="button"
              onClick={() => setConfirmFlatten(false)}
              aria-label="Cancel flatten form"
              className="rounded border border-neutral-300 px-1.5 py-0.5 text-xs hover:bg-neutral-100 dark:border-neutral-700 dark:hover:bg-neutral-800"
            >
              Cancel
            </button>
          </>
        ) : (
          <button
            type="button"
            onClick={() => setConfirmFlatten(true)}
            aria-label="Flatten form"
            className="rounded border border-neutral-300 px-1.5 py-0.5 text-xs hover:bg-neutral-100 dark:border-neutral-700 dark:hover:bg-neutral-800"
          >
            Flatten form
          </button>
        )}
      </div>
      {note ? <p className="mb-2 text-xs text-neutral-500">{note}</p> : null}
      <ol className="flex flex-col gap-1">
        {fields.map((f, i) => (
          <li key={f.name} className="flex items-center gap-1">
            <button
              type="button"
              onClick={() => select(f)}
              aria-label={`Field ${f.name}`}
              aria-pressed={selected === f.name}
              className={
                "min-w-0 flex-1 truncate rounded px-2 py-1 text-left " +
                (selected === f.name
                  ? "bg-blue-100 dark:bg-blue-300/20"
                  : "hover:bg-neutral-100 dark:hover:bg-neutral-900")
              }
              title={`${f.name} (${f.kind})`}
            >
              <span>{f.name}</span>
              <span className="ml-1 text-xs text-neutral-400">{f.kind}</span>
            </button>
            <button
              type="button"
              onClick={() => reorder(moveUp(fields, i))}
              disabled={i === 0}
              aria-label={`Move ${f.name} earlier`}
              className="rounded px-1 text-xs hover:bg-neutral-100 disabled:opacity-30 dark:hover:bg-neutral-800"
            >
              ↑
            </button>
            <button
              type="button"
              onClick={() => reorder(moveDown(fields, i))}
              disabled={i === fields.length - 1}
              aria-label={`Move ${f.name} later`}
              className="rounded px-1 text-xs hover:bg-neutral-100 disabled:opacity-30 dark:hover:bg-neutral-800"
            >
              ↓
            </button>
          </li>
        ))}
      </ol>

      {selected ? (
        <div className="mt-4 flex flex-col gap-2 border-t border-neutral-200 pt-3 dark:border-neutral-800">
          <label className="flex flex-col gap-0.5">
            <span className="text-xs text-neutral-500">Name</span>
            <input aria-label="Field name" className={input} value={name} onChange={(e) => setName(e.target.value)} />
          </label>
          <label className="flex flex-col gap-0.5">
            <span className="text-xs text-neutral-500">Tooltip</span>
            <input aria-label="Tooltip" className={input} value={tooltip} onChange={(e) => setTooltip(e.target.value)} />
          </label>
          <label className="flex flex-col gap-0.5">
            <span className="text-xs text-neutral-500">Max length (blank = none)</span>
            <input
              aria-label="Max length"
              type="number"
              min={1}
              className={input}
              value={maxLen}
              onChange={(e) => setMaxLen(e.target.value)}
            />
          </label>
          <label className="flex items-center gap-2">
            <input type="checkbox" checked={multiline} onChange={(e) => setMultiline(e.target.checked)} />
            <span>Multi-line</span>
          </label>
          <label className="flex items-center gap-2">
            <input type="checkbox" checked={required} onChange={(e) => setRequired(e.target.checked)} />
            <span>Required</span>
          </label>
          <div className="mt-1 flex items-center justify-between">
            <button
              type="button"
              onClick={() => remove(selected)}
              aria-label={`Delete field ${selected}`}
              className="rounded bg-red-100 px-2 py-1 text-xs text-red-700 hover:bg-red-200"
            >
              Delete
            </button>
            <button
              type="button"
              onClick={applyProps}
              disabled={name.trim() === ""}
              className="rounded bg-blue-600 px-2 py-1 text-xs text-white disabled:opacity-40"
            >
              Apply
            </button>
          </div>
        </div>
      ) : null}
    </aside>
  );
}
