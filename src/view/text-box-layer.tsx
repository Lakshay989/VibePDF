// SPEC: P4-EDIT-003 / P4-EDIT-003b (P4.B2) — the per-page "add text box" overlay.
//
// Unlike free-text (which writes a /FreeText annotation), this commits the text
// to the page **content stream** via `addTextBox`. The committed text carries a
// `/VibePDF` marked-content tag holding its source text + style, so — unlike
// ordinary page text — it can be re-opened and edited *as a unit*. Re-edit is
// unified under the **Edit Text** tool: click a box to reload it pre-filled and
// re-commit via `updateTextBox` (clearing it deletes via `deleteTextBox`). These
// box zones sit above Edit Text's per-run zones, so foreign text still edits
// per-run (P4-EDIT-003b). Drag to size a new box, type, commit. Pointer events,
// not HTML5 DnD (WKWebView).

import { reportError } from "@/app/report-error";
import { type PointerEvent as ReactPointerEvent, useEffect, useState } from "react";

import {
  addTextBox,
  deleteTextBox,
  readTextBoxes,
  type TextBoxInfo,
  type TextBoxRect,
  updateTextBox,
} from "@/ipc/text-box";
import { useDocEpoch, useEditEpochStore } from "@/state/edit-epoch-store";
import { useHistoryStore } from "@/state/history-store";
import { useOptimisticEditStore, usePendingEdits } from "@/state/optimistic-edit-store";
import { useToolStore } from "@/state/tool-store";
import type { FontFamily } from "@/tools/_framework";
import { type PageGeometry, pdfToScreen, type ScreenPoint, screenToPdf } from "@/tools/_framework";
import { normalizeScreenRect, type ScreenRect, withDefaultSize } from "@/tools/_framework";
import { cssFontFamily, FONT_FAMILIES } from "@/tools/free-text/free-text";

export interface TextBoxLayerProps {
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
  pdfRect: TextBoxRect;
  /** Set when re-editing an existing box (its `/Id`); null for a new box. */
  editId: string | null;
}

/** Optimistic-preview payload: a committed new text box awaiting bake. */
interface TextBoxHeld {
  rect: TextBoxRect;
  text: string;
  fontFamily: FontFamily;
  fontSize: number;
  color: string;
  bold: boolean;
  italic: boolean;
  underline: boolean;
}

