// SPEC: P3-ANN-001 (P3.B1a) — the text-markup toolbar.
//
// Select text in the page, then click a markup button to apply it to the
// selection (highlight / underline / strikethrough / squiggly), in the chosen
// colour. `onMouseDown → preventDefault` is essential: a normal mousedown on a
// button collapses the page text selection before the click handler runs, so we
// suppress it to keep the selection alive.

import { useEffect } from "react";

import { addTextMarkup, clearTextMarkup } from "@/ipc/annotations";
import { useEditEpochStore } from "@/state/edit-epoch-store";
import { useHistoryStore } from "@/state/history-store";
import { useStampStore } from "@/state/stamp-store";
import { useToolStore } from "@/state/tool-store";
import type { FontFamily, MarkupSubtype } from "@/tools/_framework";
import { FONT_FAMILIES } from "@/tools/free-text/free-text";
import { StampPalette } from "@/tools/stamp/StampPalette";
import { applyMarkupToSelection } from "@/tools/text-markup/apply-markup";

const SUBTYPES: { id: MarkupSubtype; label: string; title: string }[] = [
  { id: "highlight", label: "Highlight", title: "Highlight selected text" },
  { id: "underline", label: "Underline", title: "Underline selected text" },
  { id: "strikethrough", label: "Strikethrough", title: "Strike through selected text" },
  { id: "squiggly", label: "Squiggly", title: "Squiggly underline selected text" },
];

// A basic palette including black + white. Shared by markup stroke, free-text
// colour, and shape stroke/fill.
const COLORS = [
  "#000000",
  "#e11d48",
  "#f59e0b",
  "#ffd400",
  "#22c55e",
  "#3b82f6",
  "#a855f7",
  "#ffffff",
];

