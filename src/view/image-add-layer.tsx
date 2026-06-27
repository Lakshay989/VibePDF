// SPEC: P4-EDIT-005 (P4.C1) — the per-page "add image" overlay.
//
// When the Add Image tool is active (a file has been picked + armed), drag a box
// on the page; on release the image is embedded into the page content stream
// (aspect-fit into the box) via `addImage`. No editor — the image is the content.
// Pointer events, not HTML5 DnD (WKWebView; docs/04).

import { type PointerEvent as ReactPointerEvent, useState } from "react";

import { addImage, type ImageRect } from "@/ipc/image";
import { useEditEpochStore } from "@/state/edit-epoch-store";
import { useHistoryStore } from "@/state/history-store";
import { useImageAddStore } from "@/state/image-add-store";
import { useToolStore } from "@/state/tool-store";
import { type PageGeometry, type ScreenPoint, screenToPdf } from "@/tools/_framework";
import { normalizeScreenRect, withDefaultSize } from "@/tools/free-text/free-text";

export interface ImageAddLayerProps {
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

export function ImageAddLayer({
  documentId,
  page,
  displayedWidth,
  displayedHeight,
  scale,
  rotation,
}: ImageAddLayerProps) {
  const activeTool = useToolStore((s) => s.activeTool);
  const setActiveTool = useToolStore((s) => s.setActiveTool);
  const armedPath = useImageAddStore((s) => s.path);
  const arm = useImageAddStore((s) => s.arm);
  const setHistory = useHistoryStore((s) => s.setHistory);
  const bumpEpoch = useEditEpochStore((s) => s.bumpEpoch);

  const [start, setStart] = useState<ScreenPoint | null>(null);
  const [current, setCurrent] = useState<ScreenPoint | null>(null);

  const placing = activeTool === "add-image" && armedPath !== null;

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
    if (!placing) return;
    e.currentTarget.setPointerCapture?.(e.pointerId);
    const p = layerPoint(e);
    setStart(p);
    setCurrent(p);
  };

  const onPointerMove = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (!start) return;
    setCurrent(layerPoint(e));
  };

  const onPointerUp = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (!start || !armedPath) return;
    const rect = withDefaultSize(normalizeScreenRect(start, layerPoint(e)));
    setStart(null);
    setCurrent(null);
    const a = screenToPdf({ x: rect.left, y: rect.top }, geo);
    const b = screenToPdf({ x: rect.left + rect.width, y: rect.top + rect.height }, geo);
    const pdfRect: ImageRect = [
      Math.min(a.x, b.x),
      Math.min(a.y, b.y),
      Math.max(a.x, b.x),
      Math.max(a.y, b.y),
    ];
    const path = armedPath;
    arm(null);
    setActiveTool(null);
    addImage(documentId, page, pdfRect, path)
      .then((h) => {
        bumpEpoch(documentId);
        setHistory(documentId, h);
      })
      .catch((err: unknown) => console.warn("add image failed", documentId, err));
  };

  if (!placing) return null;

  const preview = start && current ? normalizeScreenRect(start, current) : null;

  return (
    <div
      className="absolute left-0 top-0"
      style={{ width: cssWidth, height: cssHeight, pointerEvents: "auto", cursor: "crosshair" }}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
    >
      {preview ? (
        <div
          className="absolute border-2 border-dashed border-blue-500 bg-blue-400/10"
          style={{ left: preview.left, top: preview.top, width: preview.width, height: preview.height }}
        />
      ) : null}
    </div>
  );
}