export function TextBoxLayer({
  documentId,
  page,
  displayedWidth,
  displayedHeight,
  scale,
  rotation,
}: TextBoxLayerProps) {
  const activeTool = useToolStore((s) => s.activeTool);
  const options = useToolStore((s) => s.options);
  const setActiveTool = useToolStore((s) => s.setActiveTool);
  const setOptions = useToolStore((s) => s.setOptions);
  const setHistory = useHistoryStore((s) => s.setHistory);
  const bumpEpoch = useEditEpochStore((s) => s.bumpEpoch);
  const epoch = useDocEpoch(documentId);

  const [start, setStart] = useState<ScreenPoint | null>(null);
  const [current, setCurrent] = useState<ScreenPoint | null>(null);
  const [editor, setEditor] = useState<Editor | null>(null);
  const [text, setText] = useState("");
  // SPEC: P4-EDIT-003b — this page's re-editable text boxes, for double-click
  // hit-testing. Re-read on every edit epoch so it tracks add/update/delete/undo.
  const [boxes, setBoxes] = useState<TextBoxInfo[]>([]);

  const placing = activeTool === "add-text";
  // Text has its own colour (black default), separate from the markup colour.
  const textColor = options.textColor ?? "#000000";

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

  const layerPoint = (e: ReactPointerEvent<Element>): ScreenPoint => {
    const r = e.currentTarget.getBoundingClientRect();
    return { x: e.clientX - r.left, y: e.clientY - r.top };
  };

  const onPointerDown = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (!placing || editor) return;
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
    const rect = withDefaultSize(normalizeScreenRect(start, layerPoint(e)));
    const a = screenToPdf({ x: rect.left, y: rect.top }, geo);
    const b = screenToPdf({ x: rect.left + rect.width, y: rect.top + rect.height }, geo);
    const pdfRect: TextBoxRect = [
      Math.min(a.x, b.x),
      Math.min(a.y, b.y),
      Math.max(a.x, b.x),
      Math.max(a.y, b.y),
    ];
    setStart(null);
    setCurrent(null);
    setText("");
    setEditor({ rect, pdfRect, editId: null });
  };

  // SPEC: P4-EDIT-003b — track this page's re-editable boxes for double-click.
  useEffect(() => {
    let cancelled = false;
    readTextBoxes(documentId, page)
      .then((rows) => {
        if (!cancelled) setBoxes(rows);
      })
      .catch((err: unknown) => console.warn("read text boxes failed", documentId, err));
    return () => {
      cancelled = true;
    };
  }, [documentId, page, epoch]);

  const screenRectFor = (r: TextBoxRect): ScreenRect => {
    const tl = pdfToScreen({ page, x: r[0], y: r[3] }, geo);
    const br = pdfToScreen({ page, x: r[2], y: r[1] }, geo);
    return {
      left: Math.min(tl.x, br.x),
      top: Math.min(tl.y, br.y),
      width: Math.abs(br.x - tl.x),
      height: Math.abs(br.y - tl.y),
    };
  };

  // SPEC: P4-EDIT-003b — click a committed box (in Edit Text mode) → arm the Add
  // Text tool (so its style controls show, pre-filled) and open the editor over it.
  const reEdit = (box: TextBoxInfo) => {
    const fontFamily: FontFamily = FONT_FAMILIES.includes(box.fontFamily as FontFamily)
      ? (box.fontFamily as FontFamily)
      : "Helvetica";
    setActiveTool("add-text");
    setOptions({
      fontFamily,
      fontSize: box.fontSize,
      textColor: box.color,
      bold: box.bold,
      italic: box.italic,
      underline: box.underline,
    });
    setText(box.text);
    setEditor({ rect: screenRectFor(box.rect), pdfRect: box.rect, editId: box.id });
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
    if (!ed) return;
    const done = (h: Parameters<typeof setHistory>[1]) => {
      // The PDF changed; reload so the canvas renders the new content text.
      bumpEpoch(documentId);
      setHistory(documentId, h);
    };
    if (ed.editId) {
      // Re-editing an existing box: empty text deletes it (SPEC: P4-EDIT-004).
      const promise = body
        ? updateTextBox(
            documentId,
            page,
            ed.editId,
            body,
            options.fontFamily,
            options.fontSize,
            textColor,
            options.bold,
            options.italic,
            options.underline,
          )
        : deleteTextBox(documentId, page, ed.editId);
      promise
        .then(done)
        .catch((err: unknown) =>
          reportError(body ? "Couldn't update the text" : "Couldn't delete the text", err),
        );
      return;
    }
    // A new box with no text is a no-op.
    if (!body) return;
    // Show the typed box immediately; the ~3 s backend apply + reload on a large
    // PDF would otherwise leave it blank until the bake lands.
    const oe = useOptimisticEditStore.getState();
    const key = oe.add(documentId, page, "text-box", {
      rect: ed.pdfRect,
      text: body,
      fontFamily: options.fontFamily,
      fontSize: options.fontSize,
      color: textColor,
      bold: options.bold,
      italic: options.italic,
      underline: options.underline,
    } satisfies TextBoxHeld);
    addTextBox(
      documentId,
      page,
      ed.pdfRect,
      body,
      options.fontFamily,
      options.fontSize,
      textColor,
      options.bold,
      options.italic,
      options.underline,
    )
      .then((h) => {
        done(h);
        oe.tie(documentId, key, useEditEpochStore.getState().byDoc[documentId] ?? 0);
      })
      .catch((err: unknown) => {
        oe.remove(documentId, key);
        reportError("Couldn't add text", err);
      });
  };

  const pendingBoxes = usePendingEdits<TextBoxHeld>(documentId, page, "text-box");

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

      {/* Optimistic preview: committed boxes not yet baked into the page (P4.HF29). */}
      {pendingBoxes.map(({ key, data }) => {
        const r = screenRectFor(data.rect);
        return (
          <div
            key={key}
            className="absolute whitespace-pre-wrap break-words"
            style={{
              left: r.left,
              top: r.top,
              width: r.width,
              color: data.color,
              fontFamily: cssFontFamily(data.fontFamily),
              fontSize: `${data.fontSize * scale}px`,
              fontWeight: data.bold ? 700 : 400,
              fontStyle: data.italic ? "italic" : "normal",
              textDecoration: data.underline ? "underline" : "none",
              lineHeight: 1.2,
              padding: "1px 2px",
              pointerEvents: "none",
            }}
          >
            {data.text}
          </div>
        );
      })}

      {/* SPEC: P4-EDIT-003b — with the Edit Text tool active, click a committed box
          to re-edit it as a unit. These zones sit above Edit Text's per-run zones
          (this layer mounts last), so added text edits whole-box and foreign text
          stays per-run. Each zone opts back into pointer events under the
          otherwise pass-through layer. */}
      {activeTool === "edit-text" && !editor
        ? boxes.map((b) => {
            const r = screenRectFor(b.rect);
            return (
              <div
                key={b.id}
                className="absolute"
                title="Click to edit this text box"
                onClick={() => reEdit(b)}
                style={{
                  left: r.left,
                  top: r.top,
                  width: r.width,
                  height: r.height,
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
            aria-label="Add text to the page"
            value={text}
            onChange={(e) => setText(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Escape") cancel();
              if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) commit();
            }}
            placeholder="Type text…"
            style={{
              width: "100%",
              height: Math.max(editor.rect.height, options.fontSize * scale * 1.4),
              resize: "none",
              border: "1px solid #2563eb",
              outline: "none",
              padding: "1px 2px",
              color: textColor,
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
              aria-label={editor.editId ? "Save text edit" : "Add text to page"}
              className="rounded bg-blue-600 px-2 py-0.5 text-xs font-medium text-white hover:bg-blue-700"
            >
              {editor.editId ? "Save" : "Add"}
            </button>
          </div>
        </div>
      ) : null}
    </div>
  );
}
