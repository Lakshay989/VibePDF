// SPEC: P4-EDIT-006 (P4.C2) — the per-page image-edit overlay.
//
// In edit-image mode this fetches the page's images (A1-style), lays a hit-zone
// over each, and on click shows a selection box: drag the body to move, drag a
// corner to resize, or use the floating Rotate 90° / Delete buttons. Each gesture
// computes a new placement matrix the actor applies. Pointer events, not HTML5
// DnD (WKWebView; docs/04).

import { type PointerEvent as ReactPointerEvent, useEffect, useRef, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";

import { deleteImage, extractImages, type ImageInfo, replaceImage, transformImage } from "@/ipc/image-edit";
import { useDocEpoch, useEditEpochStore } from "@/state/edit-epoch-store";
import { useHistoryStore } from "@/state/history-store";
import { useToolStore } from "@/state/tool-store";
import { type PageGeometry, pdfToScreen, type ScreenPoint, screenToPdf } from "@/tools/_framework";
import { rectToMatrix, rotate90 } from "@/tools/image-edit/matrix";

export interface ImageEditLayerProps {
  documentId: string;
  page: number;
  displayedWidth: number;
  displayedHeight: number;
  scale: number;
  rotation: number;
}

interface ScreenRect {
  left: number;
  top: number;
  width: number;
  height: number;
}

interface Drag {
  kind: "move" | "resize";
  /** Resize corner: 0=TL, 1=TR, 2=BR, 3=BL (screen-space). */
  corner: number;
  startBbox: [number, number, number, number];
  startPdf: ScreenPoint;
}

export function ImageEditLayer({
  documentId,
  page,
  displayedWidth,
  displayedHeight,
  scale,
  rotation,
}: ImageEditLayerProps) {
  const activeTool = useToolStore((s) => s.activeTool);
  const setHistory = useHistoryStore((s) => s.setHistory);
  const bumpEpoch = useEditEpochStore((s) => s.bumpEpoch);
  const epoch = useDocEpoch(documentId);

  const [images, setImages] = useState<ImageInfo[]>([]);
  const [selected, setSelected] = useState<number | null>(null);
  const [preview, setPreview] = useState<[number, number, number, number] | null>(null);
  const dragRef = useRef<Drag | null>(null);

  const active = activeTool === "edit-image";

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
  const screenRect = (bbox: [number, number, number, number]): ScreenRect => {
    const tl = pdfToScreen({ page, x: bbox[0], y: bbox[3] }, geo);
    const br = pdfToScreen({ page, x: bbox[2], y: bbox[1] }, geo);
    return {
      left: Math.min(tl.x, br.x),
      top: Math.min(tl.y, br.y),
      width: Math.abs(br.x - tl.x),
      height: Math.abs(br.y - tl.y),
    };
  };

  useEffect(() => {
    if (!active) {
      setImages([]);
      setSelected(null);
      return;
    }
    let cancelled = false;
    extractImages(documentId, page)
      .then((r) => {
        if (!cancelled) setImages(r);
      })
      .catch((err: unknown) => console.warn("extract images failed", documentId, page, err));
    return () => {
      cancelled = true;
    };
  }, [active, documentId, page, epoch]);

  const layerPoint = (e: ReactPointerEvent<Element>): ScreenPoint => {
    const r = e.currentTarget.getBoundingClientRect();
    return { x: e.clientX - r.left, y: e.clientY - r.top };
  };

  const selectedImage = images.find((i) => i.index === selected) ?? null;

  const applyHistory = (h: Awaited<ReturnType<typeof transformImage>>) => {
    bumpEpoch(documentId);
    setHistory(documentId, h);
  };

  const commit = (matrix: Parameters<typeof transformImage>[3]) => {
    if (selected === null) return;
    transformImage(documentId, page, selected, matrix)
      .then(applyHistory)
      .catch((err: unknown) => console.warn("transform image failed", documentId, err));
  };

  // SPEC: P4-EDIT-006 (P4.C2b) — pick a new PNG/JPEG and swap the image's pixels
  // (placement preserved).
  const replace = () => {
    if (selected === null) return;
    const idx = selected;
    void openDialog({
      multiple: false,
      directory: false,
      filters: [{ name: "Image", extensions: ["png", "jpg", "jpeg"] }],
    })
      .then((picked) =>
        typeof picked === "string" ? replaceImage(documentId, page, idx, picked).then(applyHistory) : undefined,
      )
      .catch((err: unknown) => console.warn("replace image failed", documentId, err));
  };

  const startDrag = (kind: "move" | "resize", corner: number) => (e: ReactPointerEvent<HTMLDivElement>) => {
    if (!selectedImage) return;
    e.stopPropagation();
    e.currentTarget.setPointerCapture?.(e.pointerId);
    dragRef.current = {
      kind,
      corner,
      startBbox: selectedImage.bbox,
      startPdf: screenToPdf(layerPoint(e), geo),
    };
    setPreview(selectedImage.bbox);
  };

  const onPointerMove = (e: ReactPointerEvent<HTMLDivElement>) => {
    const drag = dragRef.current;
    if (!drag) return;
    const now = screenToPdf(layerPoint(e), geo);
    const dx = now.x - drag.startPdf.x;
    const dy = now.y - drag.startPdf.y;
    const [x0, y0, x1, y1] = drag.startBbox;
    if (drag.kind === "move") {
      setPreview([x0 + dx, y0 + dy, x1 + dx, y1 + dy]);
    } else {
      // Move the dragged corner; the opposite corner stays put. (PDF y is up, so
      // screen TL/BL map to the larger y; the corner index is screen-oriented.)
      const left = drag.corner === 0 || drag.corner === 3;
      const topScreen = drag.corner === 0 || drag.corner === 1;
      const nx0 = left ? x0 + dx : x0;
      const nx1 = left ? x1 : x1 + dx;
      const ny1 = topScreen ? y1 + dy : y1; // screen-top = PDF-top (y1)
      const ny0 = topScreen ? y0 : y0 + dy;
      setPreview([Math.min(nx0, nx1), Math.min(ny0, ny1), Math.max(nx0, nx1), Math.max(ny0, ny1)]);
    }
  };

  const onPointerUp = () => {
    const drag = dragRef.current;
    dragRef.current = null;
    if (!drag || !selectedImage || !preview) {
      setPreview(null);
      return;
    }
    const [nx0, ny0, nx1, ny1] = preview;
    setPreview(null);
    if (drag.kind === "move") {
      // Preserve any rotation/scale: translate the original matrix by the delta.
      const [a, b, c, d, e, f] = selectedImage.matrix;
      commit([a, b, c, d, e + (nx0 - selectedImage.bbox[0]), f + (ny0 - selectedImage.bbox[1])]);
    } else if (nx1 - nx0 > 1 && ny1 - ny0 > 1) {
      commit(rectToMatrix(nx0, ny0, nx1, ny1));
    }
  };

  if (!active) return null;

  const box = preview ?? selectedImage?.bbox ?? null;
  const boxRect = box ? screenRect(box) : null;
  const handleSize = 10;

  return (
    <div
      className="absolute left-0 top-0"
      style={{ width: cssWidth, height: cssHeight, pointerEvents: "none" }}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
    >
      {/* Hit-zones to select an unselected image. */}
      {selected === null
        ? images.map((img) => {
            const r = screenRect(img.bbox);
            return (
              <div
                key={img.index}
                className="absolute hover:bg-blue-400/10"
                title="Click to edit this image"
                onClick={() => setSelected(img.index)}
                style={{ left: r.left, top: r.top, width: r.width, height: r.height, pointerEvents: "auto", cursor: "pointer" }}
              />
            );
          })
        : null}

      {selectedImage && boxRect ? (
        <>
          {/* Selection box body — drag to move. */}
          <div
            className="absolute border-2 border-blue-500"
            onPointerDown={startDrag("move", -1)}
            style={{ left: boxRect.left, top: boxRect.top, width: boxRect.width, height: boxRect.height, pointerEvents: "auto", cursor: "move", background: "rgba(37,99,235,0.06)" }}
          />
          {/* Corner resize handles (screen order TL, TR, BR, BL). */}
          {[
            [boxRect.left, boxRect.top],
            [boxRect.left + boxRect.width, boxRect.top],
            [boxRect.left + boxRect.width, boxRect.top + boxRect.height],
            [boxRect.left, boxRect.top + boxRect.height],
          ].map(([hx, hy], corner) => (
            <div
              key={corner}
              aria-label={`Resize handle ${corner}`}
              onPointerDown={startDrag("resize", corner)}
              className="absolute rounded-sm border border-blue-700 bg-white"
              style={{
                left: hx - handleSize / 2,
                top: hy - handleSize / 2,
                width: handleSize,
                height: handleSize,
                pointerEvents: "auto",
                cursor: corner === 1 || corner === 3 ? "nesw-resize" : "nwse-resize",
              }}
            />
          ))}
          {/* Floating controls above the box. */}
          <div
            className="absolute flex gap-1"
            style={{ left: boxRect.left, top: boxRect.top - 28, pointerEvents: "auto" }}
          >
            <button
              type="button"
              onClick={() => commit(rotate90(selectedImage.matrix))}
              aria-label="Rotate image 90 degrees"
              className="rounded bg-neutral-200 px-2 py-0.5 text-xs hover:bg-neutral-300"
            >
              ⟳ 90°
            </button>
            <button
              type="button"
              onClick={replace}
              aria-label="Replace image"
              className="rounded bg-neutral-200 px-2 py-0.5 text-xs hover:bg-neutral-300"
            >
              Replace
            </button>
            <button
              type="button"
              onClick={() => {
                const idx = selectedImage.index;
                setSelected(null);
                deleteImage(documentId, page, idx)
                  .then((h) => {
                    bumpEpoch(documentId);
                    setHistory(documentId, h);
                  })
                  .catch((err: unknown) => console.warn("delete image failed", documentId, err));
              }}
              aria-label="Delete image"
              className="rounded bg-red-100 px-2 py-0.5 text-xs text-red-700 hover:bg-red-200"
            >
              Delete
            </button>
            <button
              type="button"
              onClick={() => setSelected(null)}
              aria-label="Deselect image"
              className="rounded bg-neutral-200 px-2 py-0.5 text-xs hover:bg-neutral-300"
            >
              Done
            </button>
          </div>
        </>
      ) : null}
    </div>
  );
}
