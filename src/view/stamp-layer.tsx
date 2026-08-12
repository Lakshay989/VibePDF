// SPEC: P3-ANN-006 (P3.C3a) — the per-page stamp placement overlay.
//
// When the stamp tool is active AND a stamp is armed (chosen in the palette), a
// click drops it centred on the click point at a default size; the actor writes
// a `/Stamp` with a generated `/AP` and the canvas renders it. Self-contained
// like the note / polygon / ink layers — a single click-to-place gesture that
// doesn't fit `stepTool`'s drag lifecycle.

import { reportError } from "@/app/report-error";
import { type PointerEvent as ReactPointerEvent, useEffect, useState } from "react";

import { readPageFields, type PageField } from "@/ipc/forms";
import { placeSignature, signatureBytes } from "@/ipc/signatures";
import { addImageStamp, addStamp } from "@/ipc/stamps";
import { useEditEpochStore } from "@/state/edit-epoch-store";
import { useHistoryStore } from "@/state/history-store";
import { useOptimisticEditStore, usePendingEdits } from "@/state/optimistic-edit-store";
import { useStampStore } from "@/state/stamp-store";
import { useToolStore } from "@/state/tool-store";
import { type PageGeometry, pdfToScreen, screenToPdf } from "@/tools/_framework";
import { declineMessage, SIGNATURE_HEIGHT, signatureFieldAt } from "@/tools/signature/place";
import { IMAGE_STAMP_HEIGHT, stampRectAt } from "@/tools/stamp/stamps";
import { bytesToDataUrl, fileToDataUrl, imageAspect } from "@/view/file-data-url";

