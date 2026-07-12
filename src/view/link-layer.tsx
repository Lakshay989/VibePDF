// SPEC: P4-EDIT-007 (P4.C3) — the per-page "add hyperlink" overlay.
//
// When the Add Link tool is active, drag a box on the page; on release a small
// popover asks for the target (Web URL / Email / Page / Named destination). On
// confirm the region becomes a `/Link` annotation via `addLink`. The link is an
// invisible hot-zone — nothing is painted into the page. Pointer events, not
// HTML5 DnD (WKWebView; docs/04).

import { reportError } from "@/app/report-error";
import { type PointerEvent as ReactPointerEvent, useState } from "react";

import { addLink, type LinkRect } from "@/ipc/links";
import { useEditEpochStore } from "@/state/edit-epoch-store";
import { useHistoryStore } from "@/state/history-store";
import { useToolStore } from "@/state/tool-store";
import { type PageGeometry, type ScreenPoint, screenToPdf } from "@/tools/_framework";
import { normalizeScreenRect, withDefaultSize } from "@/tools/free-text/free-text";
import {
  DEFAULT_LINK_COLOR,
  DEFAULT_LINK_STYLE,
  LINK_KIND_LABELS,
  LINK_STYLE_LABELS,
  type LinkKind,
  type LinkStyle,
  type LinkTarget,
  toWireValue,
  validateTarget,
} from "@/tools/link/target";

export interface LinkLayerProps {
  documentId: string;
  /** 0-based page index. */
  page: number;
  /** Total pages in the document (bounds a "page" target). */
  pageCount: number;
  /** Displayed (rotation-swapped) page size in PDF points. */
  displayedWidth: number;
  displayedHeight: number;
  /** CSS px per point. */
  scale: number;
  /** Page display rotation in degrees. */
  rotation: number;
}

/** A drawn region awaiting a target choice: PDF-points rect + screen-px box. */
interface Pending {
  rect: LinkRect;
  box: { left: number; top: number; width: number; height: number };
}

