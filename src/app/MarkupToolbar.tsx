// SPEC: P3-ANN-001 (P3.B1a) — the text-markup toolbar.
//
// Select text in the page, then click a markup button to apply it to the
// selection (highlight / underline / strikethrough / squiggly), in the chosen
// colour. `onMouseDown → preventDefault` is essential: a normal mousedown on a
// button collapses the page text selection before the click handler runs, so we
// suppress it to keep the selection alive.

import { useEffect, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";

import { SignatureDialog } from "@/app/SignatureDialog";
import { addTextMarkup, clearTextMarkup } from "@/ipc/annotations";
import { useEditEpochStore } from "@/state/edit-epoch-store";
import { useHistoryStore } from "@/state/history-store";
import { useImageAddStore } from "@/state/image-add-store";
import { useMeasureStore } from "@/state/measure-store";
import { useStampStore } from "@/state/stamp-store";
import { useToolStore } from "@/state/tool-store";
import type { FontFamily, MarkupSubtype } from "@/tools/_framework";
import { FONT_FAMILIES } from "@/tools/free-text/free-text";
import { MeasureControls } from "@/tools/measure/MeasureControls";
import { StampPalette } from "@/tools/stamp/StampPalette";
import { usesStampLayer } from "@/tools/stamp/stamps";
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
  // SPEC: P6-SEC-001 (P6.A2) — the signature library is document-independent
  // (it writes to app data, not to the open PDF), so this toolbar owns the
  // dialog outright instead of threading a callback through PdfViewer the way
  // the page-operation dialogs do.
  const [signOpen, setSignOpen] = useState(false);
  const color = useToolStore((s) => s.options.color);
  const opacity = useToolStore((s) => s.options.opacity);
  const setOptions = useToolStore((s) => s.setOptions);
  const activeTool = useToolStore((s) => s.activeTool);
  const setActiveTool = useToolStore((s) => s.setActiveTool);
  const fontFamily = useToolStore((s) => s.options.fontFamily);
  const fontSize = useToolStore((s) => s.options.fontSize);
  const bold = useToolStore((s) => s.options.bold);
  const italic = useToolStore((s) => s.options.italic);
  const underline = useToolStore((s) => s.options.underline);
  const textColor = useToolStore((s) => s.options.textColor) ?? "#000000";
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
  // P6.A5a — armed with a signature from the library, waiting for a click.
  const signatureArmed = activeTool === "signature";
  const measureActive = activeTool === "measure";
  const editTextActive = activeTool === "edit-text";
  const addTextActive = activeTool === "add-text";
  const addImageActive = activeTool === "add-image";
  const editImageActive = activeTool === "edit-image";
  const addLinkActive = activeTool === "add-link";
  const createFieldActive = activeTool === "create-text-field";
  const fillable = shapeActive || polygonActive;

  // Disarm the stamp whenever the stamp tool is left, so a later click with
  // another tool can't drop a stale stamp (the polygon rubber-band lesson).
  const armStamp = useStampStore((s) => s.arm);

  // P6.A5a — Escape backs out of signature placement. The mode has no palette
  // and no obvious affordance beyond the hint, so the key everyone already
  // tries has to work.
  useEffect(() => {
    if (activeTool !== "signature") return undefined;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      armStamp(null);
      setActiveTool(null);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [activeTool, armStamp, setActiveTool]);
  // Disarm when the stamp layer is no longer driving. `"signature"` counts as
  // driving it (P6.A5a) — treating it as "left the stamp tool" cleared the
  // signature the dialog had just armed.
  useEffect(() => {
    if (!usesStampLayer(activeTool)) armStamp(null);
  }, [activeTool, armStamp]);

  // Disarm the picked image whenever the Add Image tool is left.
  const armImage = useImageAddStore((s) => s.arm);
  useEffect(() => {
    if (activeTool !== "add-image") armImage(null);
  }, [activeTool, armImage]);

  // Add Image: pick a PNG/JPEG, then arm it for drag-placement.
  const pickImage = () => {
    void openDialog({
      multiple: false,
      directory: false,
      filters: [{ name: "Image", extensions: ["png", "jpg", "jpeg"] }],
    })
      .then((picked) => {
        if (typeof picked === "string") {
          armImage(picked);
          setActiveTool("add-image");
        }
      })
      .catch((err: unknown) => console.warn("image pick failed", err));
  };

  // Cancel any in-progress calibration when the measure tool is left.
  const cancelCalibrating = useMeasureStore((s) => s.cancelCalibrating);
  useEffect(() => {
    if (activeTool !== "measure") cancelCalibrating();
  }, [activeTool, cancelCalibrating]);

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
    <div className="flex flex-wrap items-center gap-x-2 gap-y-1 border-b border-neutral-200 px-3 py-1 text-sm dark:border-neutral-800">
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
        title="Add a text box annotation — re-editable, and listed in the Annotations panel"
        aria-label="Text box annotation tool"
        aria-pressed={textActive}
        className={
          "rounded px-2 py-0.5 hover:bg-neutral-100 dark:hover:bg-neutral-800 " +
          (textActive ? "bg-blue-200 dark:bg-blue-300/30" : "")
        }
      >
        Text Box
      </button>
      {textActive || addTextActive ? (
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
          <button
            type="button"
            onClick={() => setOptions({ underline: !underline })}
            aria-label="Underline"
            aria-pressed={underline}
            className={
              "rounded px-1.5 py-0.5 text-xs underline hover:bg-neutral-100 dark:hover:bg-neutral-800 " +
              (underline ? "bg-blue-200 dark:bg-blue-300/30" : "")
            }
          >
            U
          </button>
          {/* SPEC: P4-EDIT-003 — text has its own colour (black by default), kept
              separate from the markup colour above (P4.HF25). */}
          <span className="ml-1 text-xs text-neutral-400">Colour</span>
          {COLORS.map((c) => (
            <button
              key={`text-${c}`}
              type="button"
              onClick={() => setOptions({ textColor: c })}
              aria-label={`Text colour ${c}`}
              aria-pressed={textColor === c}
              title={c}
              style={{ backgroundColor: c }}
              className={
                "h-4 w-4 rounded-full border border-neutral-300 dark:border-neutral-600 " +
                (textColor === c ? "ring-2 ring-blue-500 ring-offset-1" : "")
              }
            />
          ))}
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
      {/* SPEC: P6-SEC-004 (P6.A5a) — armed with a signature. No palette: there
          is nothing to choose, the choice was made in the dialog. What is
          needed is a way out, since the mode is otherwise invisible. */}
      {signatureArmed ? (
        <span className="flex items-center gap-1 rounded bg-blue-100 px-2 py-0.5 dark:bg-blue-300/20">
          <span>Click the page to place your signature</span>
          <button
            type="button"
            onClick={() => {
              armStamp(null);
              setActiveTool(null);
            }}
            aria-label="Cancel placing the signature"
            className="rounded border border-blue-400 px-1 hover:bg-blue-200 dark:hover:bg-blue-300/30"
          >
            Cancel
          </button>
        </span>
      ) : null}
      <button
        type="button"
        onClick={() => setActiveTool(measureActive ? null : "measure")}
        title="Measure (distance / perimeter / area; calibrate against a known length)"
        aria-label="Measure tool"
        aria-pressed={measureActive}
        className={
          "rounded px-2 py-0.5 hover:bg-neutral-100 dark:hover:bg-neutral-800 " +
          (measureActive ? "bg-blue-200 dark:bg-blue-300/30" : "")
        }
      >
        Measure
      </button>
      {measureActive ? <MeasureControls documentId={documentId} /> : null}
      <button
        type="button"
        onClick={() => setActiveTool(editTextActive ? null : "edit-text")}
        title="Edit text (click a text run to rewrite it in place)"
        aria-label="Edit text tool"
        aria-pressed={editTextActive}
        className={
          "rounded px-2 py-0.5 hover:bg-neutral-100 dark:hover:bg-neutral-800 " +
          (editTextActive ? "bg-blue-200 dark:bg-blue-300/30" : "")
        }
      >
        Edit Text
      </button>
      <button
        type="button"
        onClick={() => setActiveTool(addTextActive ? null : "add-text")}
        title="Add text into the page content — permanent (not an annotation); re-edit later with the Edit Text tool"
        aria-label="Add text into the page tool"
        aria-pressed={addTextActive}
        className={
          "rounded px-2 py-0.5 hover:bg-neutral-100 dark:hover:bg-neutral-800 " +
          (addTextActive ? "bg-blue-200 dark:bg-blue-300/30" : "")
        }
      >
        Add Text
      </button>
      <button
        type="button"
        onClick={pickImage}
        title="Add an image (PNG/JPEG) to the page (pick a file, then drag a box)"
        aria-label="Add image tool"
        aria-pressed={addImageActive}
        className={
          "rounded px-2 py-0.5 hover:bg-neutral-100 dark:hover:bg-neutral-800 " +
          (addImageActive ? "bg-blue-200 dark:bg-blue-300/30" : "")
        }
      >
        Add Image
      </button>
      <button
        type="button"
        onClick={() => setActiveTool(editImageActive ? null : "edit-image")}
        title="Edit image (click an image to move / resize / rotate / delete it)"
        aria-label="Edit image tool"
        aria-pressed={editImageActive}
        className={
          "rounded px-2 py-0.5 hover:bg-neutral-100 dark:hover:bg-neutral-800 " +
          (editImageActive ? "bg-blue-200 dark:bg-blue-300/30" : "")
        }
      >
        Edit Image
      </button>
      <button
        type="button"
        onClick={() => setActiveTool(addLinkActive ? null : "add-link")}
        title="Add a hyperlink (drag a box, then choose a URL / email / page / named destination)"
        aria-label="Add link tool"
        aria-pressed={addLinkActive}
        className={
          "rounded px-2 py-0.5 hover:bg-neutral-100 dark:hover:bg-neutral-800 " +
          (addLinkActive ? "bg-blue-200 dark:bg-blue-300/30" : "")
        }
      >
        Add Link
      </button>
      <button
        type="button"
        onClick={() => setSignOpen(true)}
        title="Draw a signature and save it to your library"
        aria-label="Signatures"
        className="rounded px-2 py-0.5 hover:bg-neutral-100 dark:hover:bg-neutral-800"
      >
        Sign
      </button>
      <SignatureDialog open={signOpen} onClose={() => setSignOpen(false)} />
      <button
        type="button"
        onClick={() => setActiveTool(createFieldActive ? null : "create-text-field")}
        title="Create a form field (drag a box, then choose text / checkbox / radio / dropdown / list / signature / button)"
        aria-label="Create form field tool"
        aria-pressed={createFieldActive}
        className={
          "rounded px-2 py-0.5 hover:bg-neutral-100 dark:hover:bg-neutral-800 " +
          (createFieldActive ? "bg-blue-200 dark:bg-blue-300/30" : "")
        }
      >
        Form Field
      </button>
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
