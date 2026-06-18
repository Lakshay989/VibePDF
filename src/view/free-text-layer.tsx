// SPEC: P3-ANN-003 (P3.B3a) — the per-page free-text overlay.
//
// Free-text differs from notes: the *committed* box carries a generated `/AP`,
// so the PDF.js canvas draws it (write via IPC → epoch reload → canvas), exactly
// like markup. So this overlay holds NO committed boxes — only the transient
// editor: drag to size a box, type into a positioned <textarea>, commit. On
// commit we persist through the actor and drop the tool; the canvas then renders
// the result. Pointer events, not HTML5 DnD (WKWebView; docs/04).

import { type PointerEvent as ReactPointerEvent, useState } from "react";

import { addFreeText, type FreeTextRect } from "@/ipc/freetext";
import { useEditEpochStore } from "@/state/edit-epoch-store";
import { useHistoryStore } from "@/state/history-store";
import { useToolStore } from "@/state/tool-store";
import { type PageGeometry, type ScreenPoint, screenToPdf } from "@/tools/_framework";
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
  const setHistory = useHistoryStore((s) => s.setHistory);
  const bumpEpoch = useEditEpochStore((s) => s.bumpEpoch);

  const [start, setStart] = useState<ScreenPoint | null>(null);
  const [current, setCurrent] = useState<ScreenPoint | null>(null);
  const [editor, setEditor] = useState<Editor | null>(null);
  const [text, setText] = useState("");

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
    setEditor({ rect, pdfRect });
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
    addFreeText(
      documentId,
      page,
      ed.pdfRect,
      text,
      options.fontFamily,
      options.fontSize,
      options.color,
      options.bold,
      options.italic,
    )
      .then((h) => {
        // The box is now in the PDF; reload so the canvas renders the /AP.
        bumpEpoch(documentId);
        setHistory(documentId, h);
      })
      .catch((err: unknown) => console.warn("add free-text failed", documentId, err));
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
              height: editor.rect.height,
              resize: "none",
              border: "1px solid #2563eb",
              outline: "none",
              padding: "1px 2px",
              color: options.color,
              fontFamily: cssFontFamily(options.fontFamily),
              fontSize: `${options.fontSize * scale}px`,
              fontWeight: options.bold ? 700 : 400,
              fontStyle: options.italic ? "italic" : "normal",
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