export function LinkLayer({
  documentId,
  page,
  pageCount,
  displayedWidth,
  displayedHeight,
  scale,
  rotation,
}: LinkLayerProps) {
  const activeTool = useToolStore((s) => s.activeTool);
  const setActiveTool = useToolStore((s) => s.setActiveTool);
  const setHistory = useHistoryStore((s) => s.setHistory);
  const bumpEpoch = useEditEpochStore((s) => s.bumpEpoch);

  const [start, setStart] = useState<ScreenPoint | null>(null);
  const [current, setCurrent] = useState<ScreenPoint | null>(null);
  const [pending, setPending] = useState<Pending | null>(null);
  const [target, setTarget] = useState<LinkTarget>({ kind: "url", value: "" });
  const [style, setStyle] = useState<LinkStyle>(DEFAULT_LINK_STYLE);
  const [color, setColor] = useState<string>(DEFAULT_LINK_COLOR);

  const placing = activeTool === "add-link";

  // `coords` wants the UNROTATED PDF dimensions; swap back for 90°/270°.
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

  const layerPoint = (e: ReactPointerEvent<Element>): ScreenPoint => {
    const r = e.currentTarget.getBoundingClientRect();
    return { x: e.clientX - r.left, y: e.clientY - r.top };
  };

  const onPointerDown = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (!placing || pending) return;
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
    if (!start) return;
    const box = withDefaultSize(normalizeScreenRect(start, layerPoint(e)));
    setStart(null);
    setCurrent(null);
    const a = screenToPdf({ x: box.left, y: box.top }, geo);
    const b = screenToPdf({ x: box.left + box.width, y: box.top + box.height }, geo);
    const rect: LinkRect = [
      Math.min(a.x, b.x),
      Math.min(a.y, b.y),
      Math.max(a.x, b.x),
      Math.max(a.y, b.y),
    ];
    setTarget({ kind: "url", value: "" });
    setPending({ rect, box });
  };

  const cancel = () => {
    setPending(null);
    setTarget({ kind: "url", value: "" });
  };

  const confirm = () => {
    if (!pending) return;
    if (!validateTarget(target, pageCount).ok) return;
    const { rect } = pending;
    const kind = target.kind;
    const value = toWireValue(target);
    const chosenStyle = style;
    const chosenColor = color;
    cancel();
    setActiveTool(null);
    addLink(documentId, page, rect, kind, value, chosenStyle, chosenColor)
      .then((h) => {
        bumpEpoch(documentId);
        setHistory(documentId, h);
      })
      .catch((err: unknown) => reportError("Couldn't add link", err));
  };

  if (!placing) return null;

  const preview = start && current ? normalizeScreenRect(start, current) : null;
  const validation = validateTarget(target, pageCount);
  const placeholder =
    target.kind === "url"
      ? "https://example.com"
      : target.kind === "email"
        ? "name@example.com"
        : target.kind === "page"
          ? `1–${pageCount}`
          : "destination name";

  return (
    <div
      className="absolute left-0 top-0"
      style={{
        width: cssWidth,
        height: cssHeight,
        pointerEvents: "auto",
        cursor: pending ? "default" : "crosshair",
      }}
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

      {pending ? (
        <>
          <div
            className="absolute border-2 border-blue-500 bg-blue-400/10"
            style={{
              left: pending.box.left,
              top: pending.box.top,
              width: pending.box.width,
              height: pending.box.height,
            }}
          />
          <div
            className="absolute z-10 flex w-64 flex-col gap-2 rounded border border-neutral-300 bg-white p-3 text-sm shadow-lg"
            style={{
              left: Math.min(pending.box.left, cssWidth - 256),
              top: pending.box.top + pending.box.height + 6,
            }}
            // Keep clicks inside the popover from starting a new drag.
            onPointerDown={(e) => e.stopPropagation()}
          >
            <select
              className="rounded border border-neutral-300 px-2 py-1"
              value={target.kind}
              onChange={(e) => setTarget({ kind: e.target.value as LinkKind, value: "" })}
            >
              {(Object.keys(LINK_KIND_LABELS) as LinkKind[]).map((k) => (
                <option key={k} value={k}>
                  {LINK_KIND_LABELS[k]}
                </option>
              ))}
            </select>
            <input
              autoFocus
              className="rounded border border-neutral-300 px-2 py-1"
              placeholder={placeholder}
              value={target.value}
              onChange={(e) => setTarget({ ...target, value: e.target.value })}
              onKeyDown={(e) => {
                if (e.key === "Enter") confirm();
                if (e.key === "Escape") cancel();
              }}
            />
            {!validation.ok && target.value.trim() !== "" ? (
              <p className="text-xs text-red-600">{validation.reason}</p>
            ) : null}
            <div className="flex items-center gap-2">
              <select
                className="flex-1 rounded border border-neutral-300 px-2 py-1"
                aria-label="Link appearance"
                value={style}
                onChange={(e) => setStyle(e.target.value as LinkStyle)}
              >
                {(Object.keys(LINK_STYLE_LABELS) as LinkStyle[]).map((s) => (
                  <option key={s} value={s}>
                    {LINK_STYLE_LABELS[s]}
                  </option>
                ))}
              </select>
              {style !== "invisible" ? (
                <input
                  type="color"
                  aria-label="Link color"
                  className="h-7 w-9 rounded border border-neutral-300"
                  value={color}
                  onChange={(e) => setColor(e.target.value)}
                />
              ) : null}
            </div>
            <div className="flex justify-end gap-2">
              <button
                type="button"
                className="rounded px-2 py-1 text-neutral-600 hover:bg-neutral-100"
                onClick={cancel}
              >
                Cancel
              </button>
              <button
                type="button"
                className="rounded bg-blue-600 px-2 py-1 text-white disabled:opacity-40"
                disabled={!validation.ok}
                onClick={confirm}
              >
                Add link
              </button>
            </div>
          </div>
        </>
      ) : null}
    </div>
  );
}
