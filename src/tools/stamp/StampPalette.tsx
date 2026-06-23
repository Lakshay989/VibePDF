// SPEC: P3-ANN-006 (P3.C3a) — the stamp picker shown in the toolbar while the
// Stamp tool is active. Pick a built-in stamp or type a custom label to *arm* it
// (into the stamp-store); the page then drops it on the next click.

import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
import { useState } from "react";

import { useStampStore } from "@/state/stamp-store";
import { BUILTIN_STAMPS, customStamp, imageStamp } from "@/tools/stamp/stamps";

export function StampPalette() {
  const armed = useStampStore((s) => s.armed);
  const arm = useStampStore((s) => s.arm);
  const [custom, setCustom] = useState("");

  const setCustomStamp = () => {
    const text = custom.trim();
    if (text) arm(customStamp(text));
  };

  // SPEC: P3-ANN-006 (P3.C3b) — pick a PNG to arm as an image stamp.
  const pickImage = async () => {
    const selected = await openFileDialog({
      multiple: false,
      filters: [{ name: "PNG image", extensions: ["png"] }],
    });
    if (selected === null || Array.isArray(selected)) return; // cancelled
    arm(imageStamp(selected));
  };

  return (
    <div className="flex items-center gap-1">
      {BUILTIN_STAMPS.map((s) => (
        <button
          key={s.name}
          type="button"
          onClick={() => arm(s)}
          aria-label={`Stamp: ${s.label}`}
          aria-pressed={armed?.label === s.label}
          title={s.label}
          style={{ color: s.color, borderColor: s.color }}
          className={
            "rounded border px-1.5 py-0.5 text-[10px] font-bold uppercase leading-none " +
            (armed?.label === s.label ? "ring-2 ring-blue-500 ring-offset-1" : "")
          }
        >
          {s.label}
        </button>
      ))}
      <input
        type="text"
        value={custom}
        onChange={(e) => setCustom(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") setCustomStamp();
        }}
        placeholder="Custom…"
        aria-label="Custom stamp text"
        className="w-24 rounded border border-neutral-300 bg-transparent px-1 py-0.5 text-xs dark:border-neutral-600"
      />
      <button
        type="button"
        onClick={setCustomStamp}
        disabled={custom.trim().length === 0}
        className="rounded border border-neutral-300 px-1.5 py-0.5 text-xs hover:bg-neutral-100 disabled:opacity-40 dark:border-neutral-600 dark:hover:bg-neutral-800"
      >
        Set
      </button>
      <button
        type="button"
        onClick={() => void pickImage()}
        aria-label="Image stamp"
        aria-pressed={armed?.kind === "image"}
        title="Stamp a PNG image (e.g. a signature or logo)"
        className={
          "rounded border border-neutral-300 px-1.5 py-0.5 text-xs hover:bg-neutral-100 dark:border-neutral-600 dark:hover:bg-neutral-800 " +
          (armed?.kind === "image" ? "ring-2 ring-blue-500 ring-offset-1" : "")
        }
      >
        Image…
      </button>
    </div>
  );
}
