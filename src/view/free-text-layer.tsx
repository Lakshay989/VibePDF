// SPEC: P3-ANN-003 (P3.B3a) — the per-page free-text overlay.
//
// Free-text differs from notes: the *committed* box carries a generated `/AP`,
// so the PDF.js canvas draws it (write via IPC → epoch reload → canvas), exactly
// like markup. So this overlay holds NO committed boxes — only the transient
// editor: drag to size a box, type into a positioned <textarea>, commit. On
// commit we persist through the actor and drop the tool; the canvas then renders
// the result. Pointer events, not HTML5 DnD (WKWebView; docs/04).

import { reportError } from "@/app/report-error";
import { type PointerEvent as ReactPointerEvent, useEffect, useState } from "react";

import { readAnnotations } from "@/ipc/annotations";
import { addFreeText, type FreeTextRect, readFreeText, updateFreeText } from "@/ipc/freetext";
import { useAnnotationEditStore } from "@/state/annotation-edit-store";
import { useDocEpoch, useEditEpochStore } from "@/state/edit-epoch-store";
import { useHistoryStore } from "@/state/history-store";
import { useToolStore } from "@/state/tool-store";
import { type PageGeometry, pdfToScreen, type ScreenPoint, screenToPdf } from "@/tools/_framework";
import {
  cssFontFamily,
  normalizeScreenRect,
  type ScreenRect,
  withDefaultSize,
} from "@/tools/free-text/free-text";

export interface FreeTextLayerProps {
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

interface Editor {
  rect: ScreenRect;
  pdfRect: FreeTextRect;
  /** Set when re-editing an existing box (its `/NM`); null for a new box. */
  editNm: string | null;
}

export function FreeTextLayer({
  documentId,
  page,
  displayedWidth,
  displayedHeight,
  scale,
  rotation,
}: FreeTextLayerProps) {
  const activeTool = useToolStore((s) => s.activeTool);
  const options = useToolStore((s) => s.options);
  const setActiveTool = useToolStore((s) => s.setActiveTool);
  const setOptions = useToolStore((s) => s.setOptions);
  const setHistory = useHistoryStore((s) => s.setHistory);
  const bumpEpoch = useEditEpochStore((s) => s.bumpEpoch);
  const editRequest = useAnnotationEditStore((s) => s.editing);
  const clearEdit = useAnnotationEditStore((s) => s.clearEdit);
  const requestEdit = useAnnotationEditStore((s) => s.requestEdit);
  const epoch = useDocEpoch(documentId);

  const [start, setStart] = useState<ScreenPoint | null>(null);
  const [current, setCurrent] = useState<ScreenPoint | null>(null);
  const [editor, setEditor] = useState<Editor | null>(null);
  const [text, setText] = useState("");
  // SPEC: P3-ANN-013 (P3.B3b) — this page's free-text boxes, for double-click
  // re-edit hit-testing. Re-read on every edit epoch so it tracks add/update/undo.
  const [boxes, setBoxes] = useState<{ nm: string; rect: FreeTextRect }[]>([]);

  const placing = activeTool === "free-text";

  // `coords` wants the UNROTATED PDF dimensions; swap back for 90°/270°.
  const swapped = (((rotation % 180) + 180) % 180) === 90;
  const geo: PageGeometry = {
    page,
    width: swapped ? displayedHeight : displayedWidth,
    height: swapped ? displayedWidth : displayedHeight,
    scale,
    rotation,
  };
  const cssWidth = displayedWidth * scale;
  const cssHeight = displayedHeight * scale;

  const layerPoint = (e: ReactPointerEvent<Element>): ScreenPoint => {
    const r = e.currentTarget.getBoundingClientRect();
    return { x: e.clientX - r.left, y: e.clientY - r.top };
  };

  const onPointerDown = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (!placing || editor) return; // don't start a new box while editing
    e.currentTarget.setPointerCapture?.(e.pointerId);
    const p = layerPoint(e);
    setStart(p);
    setCurrent(p);
  };

