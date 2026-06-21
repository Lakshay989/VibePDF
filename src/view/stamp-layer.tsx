// SPEC: P3-ANN-006 (P3.C3a) — the per-page stamp placement overlay.
//
// When the stamp tool is active AND a stamp is armed (chosen in the palette), a
// click drops it centred on the click point at a default size; the actor writes
// a `/Stamp` with a generated `/AP` and the canvas renders it. Self-contained
// like the note / polygon / ink layers — a single click-to-place gesture that
// doesn't fit `stepTool`'s drag lifecycle.

import { type PointerEvent as ReactPointerEvent } from "react";

import { addStamp } from "@/ipc/stamps";
import { useEditEpochStore } from "@/state/edit-epoch-store";
import { useHistoryStore } from "@/state/history-store";
import { useStampStore } from "@/state/stamp-store";
import { useToolStore } from "@/state/tool-store";
import { type PageGeometry, screenToPdf } from "@/tools/_framework";
import { stampRectAt } from "@/tools/stamp/stamps";

export interface StampLayerProps {
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

export function StampLayer({
  documentId,
  page,
  displayedWidth,
  displayedHeight,
  scale,
  rotation,
}: StampLayerProps) {
  const activeTool = useToolStore((s) => s.activeTool);
  const options = useToolStore((s) => s.options);
  const armed = useStampStore((s) => s.armed);
  const setHistory = useHistoryStore((s) => s.setHistory);
  const bumpEpoch = useEditEpochStore((s) => s.bumpEpoch);

  const active = activeTool === "stamp" && armed !== null;

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

  const onPointerDown = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (!active || !armed || e.button !== 0) return;
    const r = e.currentTarget.getBoundingClientRect();
    const pdf = screenToPdf({ x: e.clientX - r.left, y: e.clientY - r.top }, geo);
    const rect = stampRectAt(pdf.x, pdf.y, geo.width, geo.height);
    addStamp(documentId, page, rect, armed.label, armed.name, armed.color, options.opacity)
      .then((h) => {
        bumpEpoch(documentId);
        setHistory(documentId, h);
      })
      .catch((err: unknown) => console.warn("add stamp failed", documentId, err));
  };

  return (
    <div
      className="absolute left-0 top-0"
      style={{
        width: cssWidth,
        height: cssHeight,
        pointerEvents: active ? "auto" : "none",
        cursor: active ? "copy" : undefined,
      }}
      onPointerDown={onPointerDown}
    />
  );
}
