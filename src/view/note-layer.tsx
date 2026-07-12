// SPEC: P3-ANN-002 (P3.B2a) — the per-page sticky-note overlay.
//
// Notes differ from markup/shapes: a `/Text` annotation carries NO `/AP`, so the
// PDF.js canvas never draws it — we render the icon (and its editable popup)
// ourselves from the annotation store. This is an HTML layer (the popup needs a
// <textarea>), a sibling of the SVG AnnotationLayer, mounted ON TOP so it can
// catch placement clicks while the note tool is active.
//
// Persistence is immediate: placing a note writes an empty `/Text` annotation
// through the document actor (so the store and the PDF stay in lockstep from the
// first frame), and editing/deleting are their own undoable actor edits. The
// store id doubles as the annotation's `/NM`, so later edits address this exact
// note.

import { reportError } from "@/app/report-error";
import { type PointerEvent as ReactPointerEvent, useState } from "react";

import { addTextNote, deleteAnnotation, updateTextNote } from "@/ipc/notes";
import { useHistoryStore } from "@/state/history-store";
import { useAnnotationStore, useDocAnnotations } from "@/state/annotation-store";
import { useToolStore } from "@/state/tool-store";
import {
  type NoteAnnotation,
  type PageGeometry,
  pdfToScreen,
  type ScreenPoint,
  screenToPdf,
} from "@/tools/_framework";
import {
  DEFAULT_NOTE_AUTHOR,
  NOTE_COLOR,
  noteDraftAt,
  stickyNoteTool,
} from "@/tools/sticky-note/note-tool";
import { NotePopup } from "@/view/NotePopup";

const ICON_PX = 18;
const POPUP_PX = 224; // matches NotePopup's w-56 (14rem)

export interface NoteLayerProps {
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

export function NoteLayer({
  documentId,
  page,
  displayedWidth,
  displayedHeight,
  scale,
  rotation,
}: NoteLayerProps) {
  const annotations = useDocAnnotations(documentId);
  const addAnnotation = useAnnotationStore((s) => s.add);
  const updateAnnotation = useAnnotationStore((s) => s.update);
  const removeAnnotation = useAnnotationStore((s) => s.remove);
  const activeTool = useToolStore((s) => s.activeTool);
  const setActiveTool = useToolStore((s) => s.setActiveTool);
  const setHistory = useHistoryStore((s) => s.setHistory);

  const [editingId, setEditingId] = useState<string | null>(null);

  const placing = activeTool === "sticky-note";

  // `coords` wants the UNROTATED PDF dimensions; `displayed*` are already
  // rotation-swapped for layout, so swap them back for 90°/270°.
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

  const notes = annotations.filter(
    (a): a is NoteAnnotation => a.type === "note" && a.page === page,
  );

  const placeNote = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (!placing) return;
    const rect = e.currentTarget.getBoundingClientRect();
    const screen: ScreenPoint = { x: e.clientX - rect.left, y: e.clientY - rect.top };
    const pt = screenToPdf(screen, geo);
    const draft = noteDraftAt(page, pt.x, pt.y, DEFAULT_NOTE_AUTHOR);
    const note = addAnnotation(documentId, draft) as NoteAnnotation;
    // Drop back to idle so the popup interaction isn't another placement.
    setActiveTool(null);
    setEditingId(note.id);
    addTextNote(documentId, note.id, page, pt.x, pt.y, "", note.author)
      .then((h) => setHistory(documentId, h))
      .catch((err: unknown) => {
        // Roll the optimistic icon back if the actor rejected the write.
        removeAnnotation(documentId, note.id);
        setEditingId(null);
        reportError("Couldn't add note", err);
      });
  };

  const saveNote = (note: NoteAnnotation, content: string) => {
    updateAnnotation(documentId, note.id, { content });
    setEditingId(null);
    updateTextNote(documentId, note.id, content)
      .then((h) => setHistory(documentId, h))
      .catch((err: unknown) => console.warn("update note failed", documentId, err));
  };

  const deleteNote = (note: NoteAnnotation) => {
    removeAnnotation(documentId, note.id);
    setEditingId(null);
    deleteAnnotation(documentId, note.id)
      .then((h) => setHistory(documentId, h))
      .catch((err: unknown) => reportError("Couldn't delete note", err));
  };

  const editing = notes.find((n) => n.id === editingId) ?? null;

  return (
    <div
      className="absolute left-0 top-0"
      style={{
        width: cssWidth,
        height: cssHeight,
        pointerEvents: placing ? "auto" : "none",
        cursor: placing ? stickyNoteTool.cursor : undefined,
      }}
      onPointerDown={placeNote}
    >
      {notes.map((note) => {
        const s = pdfToScreen({ page, x: note.point.x, y: note.point.y }, geo);
        return (
          <button
            key={note.id}
            type="button"
            title={note.content || "Sticky note"}
            aria-label={note.content ? `Note: ${note.content}` : "Sticky note"}
            onPointerDown={(e) => e.stopPropagation()}
            onClick={(e) => {
              e.stopPropagation();
              setEditingId((cur) => (cur === note.id ? null : note.id));
            }}
            style={{
              left: s.x,
              top: s.y - ICON_PX,
              width: ICON_PX,
              height: ICON_PX,
              background: NOTE_COLOR,
              pointerEvents: "auto",
            }}
            className={
              "absolute flex items-center justify-center rounded-sm border border-amber-700/50 " +
              "shadow-sm hover:brightness-105 " +
              (note.id === editingId ? "ring-2 ring-amber-500" : "")
            }
          >
            <svg viewBox="0 0 16 16" width={11} height={11} aria-hidden="true">
              <path
                d="M2 3h12v8H6l-3 3v-3H2z"
                fill="none"
                stroke="#7a5a00"
                strokeWidth={1.5}
                strokeLinejoin="round"
              />
            </svg>
          </button>
        );
      })}
      {editing ? (
        <PopupAt note={editing} geo={geo} layerWidth={cssWidth} onSave={saveNote} onDelete={deleteNote} onClose={() => setEditingId(null)} />
      ) : null}
    </div>
  );
}

/** Positions the popup next to its icon, flipping left if it would overflow. */
function PopupAt({
  note,
  geo,
  layerWidth,
  onSave,
  onDelete,
  onClose,
}: {
  note: NoteAnnotation;
  geo: PageGeometry;
  layerWidth: number;
  onSave: (note: NoteAnnotation, content: string) => void;
  onDelete: (note: NoteAnnotation) => void;
  onClose: () => void;
}) {
  const s: ScreenPoint = pdfToScreen({ page: geo.page, x: note.point.x, y: note.point.y }, geo);
  const overflowsRight = s.x + ICON_PX + 4 + POPUP_PX > layerWidth;
  const left = overflowsRight ? Math.max(0, s.x - POPUP_PX - 4) : s.x + ICON_PX + 4;
  const top = Math.max(0, s.y - ICON_PX);
  return (
    <NotePopup
      note={note}
      left={left}
      top={top}
      onSave={(content) => onSave(note, content)}
      onDelete={() => onDelete(note)}
      onClose={onClose}
    />
  );
}