  const onPointerMove = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (!start || editor) return;
    setCurrent(layerPoint(e));
  };

  const onPointerUp = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (!start || editor) return;
    const end = layerPoint(e);
    const rect = withDefaultSize(normalizeScreenRect(start, end));
    // PDF rect from the screen box corners; min/max sorts for any rotation.
    const a = screenToPdf({ x: rect.left, y: rect.top }, geo);
    const b = screenToPdf({ x: rect.left + rect.width, y: rect.top + rect.height }, geo);
    const pdfRect: FreeTextRect = [
      Math.min(a.x, b.x),
      Math.min(a.y, b.y),
      Math.max(a.x, b.x),
      Math.max(a.y, b.y),
    ];
    setStart(null);
    setCurrent(null);
    setText("");
    setEditor({ rect, pdfRect, editNm: null });
  };

  // SPEC: P3-ANN-013 — claim an edit request for this page: open the editor at
  // the box's location, pre-filled with its text + style.
  useEffect(() => {
    if (!editRequest || editRequest.page !== page) return;
    const { data, nm } = editRequest;
    const swp = (((rotation % 180) + 180) % 180) === 90;
    const g: PageGeometry = {
      page,
      width: swp ? displayedHeight : displayedWidth,
      height: swp ? displayedWidth : displayedHeight,
      scale,
      rotation,
    };
    const tl = pdfToScreen({ page, x: data.rect[0], y: data.rect[3] }, g);
    const br = pdfToScreen({ page, x: data.rect[2], y: data.rect[1] }, g);
    setText(data.text);
    setOptions({
      fontFamily: data.fontFamily,
      fontSize: data.fontSize,
      color: data.color,
      bold: data.bold,
      italic: data.italic,
      underline: data.underline,
    });
    setEditor({
      rect: {
        left: Math.min(tl.x, br.x),
        top: Math.min(tl.y, br.y),
        width: Math.abs(br.x - tl.x),
        height: Math.abs(br.y - tl.y),
      },
      pdfRect: data.rect,
      editNm: nm,
    });
    clearEdit();
  }, [editRequest, page, scale, rotation, displayedWidth, displayedHeight, setOptions, clearEdit]);

  // SPEC: P3-ANN-013 (P3.B3b) — track this page's free-text boxes for double-click.
  useEffect(() => {
    let cancelled = false;
    readAnnotations(documentId)
      .then((rows) => {
        if (cancelled) return;
        setBoxes(
          rows
            .filter((a) => a.kind === "freetext" && a.page === page)
            .map((a) => ({ nm: a.id, rect: a.rect as FreeTextRect })),
        );
      })
      .catch((err: unknown) => console.warn("read free-text boxes failed", documentId, err));
    return () => {
      cancelled = true;
    };
  }, [documentId, page, epoch]);

  // SPEC: P3-ANN-013 (P3.B3b) — double-click a committed box → open it pre-filled
  // (the same edit request the sidebar ✎ posts).
  const reEdit = (nm: string) => {
    readFreeText(documentId, nm)
      .then((data) => {
        if (data) requestEdit({ nm, page, data });
      })
      .catch((err: unknown) => console.warn("read free-text failed", documentId, err));
  };

  const cancel = () => {
    setEditor(null);
    setText("");
    setActiveTool(null);
  };

  const commit = () => {
    const body = text.trim();
    const ed = editor;
    cancel();
    if (!ed || !body) return;
    const done = (h: Parameters<typeof setHistory>[1]) => {
      // The PDF changed; reload so the canvas renders the new /AP.
      bumpEpoch(documentId);
      setHistory(documentId, h);
    };
    const fail = (err: unknown) => reportError("Couldn't save the text", err);
    const promise = ed.editNm
      ? updateFreeText(
          documentId,
          ed.editNm,
          text,
          options.fontFamily,
          options.fontSize,
          options.color,
          options.bold,
          options.italic,
          options.underline,
        )
      : addFreeText(
          documentId,
          page,
          ed.pdfRect,
          text,
          options.fontFamily,
          options.fontSize,
          options.color,
          options.bold,
          options.italic,
          options.underline,
        );
    promise.then(done).catch(fail);
  };

  const preview = start && current ? normalizeScreenRect(start, current) : null;

  return (
    <div
      className="absolute left-0 top-0"
      style={{
        width: cssWidth,
        height: cssHeight,
        pointerEvents: placing ? "auto" : "none",
        cursor: placing && !editor ? "crosshair" : undefined,
      }}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
    >
      {preview ? (
        <div
          className="absolute border border-dashed border-blue-500"
          style={{ left: preview.left, top: preview.top, width: preview.width, height: preview.height }}
        />
      ) : null}

      {/* SPEC: P3-ANN-013 (P3.B3b) — double-click a committed box to re-edit. Only
          when idle (no tool, no open editor); each zone opts back into pointer
          events under the otherwise pass-through layer. */}
      {!placing && !editor
        ? boxes.map((b) => {
            const tl = pdfToScreen({ page, x: b.rect[0], y: b.rect[3] }, geo);
            const br = pdfToScreen({ page, x: b.rect[2], y: b.rect[1] }, geo);
            return (
              <div
                key={b.nm}
                className="absolute"
                title="Double-click to edit"
                onDoubleClick={() => reEdit(b.nm)}
                style={{
                  left: Math.min(tl.x, br.x),
                  top: Math.min(tl.y, br.y),
                  width: Math.abs(br.x - tl.x),
                  height: Math.abs(br.y - tl.y),
                  pointerEvents: "auto",
                  cursor: "text",
                }}
              />
            );
          })
        : null}

      {editor ? (
        <div
          className="absolute flex flex-col"
          onPointerDown={(e) => e.stopPropagation()}
          style={{
            left: editor.rect.left,
            top: editor.rect.top,
            width: editor.rect.width,
            minHeight: editor.rect.height,
            pointerEvents: "auto",
          }}
        >
          <textarea
            autoFocus
            aria-label="Free-text annotation"
            value={text}
            onChange={(e) => setText(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Escape") cancel();
              if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) commit();
            }}
            placeholder="Type text…"
            style={{
              width: "100%",
              // At least ~1.4 line-heights tall so a large font is visible while
              // typing (the committed box grows to fit on the backend too).
              height: Math.max(editor.rect.height, options.fontSize * scale * 1.4),
              resize: "none",
              border: "1px solid #2563eb",
              outline: "none",
              padding: "1px 2px",
              color: options.color,
              fontFamily: cssFontFamily(options.fontFamily),
              fontSize: `${options.fontSize * scale}px`,
              fontWeight: options.bold ? 700 : 400,
              fontStyle: options.italic ? "italic" : "normal",
              textDecoration: options.underline ? "underline" : "none",
              lineHeight: 1.2,
              background: "rgba(255,255,255,0.85)",
            }}
          />
          <div className="mt-0.5 flex justify-end gap-1">
            <button
              type="button"
              onClick={cancel}
              className="rounded bg-neutral-200 px-2 py-0.5 text-xs hover:bg-neutral-300"
            >
              Cancel
            </button>
            <button
              type="button"
              onClick={commit}
              aria-label="Add free text"
              className="rounded bg-blue-600 px-2 py-0.5 text-xs font-medium text-white hover:bg-blue-700"
            >
              Add
            </button>
          </div>
        </div>
      ) : null}
    </div>
  );
}
