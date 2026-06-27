// SPEC: P4-EDIT-001 (P4.B1) — the per-page click-to-edit text overlay.
//
// In edit-text mode this layer fetches the page's text runs (A1) and lays a
// transparent hit-zone over each. Click a run → an inline editor opens at its
// bounding box, prefilled with the run's text and an approximation of its style;
// commit rewrites the run via `replaceTextRun` (A3) on the document actor, then
// the epoch reload re-renders the canvas. No committed state lives here — only the
// transient editor. Pointer events, not HTML5 DnD (WKWebView; docs/04).

import { useEffect, useState } from "react";

import { useDocEpoch, useEditEpochStore } from "@/state/edit-epoch-store";
import { useHistoryStore } from "@/state/history-store";
import { useToolStore } from "@/state/tool-store";
import { extractTextRuns, type TextRun } from "@/ipc/text-runs";
import { replaceTextRun } from "@/ipc/text-edit";
import { type PageGeometry, pdfToScreen } from "@/tools/_framework";
import { cssFamilyForFont } from "@/tools/text-edit/text-edit";

export interface TextEditLayerProps {
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

interface ScreenRect {
  left: number;
  top: number;
  width: number;
  height: number;
}

interface Editor {
  runIndex: number;
  run: TextRun;
  rect: ScreenRect;
}

export function TextEditLayer({
  documentId,
  page,
  displayedWidth,
  displayedHeight,
  scale,
  rotation,
}: TextEditLayerProps) {
  const activeTool = useToolStore((s) => s.activeTool);
  const setHistory = useHistoryStore((s) => s.setHistory);
  const bumpEpoch = useEditEpochStore((s) => s.bumpEpoch);
  const epoch = useDocEpoch(documentId);

  const [runs, setRuns] = useState<TextRun[]>([]);
  const [editor, setEditor] = useState<Editor | null>(null);
  const [text, setText] = useState("");

  const active = activeTool === "edit-text";

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

  // The screen rect for a run's PDF bbox [x0,y0,x1,y1] (origin bottom-left).
  const runRect = (run: TextRun): ScreenRect => {
    const tl = pdfToScreen({ page, x: run.bbox[0], y: run.bbox[3] }, geo);
    const br = pdfToScreen({ page, x: run.bbox[2], y: run.bbox[1] }, geo);
    return {
      left: Math.min(tl.x, br.x),
      top: Math.min(tl.y, br.y),
      width: Math.abs(br.x - tl.x),
      height: Math.abs(br.y - tl.y),
    };
  };

  // Fetch this page's runs while the tool is active. Re-read on every edit epoch
  // so indices track the latest document state. Cheap to skip when idle.
  useEffect(() => {
    if (!active) {
      setRuns([]);
      return;
    }
    let cancelled = false;
    extractTextRuns(documentId, page)
      .then((r) => {
        if (!cancelled) setRuns(r);
      })
      .catch((err: unknown) => console.warn("extract runs failed", documentId, page, err));
    return () => {
      cancelled = true;
    };
  }, [active, documentId, page, epoch]);

  // Leaving edit-text mode closes any open editor.
  useEffect(() => {
    if (!active) {
      setEditor(null);
      setText("");
    }
  }, [active]);

  const openEditor = (runIndex: number, run: TextRun) => {
    setText(run.text);
    setEditor({ runIndex, run, rect: runRect(run) });
  };

  const cancel = () => {
    setEditor(null);
    setText("");
  };

  const commit = () => {
    const ed = editor;
    const body = text;
    cancel();
    if (!ed || body === ed.run.text) return; // no-op edits don't touch the file
    replaceTextRun(documentId, page, ed.runIndex, body)
      .then((h) => {
        // The PDF changed; reload so the canvas renders the rewritten run.
        bumpEpoch(documentId);
        setHistory(documentId, h);
      })
      .catch((err: unknown) => console.warn("replace text run failed", documentId, err));
  };

  if (!active) return null;

  return (
    <div
      className="absolute left-0 top-0"
      style={{ width: cssWidth, height: cssHeight, pointerEvents: "none" }}
    >
      {/* Hit-zones — one per run; each opts back into pointer events under the
          otherwise pass-through layer so scrolling between runs still works. */}
      {!editor
        ? runs.map((run, i) => {
            const rect = runRect(run);
            return (
              <div
                key={`${i}-${run.bbox[0]}-${run.bbox[1]}`}
                className="absolute hover:bg-blue-400/10"
                title="Click to edit this text"
                onClick={() => openEditor(i, run)}
                style={{
                  left: rect.left,
                  top: rect.top,
                  width: rect.width,
                  height: rect.height,
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
            width: Math.max(editor.rect.width, 40),
            pointerEvents: "auto",
          }}
        >
          <input
            autoFocus
            aria-label="Edit text run"
            value={text}
            onChange={(e) => setText(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Escape") cancel();
              if (e.key === "Enter") commit();
            }}
            style={{
              width: "100%",
              height: Math.max(editor.rect.height, editor.run.fontSize * scale * 1.4),
              border: "1px solid #2563eb",
              outline: "none",
              padding: "0 2px",
              color: editor.run.color,
              fontFamily: cssFamilyForFont(editor.run.fontName),
              fontSize: `${editor.run.fontSize * scale}px`,
              lineHeight: 1.2,
              background: "rgba(255,255,255,0.9)",
            }}
          />
          {/* SPEC: P4-EDIT-002 interplay — a per-edit honesty cue; the document
              banner (A2) carries the precise per-font story. */}
          {!editor.run.embedded ? (
            <div className="mt-0.5 rounded bg-amber-50 px-1 text-[10px] text-amber-800">
              font not embedded — may render in a substitute
            </div>
          ) : null}
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
              aria-label="Save text edit"
              className="rounded bg-blue-600 px-2 py-0.5 text-xs font-medium text-white hover:bg-blue-700"
            >
              Save
            </button>
          </div>
        </div>
      ) : null}
    </div>
  );
}
