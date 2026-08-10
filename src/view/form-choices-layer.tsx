// SPEC: P5-FORM-004 (P5.A4) — the per-page choice-field overlay.
//
// In form mode this lays a native <select> over each combo/list field, offering
// the field's /Opt options and pre-selecting its /V. Changing the selection writes
// via setChoiceField on the document actor. The <select> is the in-app display
// (our PDF.js view doesn't regenerate field appearances), so the write is a *soft*
// epoch bump — no canvas flash; the saved file's /V + /NeedAppearances render in
// other readers.

import { reportError } from "@/app/report-error";
import { type CSSProperties, useEffect, useState } from "react";

import { readChoiceFields, setChoiceField, type ChoiceField } from "@/ipc/forms";
import { useDocEpoch, useEditEpochStore } from "@/state/edit-epoch-store";
import { useFormStore } from "@/state/form-store";
import { useHistoryStore } from "@/state/history-store";
import { type PageGeometry, pdfToScreen } from "@/tools/_framework";

/** Roughly one option row in CSS px — how many rows a list box shows. */
const ROW_PX = 16;

interface ScreenRect {
  left: number;
  top: number;
  width: number;
  height: number;
}

export interface FormChoicesLayerProps {
  documentId: string;
  /** 0-based page index. */
  page: number;
  displayedWidth: number;
  displayedHeight: number;
  scale: number;
  rotation: number;
}

export function FormChoicesLayer({
  documentId,
  page,
  displayedWidth,
  displayedHeight,
  scale,
  rotation,
}: FormChoicesLayerProps) {
  const formMode = useFormStore((s) => s.formMode);
  const setHistory = useHistoryStore((s) => s.setHistory);
  const bumpEpochSoft = useEditEpochStore((s) => s.bumpEpochSoft);
  const epoch = useDocEpoch(documentId);

  const [fields, setFields] = useState<ChoiceField[]>([]);
  // Optimistic selection buffer keyed by field name.
  const [values, setValues] = useState<Record<string, string[]>>({});

  const swapped = ((rotation % 180) + 180) % 180 === 90;
  const geo: PageGeometry = {
    page,
    width: swapped ? displayedHeight : displayedWidth,
    height: swapped ? displayedWidth : displayedHeight,
    scale,
    rotation,
  };
  const cssWidth = displayedWidth * scale;
  const cssHeight = displayedHeight * scale;

  const bboxRect = (bbox: readonly [number, number, number, number]): ScreenRect => {
    const tl = pdfToScreen({ page, x: bbox[0], y: bbox[3] }, geo);
    const br = pdfToScreen({ page, x: bbox[2], y: bbox[1] }, geo);
    return {
      left: Math.min(tl.x, br.x),
      top: Math.min(tl.y, br.y),
      width: Math.abs(br.x - tl.x),
      height: Math.abs(br.y - tl.y),
    };
  };

  useEffect(() => {
    if (!formMode) {
      setFields([]);
      return;
    }
    let cancelled = false;
    readChoiceFields(documentId, page)
      .then((f) => {
        if (cancelled) return;
        setFields(f);
        setValues(Object.fromEntries(f.map((field) => [field.name, field.selected])));
      })
      .catch((err: unknown) => console.warn("read choice fields failed", documentId, page, err));
    return () => {
      cancelled = true;
    };
  }, [formMode, documentId, page, epoch]);

  if (!formMode) return null;

  const commit = (field: ChoiceField, next: string[]) => {
    setValues((prev) => ({ ...prev, [field.name]: next }));
    setChoiceField(documentId, field.name, next)
      .then((h) => {
        setHistory(documentId, h);
        bumpEpochSoft(documentId);
      })
      .catch((err: unknown) => reportError("Couldn't set field", err));
  };

  const selectStyle = (rect: ScreenRect): CSSProperties => ({
    position: "absolute",
    left: rect.left,
    top: rect.top,
    width: Math.max(rect.width, 40),
    height: Math.max(rect.height, 16),
    border: "1px solid #2563eb",
    background: "#fff",
    fontSize: `${Math.min(Math.max(rect.height * 0.5, 9), 14)}px`,
    boxSizing: "border-box",
    pointerEvents: "auto",
  });

  return (
    <div
      className="absolute left-0 top-0"
      style={{ width: cssWidth, height: cssHeight, pointerEvents: "none" }}
    >
      {fields.map((field) => {
        const rect = bboxRect(field.rect);
        const selected = values[field.name] ?? field.selected;
        // `multiple` and `size` are independent — in HTML and in the PDF flags.
        // `/Ff` bit 18 (combo) decides dropdown-vs-list; bit 22 (multi-select)
        // decides one-vs-many. This overlay keyed BOTH off `multi`, so a list
        // box that happened not to be multi-select rendered as a dropdown.
        const rows =
          field.kind === "list"
            ? Math.max(2, Math.min(field.options.length, Math.floor(rect.height / ROW_PX)))
            : undefined;
        return (
          <select
            key={field.name}
            aria-label={`Choice field ${field.name}`}
            multiple={field.multi}
            size={rows}
            title={
              field.tooltip ??
              (field.multi ? "Hold \u2318 (Ctrl on Windows) to select more than one" : undefined)
            }
            value={field.multi ? selected : (selected[0] ?? "")}
            onChange={(e) => {
              const next = field.multi
                ? Array.from(e.target.selectedOptions, (o) => o.value)
                : [e.target.value];
              // The single-select placeholder is not a value: picking it would
              // commit `[""]`, which `set_choice_field` rejects as "not an option"
              // (P5 sweep B2). `disabled` blocks it in most browsers; this is the
              // belt-and-braces half.
              if (!field.multi && next[0] === "") return;
              commit(field, next);
            }}
            style={selectStyle(rect)}
          >
            {field.kind === "combo" ? (
              <option value="" disabled>
                — select —
              </option>
            ) : null}
            {/* Keyed by position, not export value. A PDF we did not write can
                carry duplicate `/Opt` entries, and duplicate keys make React
                drop options. (Selecting one duplicate still highlights all of
                them — a `<select>` selects by value, so identical values are
                indistinguishable. That is inherent to the format; the fix is to
                stop *creating* duplicates, below.) */}
            {field.options.map((opt, i) => (
              <option key={`${i}:${opt.export}`} value={opt.export}>
                {opt.label}
              </option>
            ))}
          </select>
        );
      })}
    </div>
  );
}
