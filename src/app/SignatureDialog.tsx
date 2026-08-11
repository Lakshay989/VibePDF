// SPEC: P6-SEC-001 (P6.A2) — draw a signature and save it to the library.
//
// A pad you draw on, Clear, and Save. Pointer capture and raw-sample capture
// follow `src/view/ink-layer.tsx`; the samples are smoothed by the same
// `smoothInk` on the way to a PNG (`tools/signature/raster.ts`), which is what
// the library stores (P6.A1).
//
// Structured so A3 (typed) and A4 (image) can be added as sibling modes without
// reworking this: the pad is one branch, and everything around it — the library
// list, Save, error handling — is mode-agnostic.
//
// The UI here is deliberately plain; it is a working surface for A3–A5 rather
// than a finished design.

import { type PointerEvent as ReactPointerEvent, useEffect, useRef, useState } from "react";

import { reportError } from "@/app/report-error";
import type { InkPoint } from "@/tools/ink/ink";
import { hasInk, type Stroke } from "@/tools/signature/draw";
import { strokesToPng } from "@/tools/signature/raster";
import { useSignatureStore } from "@/state/signature-store";

/** Pad size in CSS pixels. The stored PNG is rasterised independently at a
 *  fixed long edge, so this is a comfort choice, not a quality one. */
const PAD_W = 480;
const PAD_H = 180;

interface Props {
  open: boolean;
  onClose: () => void;
}

export function SignatureDialog({ open, onClose }: Props) {
  const entries = useSignatureStore((s) => s.entries);
  const refresh = useSignatureStore((s) => s.refresh);
  const add = useSignatureStore((s) => s.add);

  const [strokes, setStrokes] = useState<Stroke[]>([]);
  const [saving, setSaving] = useState(false);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const drawing = useRef(false);

  // Load the library whenever the dialog opens, so a signature saved in a
  // previous session (or a previous open) shows up.
  useEffect(() => {
    if (open) void refresh();
  }, [open, refresh]);

  // Live preview. Redrawn from scratch on every change — a signature is a few
  // hundred points, so this is cheaper than tracking incremental damage.
  // `getContext` returns null under jsdom; the guard keeps tests mountable.
  useEffect(() => {
    const ctx = canvasRef.current?.getContext("2d");
    if (!ctx) return;
    ctx.clearRect(0, 0, PAD_W, PAD_H);
    ctx.lineCap = "round";
    ctx.lineJoin = "round";
    ctx.strokeStyle = "#111";
    ctx.lineWidth = 2;
    for (const stroke of strokes) {
      if (stroke.length === 0) continue;
      ctx.beginPath();
      ctx.moveTo(stroke[0]!.x, stroke[0]!.y);
      for (const p of stroke.slice(1)) ctx.lineTo(p.x, p.y);
      ctx.stroke();
    }
  }, [strokes]);

  if (!open) return null;

  const sample = (e: ReactPointerEvent<HTMLCanvasElement>): InkPoint => {
    const r = e.currentTarget.getBoundingClientRect();
    return { x: e.clientX - r.left, y: e.clientY - r.top, pressure: e.pressure || 0.5 };
  };

  const onDown = (e: ReactPointerEvent<HTMLCanvasElement>) => {
    // Capture so a stroke that wanders off the pad still lands here, and still
    // ends — without it a pointerup outside the element is never seen.
    e.currentTarget.setPointerCapture?.(e.pointerId);
    drawing.current = true;
    setStrokes((prev) => [...prev, [sample(e)]]);
  };

  const onMove = (e: ReactPointerEvent<HTMLCanvasElement>) => {
    if (!drawing.current) return;
    const p = sample(e);
    setStrokes((prev) => {
      const next = [...prev];
      const last = next[next.length - 1];
      if (last) next[next.length - 1] = [...last, p];
      return next;
    });
  };

  const onUp = () => {
    drawing.current = false;
  };

  const clear = () => setStrokes([]);

  const save = () => {
    void (async () => {
      setSaving(true);
      try {
        const png = await strokesToPng(strokes);
        await add("draw", png);
        // Keep the dialog open on the library view; clearing the pad is the
        // signal that the save landed.
        setStrokes([]);
      } catch (err) {
        // Leave the strokes alone — a failed save must not lose the drawing.
        reportError("Couldn't save the signature", err);
      } finally {
        setSaving(false);
      }
    })();
  };

  const canSave = hasInk(strokes) && !saving;

  return (
    <div
      role="dialog"
      aria-label="Signatures"
      className="fixed inset-0 z-40 flex items-center justify-center bg-black/40"
    >
      <div className="w-[560px] rounded-lg bg-white p-4 shadow-xl dark:bg-neutral-900">
        <h2 className="mb-2 text-sm font-semibold">Draw a signature</h2>

        <canvas
          ref={canvasRef}
          width={PAD_W}
          height={PAD_H}
          aria-label="Signature pad"
          onPointerDown={onDown}
          onPointerMove={onMove}
          onPointerUp={onUp}
          onPointerCancel={onUp}
          className="w-full cursor-crosshair rounded border border-dashed border-neutral-400 bg-white touch-none dark:bg-neutral-100"
        />

        <div className="mt-3 flex items-center gap-2">
          <button
            type="button"
            onClick={clear}
            disabled={!hasInk(strokes)}
            className="rounded border border-neutral-300 px-2 py-1 text-xs disabled:opacity-40 dark:border-neutral-700"
          >
            Clear
          </button>
          <button
            type="button"
            onClick={save}
            disabled={!canSave}
            className="rounded bg-blue-600 px-2 py-1 text-xs text-white disabled:opacity-40"
          >
            {saving ? "Saving…" : "Save to library"}
          </button>
          <button
            type="button"
            onClick={onClose}
            className="ml-auto rounded border border-neutral-300 px-2 py-1 text-xs dark:border-neutral-700"
          >
            Close
          </button>
        </div>

        <div className="mt-4 border-t border-neutral-200 pt-3 dark:border-neutral-800">
          <h3 className="mb-1 text-xs font-medium uppercase tracking-wide text-neutral-400">
            Saved signatures ({entries.length})
          </h3>
          {entries.length === 0 ? (
            <p className="text-xs text-neutral-500">None yet.</p>
          ) : (
            <ul className="flex flex-col gap-0.5 text-xs">
              {entries.map((e) => (
                <li key={e.id} className="flex items-center gap-2 text-neutral-600 dark:text-neutral-300">
                  <span className="rounded bg-neutral-100 px-1 dark:bg-neutral-800">{e.kind}</span>
                  <span className="tabular-nums text-neutral-400">
                    {new Date(e.createdAt).toLocaleString()}
                  </span>
                </li>
              ))}
            </ul>
          )}
        </div>
      </div>
    </div>
  );
}
