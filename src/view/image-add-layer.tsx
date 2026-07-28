// SPEC: P4-EDIT-005 (P4.C1) — the per-page "add image" overlay.
//
// When the Add Image tool is active (a file has been picked + armed), drag a box
// on the page; on release the image is embedded into the page content stream
// (aspect-fit into the box) via `addImage`. No editor — the image is the content.
// Pointer events, not HTML5 DnD (WKWebView; docs/04).

import { reportError } from "@/app/report-error";
import { type PointerEvent as ReactPointerEvent, useState } from "react";

import { addImage, type ImageRect } from "@/ipc/image";
import { useEditEpochStore } from "@/state/edit-epoch-store";
import { useHistoryStore } from "@/state/history-store";
import { useImageAddStore } from "@/state/image-add-store";
import { useOptimisticEditStore, usePendingEdits } from "@/state/optimistic-edit-store";
import { useToolStore } from "@/state/tool-store";
import { type PageGeometry, pdfToScreen, type ScreenPoint, screenToPdf } from "@/tools/_framework";
import { normalizeScreenRect, withDefaultSize } from "@/tools/_framework";
import { fileToDataUrl } from "@/view/file-data-url";

/** Optimistic-preview payload: a committed image awaiting bake (P4.HF29). */
interface ImageHeld {
  rect: ImageRect;
  /** `data:` URL of the picked file, for an <img> preview. */
  src: string;
}

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
  const pendingImages = usePendingEdits<ImageHeld>(documentId, page, "image");

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
    const oe = useOptimisticEditStore.getState();
    // Read the image (local, fast) → show it immediately, then persist. The
    // ~3 s apply + reload on a large PDF would otherwise leave a blank gap.
    void (async () => {
      let key: string | null = null;
      try {
        const src = await fileToDataUrl(path);
        key = oe.add(documentId, page, "image", { rect: pdfRect, src } satisfies ImageHeld);
      } catch {
        // No preview available (read failed); still place the image below.
      }
      try {
        const h = await addImage(documentId, page, pdfRect, path);
        bumpEpoch(documentId);
        if (key) oe.tie(documentId, key, useEditEpochStore.getState().byDoc[documentId] ?? 0);
        setHistory(documentId, h);
      } catch (err) {
        if (key) oe.remove(documentId, key);
        reportError("Couldn't add image", err);
      }
    })();
  };

  // The layer stays mounted even when not placing, so committed-but-not-yet-baked
  // images keep showing until their reload lands (it's click-through when idle).
  const preview = placing && start && current ? normalizeScreenRect(start, current) : null;

  return (
    <div
      className="absolute left-0 top-0"
      style={{
        width: cssWidth,
        height: cssHeight,
        pointerEvents: placing ? "auto" : "none",
        cursor: placing ? "crosshair" : undefined,
      }}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
    >
      {/* Optimistic preview: committed images not yet baked into the page (P4.HF29). */}
      {pendingImages.map(({ key, data }) => {
        const tl = pdfToScreen({ page, x: data.rect[0], y: data.rect[3] }, geo);
        const br = pdfToScreen({ page, x: data.rect[2], y: data.rect[1] }, geo);
        return (
          <img
            key={key}
            src={data.src}
            alt=""
            draggable={false}
            style={{
              position: "absolute",
              left: Math.min(tl.x, br.x),
              top: Math.min(tl.y, br.y),
              width: Math.abs(br.x - tl.x),
              height: Math.abs(br.y - tl.y),
              objectFit: "fill",
              pointerEvents: "none",
            }}
          />
        );
      })}
      {preview ? (
        <div
          className="absolute border-2 border-dashed border-blue-500 bg-blue-400/10"
          style={{ left: preview.left, top: preview.top, width: preview.width, height: preview.height }}
        />
      ) : null}
    </div>
  );
}