/** Optimistic-preview payload: a committed stamp awaiting bake (P4.HF29). */
type StampHeld =
  | { variant: "text"; rect: [number, number, number, number]; label: string; color: string }
  | { variant: "image"; rect: [number, number, number, number]; src: string };

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
  // Soft bump: overlay is the display; no main-view reload until the next bake.
  const bumpEpoch = useEditEpochStore((s) => s.bumpEpochSoft);

  // Two modes share this layer. `"signature"` (P6.A5a) exists so that placing a
  // signature does not switch the user into the rubber-stamp tool; the kinds
  // must match the mode, or an armed rubber stamp would drop while the user
  // believes they are placing a signature.
  const active =
    armed !== null &&
    (activeTool === "signature" ? armed.kind === "signature" : activeTool === "stamp");

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

  // SPEC: P6-SEC-004 (P6.A5a) — show where a signature *cannot* go, before the
  // click rather than after it.
  //
  // The refusal on a `/Sig` field was correct from the first commit, and it was
  // still a bad experience: the ruled line labelled "Signature:" is the most
  // obvious place in the document to sign, and clicking it produced a toast
  // that did not register. A guard whose message arrives too late, or not at
  // all, is indistinguishable from a broken feature — it cost an hour of
  // someone's evening to establish that the code was working.
  //
  // Advisory only. The click handler re-reads the fields and decides for
  // itself, so a stale outline can never let a signature through.
  const [blockedFields, setBlockedFields] = useState<PageField[]>([]);
  const armedSignature = active && armed?.kind === "signature";

  useEffect(() => {
    if (!armedSignature) {
      setBlockedFields([]);
      return undefined;
    }
    let cancelled = false;
    void readPageFields(documentId, page)
      .then((fields) => {
        if (!cancelled) setBlockedFields(fields.filter((f) => f.kind === "signature"));
      })
      .catch(() => {
        // No form on this page. Nothing to warn about.
        if (!cancelled) setBlockedFields([]);
      });
    return () => {
      cancelled = true;
    };
  }, [armedSignature, documentId, page]);

  const onPointerDown = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (!active || !armed || e.button !== 0) return;
    const r = e.currentTarget.getBoundingClientRect();
    const pdf = screenToPdf({ x: e.clientX - r.left, y: e.clientY - r.top }, geo);
    const oe = useOptimisticEditStore.getState();
    const tie = (key: string) =>
      oe.tie(documentId, key, (useEditEpochStore.getState().bakeByDoc[documentId] ?? 0) + 1);

    // SPEC: P6-SEC-004 (P6.A5a) — a signature from the library. Same aspect-
    // correct centring as an image stamp; what differs is the guard in front of
    // it and that the backend resolves the bytes from an id.
    if (armed.kind === "signature") {
      const { signatureId } = armed;
      void (async () => {
        // The one thing that must not happen: a picture dropped over a /Sig
        // widget renders as a signature to every reader and carries none.
        // Declining is the honest answer until P6.B1 can actually sign.
        try {
          const hit = signatureFieldAt(await readPageFields(documentId, page), pdf.x, pdf.y);
          if (hit) {
            reportError("Can't place a signature there", new Error(declineMessage(hit.name)));
            return;
          }
        } catch {
          // No form, or the fields could not be read — there is no signature
          // field to collide with, so placing is safe.
        }

        let key: string | null = null;
        try {
          const src = bytesToDataUrl(await signatureBytes(signatureId));
          const aspect = await imageAspect(src);
          const h = SIGNATURE_HEIGHT;
          const w = h * aspect;
          const rect: [number, number, number, number] = [
            pdf.x - w / 2,
            pdf.y - h / 2,
            pdf.x + w / 2,
            pdf.y + h / 2,
          ];
          key = oe.add(documentId, page, "stamp", { variant: "image", rect, src });
        } catch {
          // No preview available; still place it below.
        }
        try {
          const done = await placeSignature(
            documentId,
            page,
            pdf.x,
            pdf.y,
            SIGNATURE_HEIGHT,
            signatureId,
            options.opacity,
          );
          bumpEpoch(documentId);
          if (key) tie(key);
          setHistory(documentId, done);
          // One signature, once. A rubber stamp stays armed because you stamp
          // a batch of pages with it; nobody signs the same document eight
          // times, and leaving the tool armed means the next stray click drops
          // a second signature.
          useStampStore.getState().arm(null);
          useToolStore.getState().setActiveTool(null);
        } catch (err) {
          if (key) oe.remove(documentId, key);
          reportError("Couldn't place the signature", err);
        }
      })();
      return;
    }

    // Image stamps place aspect-correct around the click (the backend derives the
    // rect from the image's ratio); we mirror that centring for the preview.
    if (armed.kind === "image") {
      const { imagePath } = armed;
      const label = armed.label ?? null;
      void (async () => {
        let key: string | null = null;
        try {
          const src = await fileToDataUrl(imagePath);
          const aspect = await imageAspect(src);
          const h = IMAGE_STAMP_HEIGHT;
          const w = h * aspect;
          const rect: [number, number, number, number] = [
            pdf.x - w / 2,
            pdf.y - h / 2,
            pdf.x + w / 2,
            pdf.y + h / 2,
          ];
          key = oe.add(documentId, page, "stamp", { variant: "image", rect, src });
        } catch {
          // No preview available; still place the stamp below.
        }
        try {
          const done = await addImageStamp(
            documentId,
            page,
            pdf.x,
            pdf.y,
            IMAGE_STAMP_HEIGHT,
            imagePath,
            label,
            options.opacity,
          );
          bumpEpoch(documentId);
          if (key) tie(key);
          setHistory(documentId, done);
        } catch (err) {
          if (key) oe.remove(documentId, key);
          reportError("Couldn't add stamp", err);
        }
      })();
      return;
    }

    // Text stamp: fixed-size box with a bold uppercase label.
    const rect = stampRectAt(pdf.x, pdf.y, geo.width, geo.height);
    const key = oe.add(documentId, page, "stamp", {
      variant: "text",
      rect: [rect[0], rect[1], rect[2], rect[3]],
      label: armed.label,
      color: armed.color,
    });
    addStamp(documentId, page, rect, armed.label, armed.name, armed.color, options.opacity)
      .then((h) => {
        bumpEpoch(documentId);
        tie(key);
        setHistory(documentId, h);
      })
      .catch((err: unknown) => {
        oe.remove(documentId, key);
        reportError("Couldn't add stamp", err);
      });
  };

  const pendingStamps = usePendingEdits<StampHeld>(documentId, page, "stamp");
  const stampScreenRect = (rect: [number, number, number, number]) => {
    const tl = pdfToScreen({ page, x: rect[0], y: rect[3] }, geo);
    const br = pdfToScreen({ page, x: rect[2], y: rect[1] }, geo);
    return {
      left: Math.min(tl.x, br.x),
      top: Math.min(tl.y, br.y),
      width: Math.abs(br.x - tl.x),
      height: Math.abs(br.y - tl.y),
    };
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
    >
      {/* SPEC: P6-SEC-004 (P6.A5a) — the signature fields, marked out while a
          signature is armed. `pointerEvents: none` so the click still reaches
          the layer and still gets the explanation; this only means the refusal
          is no longer a surprise. */}
      {blockedFields.map((f) => {
        const r = stampScreenRect(f.rect);
        return (
          <div
            key={`sigfield-${f.name}`}
            aria-label={`Signature field ${f.name} — needs a certificate`}
            className="absolute flex items-center justify-center rounded border-2 border-dashed border-amber-500 bg-amber-100/40"
            style={{
              left: r.left,
              top: r.top,
              width: r.width,
              height: r.height,
              pointerEvents: "none",
            }}
          >
            {/* Only when it fits — a small witness box would otherwise be all
                label and no box. */}
            {r.width > 110 && r.height > 16 ? (
              <span className="rounded bg-amber-500 px-1 text-[10px] leading-tight text-white">
                Needs a certificate
              </span>
            ) : null}
          </div>
        );
      })}

      {/* Optimistic preview: committed stamps not yet baked into the page (P4.HF29). */}
      {pendingStamps.map(({ key, data }) => {
        const r = stampScreenRect(data.rect);
        if (data.variant === "image") {
          return (
            <img
              key={key}
              src={data.src}
              alt=""
              draggable={false}
              style={{
                position: "absolute",
                left: r.left,
                top: r.top,
                width: r.width,
                height: r.height,
                objectFit: "fill",
                pointerEvents: "none",
              }}
            />
          );
        }
        return (
          <div
            key={key}
            className="absolute flex items-center justify-center rounded font-bold uppercase"
            style={{
              left: r.left,
              top: r.top,
              width: r.width,
              height: r.height,
              border: `2px solid ${data.color}`,
              color: data.color,
              fontSize: `${Math.max(8, r.height * 0.4)}px`,
              letterSpacing: "0.05em",
              pointerEvents: "none",
            }}
          >
            {data.label}
          </div>
        );
      })}
    </div>
  );
}
