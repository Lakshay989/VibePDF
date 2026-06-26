// SPEC: P4-EDIT-002 (P4.A2) — "warn the user once per document, and offer to
// re-flow the affected text run." The warning + the offer live here; the
// re-flow *action* is wired when text editing (P4.B1) lands, so the button is
// present-but-disabled with an explanatory tooltip. Honesty over silence: we
// name every font we'd substitute rather than swap glyphs quietly.

import type { FontReport } from "@/ipc/fonts";

interface Props {
  /** The font report, or `null` while it loads. */
  report: FontReport | null;
  /** Whether the user has dismissed the banner for this document. */
  dismissed: boolean;
  /** Dismiss handler (the "once" in once-per-document). */
  onDismiss: () => void;
}

export function FontFallbackBanner({ report, dismissed, onDismiss }: Props) {
  // Nothing to warn about → render nothing (the common case).
  if (!report || !report.needsFallback || dismissed) return null;

  const fallbacks = report.fonts.filter((f) => f.status === "fallback");

  return (
    <div
      role="status"
      className="flex items-start gap-3 border-b border-amber-300 bg-amber-50 px-4 py-2 text-sm text-amber-900 dark:border-amber-700/60 dark:bg-amber-950/40 dark:text-amber-200"
    >
      <span aria-hidden className="mt-0.5 select-none">
        ⚠️
      </span>
      <div className="min-w-0 flex-1">
        <p>
          {fallbacks.length === 1
            ? "1 font isn't embedded or installed — editing its text will substitute a stand-in:"
            : `${fallbacks.length} fonts aren't embedded or installed — editing their text will substitute stand-ins:`}
        </p>
        <ul className="mt-1 flex flex-wrap gap-x-4 gap-y-0.5 text-amber-800 dark:text-amber-300/90">
          {fallbacks.map((f) => (
            <li key={f.fontName} className="font-mono text-xs">
              {f.fontName} → {f.substitute}
            </li>
          ))}
        </ul>
      </div>
      <button
        type="button"
        disabled
        title="Available once text editing ships (P4.B1)"
        aria-label="Re-flow affected text (available once text editing ships)"
        className="shrink-0 cursor-not-allowed rounded border border-amber-300 px-2 py-1 text-xs opacity-50 dark:border-amber-700/60"
      >
        Re-flow affected text
      </button>
      <button
        type="button"
        onClick={onDismiss}
        aria-label="Dismiss font warning"
        className="shrink-0 rounded px-2 py-1 text-xs hover:bg-amber-100 dark:hover:bg-amber-900/40"
      >
        Dismiss
      </button>
    </div>
  );
}
