// SPEC: P3-ANN-002 (re-openable clause, P3.B2b) — keep the sticky-note overlay
// a *projection of the PDF*.
//
// Notes carry no /AP, so the canvas can't draw them; the NoteLayer renders them
// from the annotation store. B2a kept that store in lockstep only for in-session
// edits, which left two gaps: a reopened (or session-restored) file showed no
// notes, and an actor-level undo/redo left a ghost icon. This hook closes both by
// reading the notes out of the actor and replacing the store's notes whenever the
// document changes OR its edit epoch bumps (every reload-edit, incl. undo/redo).
//
// Placement deliberately does NOT bump the epoch, so the optimistic icon added at
// placement isn't clobbered before its write lands.

import { useEffect } from "react";

import { readTextNotes } from "@/ipc/notes";
import type { DocumentId } from "@/ipc/pdf";
import { useAnnotationStore } from "@/state/annotation-store";
import { useDocEpoch } from "@/state/edit-epoch-store";
import type { NoteAnnotation } from "@/tools/_framework";
import { NOTE_COLOR } from "@/tools/sticky-note/note-tool";

export function useNotesSync(documentId: DocumentId | undefined): void {
  const replaceNotes = useAnnotationStore((s) => s.replaceNotes);
  // Re-run on every reload-edit (delete/undo/redo bump this); 0 before any.
  const epoch = useDocEpoch(documentId);

  useEffect(() => {
    if (!documentId) return undefined;
    let cancelled = false;
    void readTextNotes(documentId)
      .then((notes) => {
        if (cancelled) return;
        const mapped: NoteAnnotation[] = notes.map((n) => ({
          id: n.nm,
          type: "note",
          page: n.page,
          point: { x: n.x, y: n.y },
          content: n.content,
          author: n.author,
          color: NOTE_COLOR,
          opacity: 1,
          // The PDF's /CreationDate isn't parsed back yet (display is D1); the
          // overlay doesn't use createdAt, so 0 is a safe placeholder.
          createdAt: 0,
        }));
        replaceNotes(documentId, mapped);
      })
      .catch((err: unknown) => console.warn("read notes failed", documentId, err));
    return () => {
      cancelled = true;
    };
  }, [documentId, epoch, replaceNotes]);
}
