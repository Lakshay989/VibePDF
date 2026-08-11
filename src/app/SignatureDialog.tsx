// SPEC: P6-SEC-001 (P6.A2) — draw a signature and save it to the library.
//
// A pad you draw on, Clear, and Save. Pointer capture and raw-sample capture
// follow `src/view/ink-layer.tsx`; the samples are smoothed by the same
// `smoothInk` on the way to a PNG (`tools/signature/raster.ts`), which is what
// the library stores (P6.A1).
//
// Structured so A3 (typed) and A4 (image) can be added as sibling modes without
// reworking this: the pad is one branch, and everything around it — the library
// list, Save, error handling — is mode-agnostic. Both have since landed, and
// the structure held.
//
// The UI here is deliberately plain; it is a working surface for A5 rather
// than a finished design.

import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
import { readFile } from "@tauri-apps/plugin-fs";
import { type PointerEvent as ReactPointerEvent, useEffect, useRef, useState } from "react";

import { basename } from "@/app/paths";
import { reportError } from "@/app/report-error";
import type { InkPoint } from "@/tools/ink/ink";
import { hasInk, type Stroke } from "@/tools/signature/draw";
import { availableFonts, canvasMeasurer, type FontCandidate } from "@/tools/signature/fonts";
import { imageToPng, strokesToPng, textToPng, type ImageRaster } from "@/tools/signature/raster";
import { useSignatureStore } from "@/state/signature-store";

/** Pad size in CSS pixels. The stored PNG is rasterised independently at a
 *  fixed long edge, so this is a comfort choice, not a quality one. */
const PAD_W = 480;
const PAD_H = 180;

/**
 * PNG bytes → a `data:` URL for the preview.
 *
 * Chunked because `String.fromCharCode(...bytes)` spreads every byte into the
 * argument list, which overflows the stack well below the size of a 600px
 * signature. A data URL rather than an object URL for the reason
 * `view/file-data-url.ts` already gives: nothing to revoke.
 */
function pngDataUrl(bytes: Uint8Array): string {
  const CHUNK = 0x8000;
  let binary = "";
  for (let i = 0; i < bytes.length; i += CHUNK) {
    binary += String.fromCharCode(...bytes.subarray(i, i + CHUNK));
  }
  return `data:image/png;base64,${btoa(binary)}`;
}

interface Props {
  open: boolean;
  onClose: () => void;
}

