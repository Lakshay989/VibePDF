// The "Open source licences" view.
//
// Apache-2.0 §4 and the BSD/MIT family all require that a binary distribution
// reproduce the licence and attribution of what it contains. A file next to the
// executable technically satisfies that and is read by nobody; this puts the
// inventory where someone can actually find it.
//
// The data is generated — `npm run licenses` walks the resolved Rust and npm
// trees into `src/generated/third-party-licenses.ts`. A hand-maintained list
// goes stale on the next dependency change, and a stale licence list is worse
// than none: it makes a false claim about what the binary contains.

import { useMemo, useState } from "react";

import {
  BUNDLED_COMPONENTS,
  NPM_DEPENDENCIES,
  RUST_DEPENDENCIES,
  type ThirdPartyLicence,
} from "@/generated/third-party-licenses";

interface Props {
  open: boolean;
  onClose: () => void;
}

function matches(entry: ThirdPartyLicence, needle: string): boolean {
  if (needle === "") return true;
  const q = needle.toLowerCase();
  return (
    entry.name.toLowerCase().includes(q) ||
    entry.license.toLowerCase().includes(q) ||
    entry.version.toLowerCase().includes(q)
  );
}

function Section({
  title,
  entries,
  filter,
}: {
  title: string;
  entries: readonly ThirdPartyLicence[];
  filter: string;
}) {
  const shown = entries.filter((e) => matches(e, filter));
  if (shown.length === 0) return null;

  return (
    <section className="mb-4">
      <h3 className="mb-1 text-xs font-medium text-neutral-500 dark:text-neutral-400">
        {title} ({shown.length}
        {shown.length === entries.length ? "" : ` of ${entries.length}`})
      </h3>
      <ul className="divide-y divide-neutral-100 dark:divide-neutral-800">
        {shown.map((e) => (
          <li key={`${e.name}@${e.version}`} className="py-1">
            <div className="flex items-baseline justify-between gap-2">
              <span className="font-mono text-xs">
                {e.name}
                <span className="text-neutral-400"> {e.version}</span>
              </span>
              <span className="shrink-0 text-xs text-neutral-500 dark:text-neutral-400">
                {e.license}
              </span>
            </div>
            {e.note ? (
              <p className="mt-0.5 text-xs text-neutral-500 dark:text-neutral-400">{e.note}</p>
            ) : null}
          </li>
        ))}
      </ul>
    </section>
  );
}

export function LicensesDialog({ open, onClose }: Props) {
  const [filter, setFilter] = useState("");

  const total = useMemo(
    () => BUNDLED_COMPONENTS.length + RUST_DEPENDENCIES.length + NPM_DEPENDENCIES.length,
    [],
  );

  if (!open) return null;

  const close = () => {
    setFilter("");
    onClose();
  };

  return (
    <div
      role="dialog"
      aria-label="Open source licences"
      className="fixed inset-0 z-40 flex items-center justify-center bg-black/40"
    >
      <div className="flex max-h-[80vh] w-[640px] flex-col rounded-lg bg-white p-4 shadow-xl dark:bg-neutral-900">
        <h2 className="mb-1 text-sm font-medium">Open source licences</h2>
        <p className="mb-3 text-xs text-neutral-500 dark:text-neutral-400">
          VibePDF is dual-licensed MIT or Apache-2.0. It is built on {total} third-party
          components, listed below with the terms each is used under. The full text of
          PDFium&rsquo;s licences, and of the libraries it bundles, ships beside the
          application in <span className="font-mono">resources/pdfium/licenses</span>.
        </p>

        <input
          type="search"
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
          placeholder="Filter by name or licence…"
          aria-label="Filter licences"
          className="mb-3 w-full rounded border border-neutral-300 bg-transparent px-2 py-1 text-sm dark:border-neutral-700"
        />

        <div className="min-h-0 flex-1 overflow-y-auto pr-1">
          <Section title="Bundled components" entries={BUNDLED_COMPONENTS} filter={filter} />
          <Section title="Rust crates" entries={RUST_DEPENDENCIES} filter={filter} />
          <Section title="npm packages" entries={NPM_DEPENDENCIES} filter={filter} />
        </div>

        <div className="mt-3 flex justify-end">
          <button
            type="button"
            onClick={close}
            className="rounded bg-neutral-900 px-3 py-1 text-sm text-white dark:bg-neutral-100 dark:text-neutral-900"
          >
            Close
          </button>
        </div>
      </div>
    </div>
  );
}
