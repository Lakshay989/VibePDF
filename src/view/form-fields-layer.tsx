// SPEC: P5-FORM-002 (P5.A2) — the per-page form-fill overlay.
//
// In form mode this layer fetches the page's fillable text fields (A2) and lays a
// text input over each, at the field's widget rect. Typing is local; on blur/Enter
// the value is written via `fillTextField` on the document actor. The fill is a
// *soft* epoch bump (no main-view reload/flash); the saved file carries
// `/NeedAppearances`, so other readers render the value.
//
// The input background is deliberately **opaque**. An earlier version of this
// file claimed "our PDF.js view doesn't regenerate field appearances" and used a
// 92%-alpha white — but PDF.js honours `/NeedAppearances` and synthesizes the
// widget appearance from `/V` itself (`pdf.worker.mjs`: `hasAppearance ||=
// _needAppearances && fieldValue != null`). So the canvas *does* paint the value,
// and a translucent input let it ghost through, offset by the HTML padding
// (P5 sweep A1/A6). Covering it is correct: leaving form mode unmounts this layer
// and PDF.js's copy is then the one you want to see.

import { reportError } from "@/app/report-error";
import { type CSSProperties, useEffect, useState } from "react";

import { fillTextField, readTextFields, type FormField } from "@/ipc/forms";
import { useDocEpoch, useEditEpochStore } from "@/state/edit-epoch-store";
import { useFormStore } from "@/state/form-store";
import { useHistoryStore } from "@/state/history-store";
import { type PageGeometry, pdfToScreen } from "@/tools/_framework";

interface ScreenRect {
  left: number;
  top: number;
  width: number;
  height: number;
}

export interface FormFieldsLayerProps {
  documentId: string;
  /** 0-based page index. */
  page: number;
  /** Displayed (rotation-swapped) page size in PDF points. */
  displayedWidth: number;
  displayedHeight: number;
  /** CSS px per point. */
  scale: number;
  /** Page display rotation in degrees. */
  rotation: number;
}

export function FormFieldsLayer({
  documentId,
  page,
  displayedWidth,
  displayedHeight,
  scale,
  rotation,
}: FormFieldsLayerProps) {
  const formMode = useFormStore((s) => s.formMode);
  const setHistory = useHistoryStore((s) => s.setHistory);
  const bumpEpochSoft = useEditEpochStore((s) => s.bumpEpochSoft);
  const setEditing = useFormStore((s) => s.setEditing);
  const epoch = useDocEpoch(documentId);

  const [fields, setFields] = useState<FormField[]>([]);
  // Local edit buffer keyed by field name — the input shows this (optimistic).
  const [values, setValues] = useState<Record<string, string>>({});

  // `coords` wants the UNROTATED PDF dimensions; swap back for 90°/270°.
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

  // Fetch this page's fields while in form mode. Re-read on every edit epoch so
  // an undo/redo (which reverts a value) is reflected.
  useEffect(() => {
    if (!formMode) {
      setFields([]);
      return;
    }
    let cancelled = false;
    readTextFields(documentId, page)
      .then((f) => {
        if (cancelled) return;
        setFields(f);
        setValues(Object.fromEntries(f.map((field) => [field.name, field.value])));
      })
      .catch((err: unknown) => console.warn("read text fields failed", documentId, page, err));
    return () => {
      cancelled = true;
    };
  }, [formMode, documentId, page, epoch]);

  if (!formMode) return null;

  const commit = (field: FormField) => {
    const val = values[field.name] ?? "";
    if (val === field.value) return; // no change → don't touch the file
    fillTextField(documentId, field.name, val)
      .then((h) => {
        setHistory(documentId, h);
        // Soft bump: the input already shows the value; no canvas reload needed.
        bumpEpochSoft(documentId);
      })
      .catch((err: unknown) => reportError("Couldn't fill field", err));
  };

  const fieldStyle = (rect: ScreenRect): CSSProperties => ({
    position: "absolute",
    left: rect.left,
    top: rect.top,
    width: Math.max(rect.width, 24),
    height: Math.max(rect.height, 14),
    border: "1px solid #2563eb",
    background: "#fff",
    padding: "0 2px",
    fontSize: `${Math.min(Math.max(rect.height * 0.7, 9), 16)}px`,
    lineHeight: 1.1,
    boxSizing: "border-box",
    pointerEvents: "auto",
  });

  return (
    <div
      className="absolute left-0 top-0"
      style={{ width: cssWidth, height: cssHeight, pointerEvents: "none" }}
    >
      {fields.map((field, i) => {
        const rect = bboxRect(field.rect);
        const value = values[field.name] ?? "";
        const onChange = (v: string) => setValues((prev) => ({ ...prev, [field.name]: v }));
        if (field.multiline) {
          return (
            <textarea
              key={field.name}
              aria-label={`Form field ${field.name}`}
              title={field.tooltip ?? undefined}
              tabIndex={i + 1}
              value={value}
              maxLength={field.maxLen ?? undefined}
              onChange={(e) => onChange(e.target.value)}
              onFocus={() => setEditing(true)}
              onBlur={() => {
                setEditing(false);
                commit(field);
              }}
              onKeyDown={(e) => {
                if (e.key === "Escape") e.currentTarget.blur();
              }}
              style={fieldStyle(rect)}
            />
          );
        }
        return (
          <input
            key={field.name}
            aria-label={`Form field ${field.name}`}
            title={field.tooltip ?? undefined}
            tabIndex={i + 1}
            value={value}
            maxLength={field.maxLen ?? undefined}
            onChange={(e) => onChange(e.target.value)}
            onFocus={() => setEditing(true)}
            onBlur={() => {
              setEditing(false);
              commit(field);
            }}
            onKeyDown={(e) => {
              if (e.key === "Enter" || e.key === "Escape") e.currentTarget.blur();
            }}
            style={fieldStyle(rect)}
          />
        );
      })}
    </div>
  );
}
