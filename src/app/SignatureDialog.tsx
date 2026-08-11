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
import { availableFonts, canvasMeasurer, type FontCandidate } from "@/tools/signature/fonts";
import { strokesToPng, textToPng } from "@/tools/signature/raster";
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

  const [mode, setMode] = useState<"draw" | "type">("draw");
  const [strokes, setStrokes] = useState<Stroke[]>([]);
  const [typed, setTyped] = useState("");
  const [family, setFamily] = useState<string | null>(null);
  const [fonts, setFonts] = useState<FontCandidate[]>([]);
  const [saving, setSaving] = useState(false);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const drawing = useRef(false);

  // Load the library whenever the dialog opens, so a signature saved in a
  // previous session (or a previous open) shows up.
  useEffect(() => {
    if (open) void refresh();
  }, [open, refresh]);

  // Which families this machine can render *this text* in. Re-run as the text
  // changes: a Latin-only script face is available for "Ada" and not for a name
  // in another script, and the picker should say so. Before anything is typed,
  // probe with a stand-in so the menu is populated on arrival.
  useEffect(() => {
    if (!open || mode !== "type") return;
    const found = availableFonts(typed.trim() || "Signature", canvasMeasurer());
    setFonts(found);
    setFamily((current) =>
      current && found.some((f) => f.family === current) ? current : (found[0]?.family ?? null),
    );
  }, [open, mode, typed]);

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

  // Takes values, not the event. A helper that accepts the event can be called
  // from inside a state updater — which runs during render, after React has
  // nulled `currentTarget` — and that is exactly the crash this shipped with.
  // Requiring the caller to have already read the DOM makes that impossible.
  const sample = (
    origin: { left: number; top: number },
    clientX: number,
    clientY: number,
    pressure: number,
  ): InkPoint => ({
    x: clientX - origin.left,
    y: clientY - origin.top,
    pressure: pressure || 0.5,
  });

  const onDown = (e: ReactPointerEvent<HTMLCanvasElement>) => {
    // Capture so a stroke that wanders off the pad still lands here, and still
    // ends — without it a pointerup outside the element is never seen.
    e.currentTarget.setPointerCapture?.(e.pointerId);
    drawing.current = true;
    // Read the event BEFORE handing a closure to setState. React nulls
    // `currentTarget` as soon as the handler returns, and a state updater runs
    // later, during render — so `sample(e)` inside the updater dereferences
    // null and takes the whole tree down. jsdom does not reproduce this (the
    // flush timing differs), which is exactly why it reached the app.
    const p = sample(e.currentTarget.getBoundingClientRect(), e.clientX, e.clientY, e.pressure);
    setStrokes((prev) => [...prev, [p]]);
  };

  const onMove = (e: ReactPointerEvent<HTMLCanvasElement>) => {
    if (!drawing.current) return;
    const p = sample(e.currentTarget.getBoundingClientRect(), e.clientX, e.clientY, e.pressure);
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

  const clear = () => (mode === "draw" ? setStrokes([]) : setTyped(""));

  const save = () => {
    void (async () => {
      setSaving(true);
      try {
        if (mode === "draw") {
          await add("draw", await strokesToPng(strokes));
          setStrokes([]);
        } else {
          await add("type", await textToPng(typed, family ? { family } : {}));
          setTyped("");
        }
        // Clearing the input is the signal that the save landed.
      } catch (err) {
        // Leave the input alone — a failed save must not lose the work.
        reportError("Couldn't save the signature", err);
      } finally {
        setSaving(false);
      }
    })();
  };

  const canSave = (mode === "draw" ? hasInk(strokes) : typed.trim().length > 0) && !saving;
  // Switching modes keeps both drafts; only an explicit Clear discards one.
  const clearable = mode === "draw" ? hasInk(strokes) : typed.length > 0;

  return (
    <div
      role="dialog"
      aria-label="Signatures"
      className="fixed inset-0 z-40 flex items-center justify-center bg-black/40"
    >
      <div className="w-[560px] rounded-lg bg-white p-4 shadow-xl dark:bg-neutral-900">
        <div className="mb-3 flex items-center gap-1" role="tablist" aria-label="Signature source">
          {(["draw", "type"] as const).map((m) => (
            <button
              key={m}
              type="button"
              role="tab"
              aria-selected={mode === m}
              onClick={() => setMode(m)}
              className={
                "rounded px-2 py-1 text-xs capitalize " +
                (mode === m
                  ? "bg-blue-600 text-white"
                  : "border border-neutral-300 hover:bg-neutral-100 dark:border-neutral-700")
              }
            >
              {m}
            </button>
          ))}
        </div>

        {mode === "type" ? (
          <div className="flex flex-col gap-2">
            <label className="flex flex-col gap-0.5">
              <span className="text-xs text-neutral-500">Your name</span>
              <input
                aria-label="Signature text"
                autoCapitalize="off"
                autoCorrect="off"
                spellCheck={false}
                maxLength={60}
                value={typed}
                onChange={(e) => setTyped(e.target.value)}
                className="w-full rounded border border-neutral-300 px-2 py-1 text-sm dark:border-neutral-700 dark:bg-neutral-900"
              />
            </label>
            <label className="flex flex-col gap-0.5">
              <span className="text-xs text-neutral-500">Handwriting font</span>
              <select
                aria-label="Handwriting font"
                value={family ?? ""}
                onChange={(e) => setFamily(e.target.value)}
                className="w-full rounded border border-neutral-300 px-2 py-1 text-sm dark:border-neutral-700 dark:bg-neutral-900"
              >
                {fonts.map((f) => (
                  <option key={f.family} value={f.family}>
                    {f.label}
                  </option>
                ))}
              </select>
            </label>
            {fonts.length === 1 && fonts[0]?.family === "cursive" ? (
              <p className="text-xs text-amber-700 dark:text-amber-500">
                No handwriting fonts were found on this machine, so the system default is
                being used.
              </p>
            ) : null}
            {/* Preview in the chosen family. The saved PNG is rendered
                independently at 600px, so this is for choosing, not fidelity. */}
            <div
              aria-label="Signature preview"
              style={{ fontFamily: family ? `"${family}", cursive` : "cursive" }}
              className="flex h-[120px] items-center justify-center overflow-hidden rounded border border-dashed border-neutral-400 bg-white text-4xl text-neutral-900"
            >
              {typed.trim() || <span className="text-base text-neutral-400">Type your name</span>}
            </div>
          </div>
        ) : null}

        <canvas
          ref={canvasRef}
          width={PAD_W}
          height={PAD_H}
          aria-label="Signature pad"
          onPointerDown={onDown}
          onPointerMove={onMove}
          onPointerUp={onUp}
          onPointerCancel={onUp}
          className={
            "w-full cursor-crosshair rounded border border-dashed border-neutral-400 bg-white touch-none dark:bg-neutral-100 " +
            (mode === "draw" ? "" : "hidden")
          }
        />

        <div className="mt-3 flex items-center gap-2">
          <button
            type="button"
            onClick={clear}
            disabled={!clearable}
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
            <ul aria-label="Saved signatures" className="flex flex-col gap-0.5 text-xs">
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