export function MarkupToolbar({ documentId }: { documentId: string }) {
  const color = useToolStore((s) => s.options.color);
  const opacity = useToolStore((s) => s.options.opacity);
  const setOptions = useToolStore((s) => s.setOptions);
  const activeTool = useToolStore((s) => s.activeTool);
  const setActiveTool = useToolStore((s) => s.setActiveTool);
  const fontFamily = useToolStore((s) => s.options.fontFamily);
  const fontSize = useToolStore((s) => s.options.fontSize);
  const bold = useToolStore((s) => s.options.bold);
  const italic = useToolStore((s) => s.options.italic);
  const bumpEpoch = useEditEpochStore((s) => s.bumpEpoch);
  const setHistory = useHistoryStore((s) => s.setHistory);
  const fillColor = useToolStore((s) => s.options.fillColor);
  const noteActive = activeTool === "sticky-note";
  const textActive = activeTool === "free-text";
  const rectActive = activeTool === "rectangle";
  const ellipseActive = activeTool === "ellipse";
  const shapeActive = rectActive || ellipseActive;
  const lineActive = activeTool === "line";
  const arrowActive = activeTool === "arrow";
  const polygonActive = activeTool === "polygon";
  const inkActive = activeTool === "ink";
  const stampActive = activeTool === "stamp";
  const fillable = shapeActive || polygonActive;

  // Disarm the stamp whenever the stamp tool is left, so a later click with
  // another tool can't drop a stale stamp (the polygon rubber-band lesson).
  const armStamp = useStampStore((s) => s.arm);
  useEffect(() => {
    if (activeTool !== "stamp") armStamp(null);
  }, [activeTool, armStamp]);

  const apply = (subtype: MarkupSubtype) => {
    void applyMarkupToSelection({ documentId, subtype, color, opacity }, (page, quads) =>
      addTextMarkup(documentId, page, subtype, quads, color, opacity),
    )
      .then((history) => {
        if (history) {
          // The markup is now in the PDF; reload so the canvas renders it.
          bumpEpoch(documentId);
          setHistory(documentId, history);
        }
      })
      .catch((err: unknown) => console.warn("markup failed", documentId, err));
  };

  const clearAll = () => {
    void clearTextMarkup(documentId)
      .then((history) => {
        bumpEpoch(documentId);
        setHistory(documentId, history);
      })
      .catch((err: unknown) => console.warn("clear markup failed", documentId, err));
  };

  return (
    <div className="flex items-center gap-2 border-b border-neutral-200 px-3 py-1 text-sm dark:border-neutral-800">
      <span className="text-xs uppercase tracking-wide text-neutral-400">Markup</span>
      {SUBTYPES.map((s) => (
        <button
          key={s.id}
          type="button"
          onMouseDown={(e) => e.preventDefault()}
          onClick={() => apply(s.id)}
          title={s.title}
          aria-label={s.title}
          className="rounded px-2 py-0.5 hover:bg-neutral-100 dark:hover:bg-neutral-800"
        >
          {s.label}
        </button>
      ))}
      <span className="text-neutral-300 dark:text-neutral-700">|</span>
      <div className="flex items-center gap-1">
        {COLORS.map((c) => (
          <button
            key={c}
            type="button"
            onMouseDown={(e) => e.preventDefault()}
            onClick={() => setOptions({ color: c })}
            aria-label={`Markup colour ${c}`}
            aria-pressed={color === c}
            title={c}
            style={{ backgroundColor: c }}
            className={
              "h-4 w-4 rounded-full border border-neutral-300 dark:border-neutral-600 " +
              (color === c ? "ring-2 ring-blue-500 ring-offset-1" : "")
            }
          />
        ))}
      </div>
      <span className="text-neutral-300 dark:text-neutral-700">|</span>
      <button
        type="button"
        onClick={() => setActiveTool(noteActive ? null : "sticky-note")}
        title="Place a sticky note (click on the page)"
        aria-label="Sticky note tool"
        aria-pressed={noteActive}
        className={
          "rounded px-2 py-0.5 hover:bg-neutral-100 dark:hover:bg-neutral-800 " +
          (noteActive ? "bg-amber-200 dark:bg-amber-300/30" : "")
        }
      >
        Note
      </button>
      <button
        type="button"
        onClick={() => setActiveTool(textActive ? null : "free-text")}
        title="Add a text box (drag on the page)"
        aria-label="Free-text tool"
        aria-pressed={textActive}
        className={
          "rounded px-2 py-0.5 hover:bg-neutral-100 dark:hover:bg-neutral-800 " +
          (textActive ? "bg-blue-200 dark:bg-blue-300/30" : "")
        }
      >
        Text
      </button>
      {textActive ? (
        <div className="flex items-center gap-1">
          <select
            aria-label="Font family"
            value={fontFamily}
            onChange={(e) => setOptions({ fontFamily: e.target.value as FontFamily })}
            className="rounded border border-neutral-300 bg-transparent px-1 py-0.5 text-xs dark:border-neutral-600"
          >
            {FONT_FAMILIES.map((f) => (
              <option key={f} value={f}>
                {f}
              </option>
            ))}
          </select>
          <input
            type="number"
            aria-label="Font size"
            min={6}
            max={144}
            value={fontSize}
            onChange={(e) => setOptions({ fontSize: Number(e.target.value) || fontSize })}
            className="w-12 rounded border border-neutral-300 bg-transparent px-1 py-0.5 text-xs dark:border-neutral-600"
          />
          <button
            type="button"
            onClick={() => setOptions({ bold: !bold })}
            aria-label="Bold"
            aria-pressed={bold}
            className={
              "rounded px-1.5 py-0.5 text-xs font-bold hover:bg-neutral-100 dark:hover:bg-neutral-800 " +
              (bold ? "bg-blue-200 dark:bg-blue-300/30" : "")
            }
          >
            B
          </button>
          <button
            type="button"
            onClick={() => setOptions({ italic: !italic })}
            aria-label="Italic"
            aria-pressed={italic}
            className={
              "rounded px-1.5 py-0.5 text-xs italic hover:bg-neutral-100 dark:hover:bg-neutral-800 " +
              (italic ? "bg-blue-200 dark:bg-blue-300/30" : "")
            }
          >
            I
          </button>
        </div>
      ) : null}
      <button
        type="button"
        onClick={() => setActiveTool(rectActive ? null : "rectangle")}
        title="Draw a rectangle (drag on the page)"
        aria-label="Rectangle tool"
        aria-pressed={rectActive}
        className={
          "rounded px-2 py-0.5 hover:bg-neutral-100 dark:hover:bg-neutral-800 " +
          (rectActive ? "bg-blue-200 dark:bg-blue-300/30" : "")
        }
      >
        Rectangle
      </button>
      <button
        type="button"
        onClick={() => setActiveTool(ellipseActive ? null : "ellipse")}
        title="Draw an ellipse (drag on the page)"
        aria-label="Ellipse tool"
        aria-pressed={ellipseActive}
        className={
          "rounded px-2 py-0.5 hover:bg-neutral-100 dark:hover:bg-neutral-800 " +
          (ellipseActive ? "bg-blue-200 dark:bg-blue-300/30" : "")
        }
      >
        Ellipse
      </button>
      <button
        type="button"
        onClick={() => setActiveTool(lineActive ? null : "line")}
        title="Draw a line (drag on the page)"
        aria-label="Line tool"
        aria-pressed={lineActive}
        className={
          "rounded px-2 py-0.5 hover:bg-neutral-100 dark:hover:bg-neutral-800 " +
          (lineActive ? "bg-blue-200 dark:bg-blue-300/30" : "")
        }
      >
        Line
      </button>
      <button
        type="button"
        onClick={() => setActiveTool(arrowActive ? null : "arrow")}
        title="Draw an arrow (drag on the page)"
        aria-label="Arrow tool"
        aria-pressed={arrowActive}
        className={
          "rounded px-2 py-0.5 hover:bg-neutral-100 dark:hover:bg-neutral-800 " +
          (arrowActive ? "bg-blue-200 dark:bg-blue-300/30" : "")
        }
      >
        Arrow
      </button>
      <button
        type="button"
        onClick={() => setActiveTool(polygonActive ? null : "polygon")}
        title="Draw a polygon (click each vertex; click the first dot, double-click, or Enter to finish; Esc cancels)"
        aria-label="Polygon tool"
        aria-pressed={polygonActive}
        className={
          "rounded px-2 py-0.5 hover:bg-neutral-100 dark:hover:bg-neutral-800 " +
          (polygonActive ? "bg-blue-200 dark:bg-blue-300/30" : "")
        }
      >
        Polygon
      </button>
      <button
        type="button"
        onClick={() => setActiveTool(inkActive ? null : "ink")}
        title="Freehand pen (draw, release to finish)"
        aria-label="Pen tool"
        aria-pressed={inkActive}
        className={
          "rounded px-2 py-0.5 hover:bg-neutral-100 dark:hover:bg-neutral-800 " +
          (inkActive ? "bg-blue-200 dark:bg-blue-300/30" : "")
        }
      >
        Pen
      </button>
      <button
        type="button"
        onClick={() => setActiveTool(stampActive ? null : "stamp")}
        title="Stamp (pick one, then click the page to place it)"
        aria-label="Stamp tool"
        aria-pressed={stampActive}
        className={
          "rounded px-2 py-0.5 hover:bg-neutral-100 dark:hover:bg-neutral-800 " +
          (stampActive ? "bg-blue-200 dark:bg-blue-300/30" : "")
        }
      >
        Stamp
      </button>
      {stampActive ? <StampPalette /> : null}
      {fillable ? (
        <div className="flex items-center gap-1">
          <span className="text-xs text-neutral-400">Fill</span>
          <button
            type="button"
            onClick={() => setOptions({ fillColor: null })}
            aria-label="No fill"
            aria-pressed={fillColor === null}
            title="No fill"
            className={
              "h-4 w-4 rounded-full border border-neutral-300 bg-white text-[8px] leading-none text-neutral-400 dark:border-neutral-600 " +
              (fillColor === null ? "ring-2 ring-blue-500 ring-offset-1" : "")
            }
          >
            ∅
          </button>
          {COLORS.map((c) => (
            <button
              key={`fill-${c}`}
              type="button"
              onClick={() => setOptions({ fillColor: c })}
              aria-label={`Fill colour ${c}`}
              aria-pressed={fillColor === c}
              title={c}
              style={{ backgroundColor: c }}
              className={
                "h-4 w-4 rounded-full border border-neutral-300 dark:border-neutral-600 " +
                (fillColor === c ? "ring-2 ring-blue-500 ring-offset-1" : "")
              }
            />
          ))}
        </div>
      ) : null}
      <span className="text-neutral-300 dark:text-neutral-700">|</span>
      <button
        type="button"
        onMouseDown={(e) => e.preventDefault()}
        onClick={clearAll}
        title="Remove all text markup from the document"
        className="rounded px-2 py-0.5 text-red-600 hover:bg-neutral-100 dark:text-red-400 dark:hover:bg-neutral-800"
      >
        Clear
      </button>
    </div>
  );
}
