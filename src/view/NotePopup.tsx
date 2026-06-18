// SPEC: P3-ANN-002 (P3.B2a) — the sticky-note popup editor.
//
// A small floating card anchored next to a note icon. It edits the note body
// and exposes Save / Delete. Persistence (the IPC round-trip through the
// document actor) is the caller's job — this is a pure controlled editor so it
// stays trivially testable. Pointer events are stopped from bubbling so typing
// or clicking inside never reaches `NoteLayer` (which would place or dismiss).

import { type PointerEvent as ReactPointerEvent, useEffect, useRef, useState } from "react";

import type { NoteAnnotation } from "@/tools/_framework";

export interface NotePopupProps {
  note: NoteAnnotation;
  /** Top-left in layer (CSS px) coordinates. */
  left: number;
  top: number;
  onSave: (content: string) => void;
  onDelete: () => void;
  onClose: () => void;
}

export function NotePopup({ note, left, top, onSave, onDelete, onClose }: NotePopupProps) {
  const [text, setText] = useState(note.content);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  // Re-sync when the popup is reused for a different note.
  useEffect(() => {
    setText(note.content);
  }, [note.id, note.content]);

  useEffect(() => {
    const el = textareaRef.current;
    if (!el) return;
    el.focus();
    // Caret at the end so existing text is appended to, not overwritten.
    el.setSelectionRange(el.value.length, el.value.length);
  }, [note.id]);

  const stop = (e: ReactPointerEvent<HTMLElement>) => e.stopPropagation();

  return (
    <div
      role="dialog"
      aria-label="Sticky note"
      onPointerDown={stop}
      // `pointer-events: auto` is essential: the NoteLayer container is
      // `pointer-events: none` when no tool is active (so clicks fall through to
      // the text layer), and this popup is its child. Without this, clicks into
      // the textarea / Save pass straight through and steal focus, so you can't
      // type — only the programmatic autofocus appeared to work.
      style={{ left, top, pointerEvents: "auto" }}
      className="absolute z-10 w-56 rounded border border-neutral-300 bg-amber-50 p-2 text-sm shadow-lg dark:border-neutral-600 dark:bg-amber-100"
    >
      <div className="mb-1 flex items-center justify-between">
        <span className="truncate text-xs font-medium text-neutral-600" title={note.author}>
          {note.author}
        </span>
        <button
          type="button"
          onClick={onClose}
          aria-label="Close note"
          title="Close"
          className="px-1 text-neutral-500 hover:text-neutral-800"
        >
          ✕
        </button>
      </div>
      <textarea
        ref={textareaRef}
        value={text}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Escape") onClose();
          // Cmd/Ctrl+Enter saves, matching common note editors.
          if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) onSave(text);
        }}
        rows={4}
        placeholder="Add a note…"
        className="w-full resize-none rounded border border-neutral-300 bg-white p-1 text-neutral-900 focus:outline-none focus:ring-1 focus:ring-amber-500"
      />
      <div className="mt-1 flex items-center justify-between">
        <button
          type="button"
          onClick={onDelete}
          aria-label="Delete note"
          className="rounded px-2 py-0.5 text-xs text-red-600 hover:bg-red-100"
        >
          Delete
        </button>
        <button
          type="button"
          onClick={() => onSave(text)}
          aria-label="Save note"
          className="rounded bg-amber-500 px-3 py-0.5 text-xs font-medium text-white hover:bg-amber-600"
        >
          Save
        </button>
      </div>
    </div>
  );
}