export function SignatureDialog({ open, onClose }: Props) {
  const entries = useSignatureStore((s) => s.entries);
  const refresh = useSignatureStore((s) => s.refresh);
  const add = useSignatureStore((s) => s.add);

  const [mode, setMode] = useState<"draw" | "type" | "image">("draw");
  const [strokes, setStrokes] = useState<Stroke[]>([]);
  const [typed, setTyped] = useState("");
  const [family, setFamily] = useState<string | null>(null);
  const [fonts, setFonts] = useState<FontCandidate[]>([]);
  const [saving, setSaving] = useState(false);
  // SPEC: P6-SEC-003 (P6.A4) — the imported file, what the threshold made of
  // it, and why it failed if it did.
  const [picked, setPicked] = useState<{ name: string; bytes: Uint8Array } | null>(null);
  const [strength, setStrength] = useState(0);
  const [imaged, setImaged] = useState<ImageRaster | null>(null);
  const [preview, setPreview] = useState<string | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [working, setWorking] = useState(false);
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

  // SPEC: P6-SEC-003 (P6.A4) — re-run the threshold whenever the file or the
  // slider changes. What this produces is the exact PNG that Save stores, so
  // the preview is the artifact rather than an impression of it.
  //
  // Failures land inline rather than in a toast: dragging the slider to the top
  // erases the whole image, which is a legitimate thing to do on the way to a
  // good value, and toasting it would fire on every tick of the drag.
  useEffect(() => {
    if (mode !== "image" || !picked) return undefined;
    let cancelled = false;
    setWorking(true);
    void imageToPng(picked.bytes, { strength })
      .then((res) => {
        if (cancelled) return;
        setImaged(res);
        setPreview(pngDataUrl(res.png));
        setProblem(null);
      })
      .catch((err: unknown) => {
        if (cancelled) return;
        // The file stays selected. Re-picking the same image to undo a slider
        // drag would be a ridiculous thing to ask of anyone.
        setImaged(null);
        setPreview(null);
        setProblem(err instanceof Error ? err.message : "that image could not be read");
      })
      .finally(() => {
        if (!cancelled) setWorking(false);
      });
    return () => {
      cancelled = true;
    };
  }, [mode, picked, strength]);

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

  const clearImage = () => {
    setPicked(null);
    setImaged(null);
    setPreview(null);
    setProblem(null);
  };

  // SPEC: P6-SEC-003 (P6.A4) — the three formats the spec names. The engine
  // sniffs the actual bytes when decoding, so this filter is only about which
  // files the picker offers.
  const pickImage = () => {
    void openFileDialog({
      multiple: false,
      directory: false,
      filters: [{ name: "Image", extensions: ["png", "jpg", "jpeg", "bmp"] }],
    })
      .then(async (path) => {
        if (typeof path !== "string") return;
        setPicked({ name: basename(path), bytes: await readFile(path) });
      })
      .catch((err: unknown) => reportError("Couldn't open that image", err));
  };

  const clear = () => {
    if (mode === "draw") setStrokes([]);
    else if (mode === "type") setTyped("");
    else clearImage();
  };

  const save = () => {
    void (async () => {
      setSaving(true);
      try {
        if (mode === "draw") {
          await add("draw", await strokesToPng(strokes));
          setStrokes([]);
        } else if (mode === "type") {
          await add("type", await textToPng(typed, family ? { family } : {}));
          setTyped("");
        } else if (imaged) {
          // Already encoded by the preview effect — what was on screen is
          // byte-for-byte what goes into the library.
          await add("image", imaged.png);
          clearImage();
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

  const ready =
    mode === "draw"
      ? hasInk(strokes)
      : mode === "type"
        ? typed.trim().length > 0
        : imaged !== null && !working;
  const canSave = ready && !saving;
  // Switching modes keeps every draft; only an explicit Clear discards one.
  const clearable =
    mode === "draw" ? hasInk(strokes) : mode === "type" ? typed.length > 0 : picked !== null;

  return (
    <div
      role="dialog"
      aria-label="Signatures"
      className="fixed inset-0 z-40 flex items-center justify-center bg-black/40"
    >
      <div className="w-[560px] rounded-lg bg-white p-4 shadow-xl dark:bg-neutral-900">
        <div className="mb-3 flex items-center gap-1" role="tablist" aria-label="Signature source">
          {(["draw", "type", "image"] as const).map((m) => (
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
              // Backspacing used to leave ghosts of the deleted glyphs, which
              // cleared as soon as the font changed. Script faces paint ink well
              // outside their layout box — Zapfino's swashes especially — and a
              // shrinking string invalidates the layout rect, not the larger ink
              // rect, so the overhang stayed on screen.
              //
              // `contain: paint` is the honest fix: the box already clips with
              // overflow-hidden, so promising the engine that nothing paints
              // outside it is true, and it makes the invalidation correct. The
              // key is the belt to that braces — any change to the text or the
              // family yields a fresh node, which is exactly the re-layout that
              // was observed to clear the artifact.
              key={`${family ?? "none"}:${typed}`}
              aria-label="Signature preview"
              style={{
                fontFamily: family ? `"${family}", cursive` : "cursive",
                contain: "paint",
              }}
              className="flex h-[120px] items-center justify-center overflow-hidden rounded border border-dashed border-neutral-400 bg-white text-4xl text-neutral-900"
            >
              {typed.trim() || <span className="text-base text-neutral-400">Type your name</span>}
            </div>
          </div>
        ) : null}

        {/* SPEC: P6-SEC-003 (P6.A4) — import an image and lift its background. */}
        {mode === "image" ? (
          <div className="flex flex-col gap-2">
            <div className="flex items-center gap-2">
              <button
                type="button"
                onClick={pickImage}
                className="rounded border border-neutral-300 px-2 py-1 text-xs dark:border-neutral-700"
              >
                Choose image…
              </button>
              <span className="min-w-0 truncate text-xs text-neutral-500">
                {picked ? picked.name : "PNG, JPG or BMP"}
              </span>
            </div>
            <label className="flex flex-col gap-0.5">
              <span className="text-xs text-neutral-500">Background removal — {strength}%</span>
              <input
                type="range"
                min={0}
                max={100}
                step={1}
                aria-label="Background removal"
                value={strength}
                disabled={!picked}
                onChange={(e) => setStrength(Number(e.target.value))}
                className="w-full disabled:opacity-40"
              />
            </label>
            {imaged && imaged.transparent === 0 ? (
              <p className="text-xs text-amber-700 dark:text-amber-500">
                Nothing in this image is transparent, so it will be placed as a solid rectangle.
                Raise Background removal to drop the paper out from behind the ink.
              </p>
            ) : null}
            {problem ? <p className="text-xs text-red-600 dark:text-red-400">{problem}</p> : null}
            {/* On a checkerboard, because a transparent PNG against a white
                panel looks exactly like a white one — and against a dark
                viewer it looks black, which is the confusion A2 ran into. */}
            <div
              aria-label="Imported signature preview"
              style={{
                backgroundImage:
                  "linear-gradient(45deg,#ccc 25%,transparent 25%)," +
                  "linear-gradient(-45deg,#ccc 25%,transparent 25%)," +
                  "linear-gradient(45deg,transparent 75%,#ccc 75%)," +
                  "linear-gradient(-45deg,transparent 75%,#ccc 75%)",
                backgroundSize: "16px 16px",
                backgroundPosition: "0 0, 0 8px, 8px -8px, -8px 0",
              }}
              className="flex h-[160px] items-center justify-center overflow-hidden rounded border border-dashed border-neutral-400"
            >
              {preview ? (
                <img src={preview} alt="" className="max-h-full max-w-full object-contain" />
              ) : (
                <span className="rounded bg-white/80 px-1 text-xs text-neutral-600">
                  {picked ? (working ? "Working…" : "Nothing to show") : "No image chosen"}
                </span>
              )}
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
