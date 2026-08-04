// SPEC: P4-EDIT-001 (P4.B1) — the per-page click-to-edit text overlay.
//
// In edit-text mode this layer fetches the page's text runs (A1) and lays a
// transparent hit-zone over each. Click a run → an inline editor opens at its
// bounding box, prefilled with the run's text and an approximation of its style;
// commit rewrites the run via `replaceTextRun` (A3) on the document actor, then
// the epoch reload re-renders the canvas. No committed state lives here — only the
// transient editor. Pointer events, not HTML5 DnD (WKWebView; docs/04).

import { reportError } from "@/app/report-error";
import { useEffect, useRef, useState } from "react";

import { useDocEpoch, useEditEpochStore } from "@/state/edit-epoch-store";
import { useHistoryStore } from "@/state/history-store";
import { useOptimisticEditStore, usePendingEdits } from "@/state/optimistic-edit-store";
import { useToastStore } from "@/state/toast-store";
import { useToolStore } from "@/state/tool-store";
import { extractTextRuns, type TextRun } from "@/ipc/text-runs";
import { deleteTextRun, replaceTextRun } from "@/ipc/text-edit";
import { type PageGeometry, pdfToScreen } from "@/tools/_framework";
import { cssFamilyForFont } from "@/tools/text-edit/text-edit";

/** Optimistic-preview payload: a committed text edit awaiting the baked reload.
 *  The old run is covered and the new text drawn over it, in PDF-space so it
 *  survives zoom/scroll (P4.HF29 pattern). `text` empty ⇒ a delete (cover only). */
interface TextEditHeld {
  bbox: [number, number, number, number];
  text: string;
  color: string;
  fontName: string;
  fontSize: number;
}

/** Don't stack a hint on every stray click. */
const MISS_TOAST_THROTTLE_MS = 2000;

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
  const pushToast = useToastStore((s) => s.push);
  const epoch = useDocEpoch(documentId);
  const pendingEdits = usePendingEdits<TextEditHeld>(documentId, page, "text-edit");

  const [runs, setRuns] = useState<TextRun[]>([]);
  const [editor, setEditor] = useState<Editor | null>(null);
  const [text, setText] = useState("");
  const lastMissToastRef = useRef(0);

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

  // The screen rect for a PDF bbox [x0,y0,x1,y1] (origin bottom-left).
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
  const runRect = (run: TextRun): ScreenRect => bboxRect(run.bbox);

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

  // Hold the edit optimistically (cover the old run, draw `body` over it) so it
  // appears instantly, then tie it to the reload epoch that bakes it. `body` empty
  // ⇒ a delete (cover only). Returns the store key so `.catch` can drop it.
  const holdEdit = (run: TextRun, body: string): { key: string; oe: ReturnType<typeof useOptimisticEditStore.getState> } => {
    const oe = useOptimisticEditStore.getState();
    const key = oe.add(documentId, page, "text-edit", {
      bbox: [run.bbox[0], run.bbox[1], run.bbox[2], run.bbox[3]],
      text: body,
      color: run.color,
      fontName: run.fontName,
      fontSize: run.fontSize,
    } satisfies TextEditHeld);
    return { key, oe };
  };
  const tie = (oe: ReturnType<typeof useOptimisticEditStore.getState>, key: string) => {
    // Text edits are "hard" (they need a bake — the white cover is only an
    // approximation), so bump the bake epoch and tie the overlay to it.
    oe.tie(documentId, key, bumpEpoch(documentId));
  };

  const commit = () => {
    const ed = editor;
    const body = text;
    cancel();
    if (!ed) return;
    if (body === ed.run.text) return; // no-op edits don't touch the file
    const { key, oe } = holdEdit(ed.run, body.trim() === "" ? "" : body);
    // SPEC: P4-EDIT-004 — clearing all the text deletes the run (PDFium's
    // set_text("") would silently revert to the original), matching the Delete button.
    const op =
      body.trim() === ""
        ? deleteTextRun(documentId, page, ed.runIndex)
        : replaceTextRun(documentId, page, ed.runIndex, body);
    op
      .then((h) => {
        tie(oe, key);
        setHistory(documentId, h);
      })
      .catch((err: unknown) => {
        oe.remove(documentId, key);
        reportError(body.trim() === "" ? "Couldn't delete text" : "Couldn't edit text", err);
      });
  };

  // SPEC: P4-EDIT-004 (P4.B3) — remove the run from the content stream entirely.
  const remove = () => {
    const ed = editor;
    cancel();
    if (!ed) return;
    const { key, oe } = holdEdit(ed.run, "");
    deleteTextRun(documentId, page, ed.runIndex)
      .then((h) => {
        tie(oe, key);
        setHistory(documentId, h);
      })
      .catch((err: unknown) => {
        oe.remove(documentId, key);
        reportError("Couldn't delete text", err);
      });
  };

  // A click in edit-text mode that lands on no run — the text there isn't a
  // decodable run: glyphs flattened to vector outlines (paths), a font without a
  // Unicode map, or rasterized text in an image. Say so, rather than silently
  // doing nothing. Throttled so clicks don't stack hints.
  const onMiss = () => {
    const now = Date.now();
    if (now - lastMissToastRef.current < MISS_TOAST_THROTTLE_MS) return;
    lastMissToastRef.current = now;
    pushToast("info", "No editable text here — it may be vector outlines, use a font without a Unicode map, or be part of an image.");
  };

  if (!active) return null;

  return (
    <div
      className="absolute left-0 top-0"
      style={{ width: cssWidth, height: cssHeight, pointerEvents: "none" }}
    >
      {/* Optimistic preview: cover each committed run and draw its new text over
          it, so the edit shows instantly instead of after the bake reload (HF29).
          White cover assumes a light page; the baked reload corrects it. */}
      {pendingEdits.map(({ key, data }) => {
        const rect = bboxRect(data.bbox);
        return (
          <div
            key={key}
            className="absolute"
            style={{
              left: rect.left,
              top: rect.top,
              minWidth: rect.width,
              height: rect.height,
              background: "#ffffff",
              color: data.color,
              fontFamily: cssFamilyForFont(data.fontName),
              fontSize: `${data.fontSize * scale}px`,
              lineHeight: 1,
              whiteSpace: "pre",
              overflow: "visible",
              pointerEvents: "none",
            }}
          >
            {data.text}
          </div>
        );
      })}

      {/* Full-page catch layer under the hit-zones: a click that misses every run
          lands here and explains why nothing is editable. */}
      {!editor ? (
        <div className="absolute inset-0" style={{ pointerEvents: "auto" }} onClick={onMiss} />
      ) : null}

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
              onClick={remove}
              aria-label="Delete text run"
              className="mr-auto rounded bg-red-100 px-2 py-0.5 text-xs text-red-700 hover:bg-red-200"
            >
              Delete
            </button>
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
