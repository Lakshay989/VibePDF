// SPEC: P3-ANN-006 (P3.C3a) — the per-page stamp placement overlay.
//
// When the stamp tool is active AND a stamp is armed (chosen in the palette), a
// click drops it centred on the click point at a default size; the actor writes
// a `/Stamp` with a generated `/AP` and the canvas renders it. Self-contained
// like the note / polygon / ink layers — a single click-to-place gesture that
// doesn't fit `stepTool`'s drag lifecycle.

import { reportError } from "@/app/report-error";
import { type PointerEvent as ReactPointerEvent, useEffect, useState } from "react";

import { ask } from "@tauri-apps/plugin-dialog";

import { readPageFields, type PageField } from "@/ipc/forms";
import { placeSignature, signatureBytes } from "@/ipc/signatures";
import { addImageStamp, addStamp } from "@/ipc/stamps";
import { useEditEpochStore } from "@/state/edit-epoch-store";
import { useHistoryStore } from "@/state/history-store";
import { useOptimisticEditStore, usePendingEdits } from "@/state/optimistic-edit-store";
import { useStampStore } from "@/state/stamp-store";
import { useToolStore } from "@/state/tool-store";
import { type PageGeometry, pdfToScreen, screenToPdf } from "@/tools/_framework";
import {
  hasSeenPictureWarning,
  notePictureWarningSeen,
  pictureWarning,
  SIGNATURE_HEIGHT,
  signatureFieldAt,
} from "@/tools/signature/place";
import { IMAGE_STAMP_HEIGHT, stampRectAt, usesStampLayer } from "@/tools/stamp/stamps";
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
  // signature does not switch the user into the rubber-stamp tool. The mode and
  // the armed kind must agree, or an armed rubber stamp would drop while the
  // user believes they are placing a signature.
  //
  // `usesStampLayer` is shared with the toolbar's disarm-on-leave effect. They
  // were separate once, and the day they disagreed the toolbar wiped every
  // freshly armed signature.
  const active =
    armed !== null &&
    usesStampLayer(activeTool) &&
    (armed.kind === "signature") === (activeTool === "signature");

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

  // SPEC: P6-SEC-004 (P6.A5a) — mark the signature fields while a signature is
  // armed, so that what happens there is known before the click rather than
  // discovered after it.
  //
  // Placing on one is allowed; it is where the form asks you to sign. What it
  // is *not* is a digital signature, and that is the thing worth saying in
  // advance. An earlier version refused outright and explained in a toast,
  // which went unnoticed and read as a broken feature.
  //
  // Advisory only. The click handler re-reads the fields and decides for
  // itself, so a stale outline can never change what actually happens.
  const [signatureFields, setSignatureFields] = useState<PageField[]>([]);
  const armedSignature = active && armed?.kind === "signature";

  useEffect(() => {
    if (!armedSignature) {
      setSignatureFields([]);
      return undefined;
    }
    let cancelled = false;
    void readPageFields(documentId, page)
      .then((fields) => {
        if (!cancelled) setSignatureFields(fields.filter((f) => f.kind === "signature"));
      })
      .catch(() => {
        // No form on this page. Nothing to warn about.
        if (!cancelled) setSignatureFields([]);
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
        // Aiming at a /Sig field is allowed — it is where the form asks you to
        // sign — but it must not be mistaken for signing. Warned once per run,
        // through a modal rather than a toast, because a toast is exactly what
        // went unnoticed when this used to refuse outright.
        //
        // The two failures here are not alike, and wrapping both in one `try`
        // was a bug: a page with no form is ordinary and placement should carry
        // on, but a warning that *could not be shown* must not be followed by
        // doing the thing it was meant to warn about. The first version of this
        // swallowed a rejected `ask` and placed anyway — silently, which is the
        // worst of both.
        let hit: PageField | null = null;
        try {
          hit = signatureFieldAt(await readPageFields(documentId, page), pdf.x, pdf.y);
        } catch {
          // No form on this page. Nothing to warn about; carry on.
        }
        if (hit && !hasSeenPictureWarning()) {
          let go: boolean;
          try {
            go = await ask(pictureWarning(hit.name), {
              title: "Not a digital signature",
              kind: "warning",
              okLabel: "Place picture",
              cancelLabel: "Cancel",
            });
          } catch (err) {
            reportError("Couldn't show the signature-field warning", err);
            return;
          }
          if (!go) return;
          notePictureWarningSeen();
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
      {signatureFields.map((f) => {
        const r = stampScreenRect(f.rect);
        return (
          <div
            key={`sigfield-${f.name}`}
            aria-label={`Signature field ${f.name} — places a picture, not a signature`}
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
                Picture, not signed
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
