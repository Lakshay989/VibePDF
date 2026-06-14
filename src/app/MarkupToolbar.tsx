// SPEC: P3-ANN-001 (P3.B1a) — the text-markup toolbar.
//
// Select text in the page, then click a markup button to apply it to the
// selection (highlight / underline / strikethrough / squiggly), in the chosen
// colour. `onMouseDown → preventDefault` is essential: a normal mousedown on a
// button collapses the page text selection before the click handler runs, so we
// suppress it to keep the selection alive.

import { addTextMarkup } from "@/ipc/annotations";
import { useEditEpochStore } from "@/state/edit-epoch-store";
import { useHistoryStore } from "@/state/history-store";
import { useToolStore } from "@/state/tool-store";
import type { MarkupSubtype } from "@/tools/_framework";
import { applyMarkupToSelection } from "@/tools/text-markup/apply-markup";

const SUBTYPES: { id: MarkupSubtype; label: string; title: string }[] = [
  { id: "highlight", label: "Highlight", title: "Highlight selected text" },
  { id: "underline", label: "Underline", title: "Underline selected text" },
  { id: "strikethrough", label: "Strikethrough", title: "Strike through selected text" },
  { id: "squiggly", label: "Squiggly", title: "Squiggly underline selected text" },
];

const COLORS = ["#ffd400", "#7dd957", "#5ac8fa", "#ff8a8a", "#d09bff"];

export function MarkupToolbar({ documentId }: { documentId: string }) {
  const color = useToolStore((s) => s.options.color);
  const opacity = useToolStore((s) => s.options.opacity);
  const setOptions = useToolStore((s) => s.setOptions);
  const bumpEpoch = useEditEpochStore((s) => s.bumpEpoch);
  const setHistory = useHistoryStore((s) => s.setHistory);

  const apply = (subtype: MarkupSubtype) => {
    void applyMarkupToSelection({ documentId, subtype, color, opacity }, (page, quads) =>
      addTextMarkup(documentId, page, subtype, quads, color, opacity),
    )
      .then((history) => {
        if (history) {
          // The markup is now in the PDF; reload so the canvas renders it.
          bumpEpoch(documentId);
          setHistory(documentId, history);
        }
      })
      .catch((err: unknown) => console.warn("markup failed", documentId, err));
  };

  return (
    <div className="flex items-center gap-2 border-b border-neutral-200 px-3 py-1 text-sm dark:border-neutral-800">
      <span className="text-xs uppercase tracking-wide text-neutral-400">Markup</span>
      {SUBTYPES.map((s) => (
        <button
          key={s.id}
          type="button"
          onMouseDown={(e) => e.preventDefault()}
          onClick={() => apply(s.id)}
          title={s.title}
          aria-label={s.title}
          className="rounded px-2 py-0.5 hover:bg-neutral-100 dark:hover:bg-neutral-800"
        >
          {s.label}
        </button>
      ))}
      <span className="text-neutral-300 dark:text-neutral-700">|</span>
      <div className="flex items-center gap-1">
        {COLORS.map((c) => (
          <button
            key={c}
            type="button"
            onMouseDown={(e) => e.preventDefault()}
            onClick={() => setOptions({ color: c })}
            aria-label={`Markup colour ${c}`}
            aria-pressed={color === c}
            title={c}
            style={{ backgroundColor: c }}
            className={
              "h-4 w-4 rounded-full border border-neutral-300 dark:border-neutral-600 " +
              (color === c ? "ring-2 ring-blue-500 ring-offset-1" : "")
            }
          />
        ))}
      </div>
    </div>
  );
}
