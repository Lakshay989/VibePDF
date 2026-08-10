// Layout contract for the side panels — a source guard, in the same spirit as
// `csp.test.ts`.
//
// The bug: in a half-screen window the left panels got crushed. Every flex item
// defaults to `flex-shrink: 1`, and only `FieldPropertiesPanel` opted out with
// `shrink-0` — so when the row ran out of room the squeeze landed entirely on
// the three left panels, squashing thumbnails and clipping labels while the
// right-hand panel kept its full width.
//
// The fix is a pair, and both halves have to stay: `shrink-0` on each panel so
// it holds its declared width, and `min-w-0` on the page container so it is
// willing to absorb the squeeze instead. Drop either one and the layout breaks
// again in a way no rendering test in this suite would notice — jsdom does no
// layout, so this asserts the class contract at the source.

import { readFileSync } from "node:fs";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

const read = (rel: string) => readFileSync(resolve(process.cwd(), rel), "utf8");

/** The `<aside>` opening tag's className, for a panel file. */
const asideClasses = (rel: string): string => {
  // `<aside>` may carry other attributes first, on their own lines.
  const m = /<aside[^>]*?className="([^"]*)"/s.exec(read(rel));
  expect(m, `${rel} has an <aside> with a literal className`).not.toBeNull();
  return m?.[1] ?? "";
};

const PANELS = [
  ["thumbnails", "src/panels/ThumbnailPanel.tsx"],
  ["outline", "src/panels/OutlinePanel.tsx"],
  ["annotations", "src/panels/AnnotationPanel.tsx"],
  ["field properties", "src/app/FieldPropertiesPanel.tsx"],
] as const;

describe("side panel layout contract", () => {
  it.each(PANELS)("the %s panel does not shrink", (_name, file) => {
    expect(asideClasses(file).split(/\s+/)).toContain("shrink-0");
  });

  it.each(PANELS)("the %s panel declares a fixed width", (_name, file) => {
    // A panel that neither shrinks nor has a width would size to its content.
    expect(asideClasses(file)).toMatch(/\bw-(\d+|\[[^\]]+\])/);
  });

  it("the page column is at least as wide as its widest page", () => {
    // Measured: with `items-center` alone, a 700px page in a 400px scroller put
    // 148px of the page LEFT of the scroll origin — where `overflow-auto` cannot
    // reach it — and reported scrollWidth 550, so the browser did not even count
    // it. `min-w-min` moved that to clippedLeft 0 / scrollWidth 700.
    const src = read("src/view/PageVirtualizer.tsx");
    expect(src).toMatch(/className="flex min-w-min flex-col items-center/);
  });

  it("the page container can shrink below its content", () => {
    // Without `min-w-0` the page area refuses to give way and the panels take
    // the whole squeeze — the original bug.
    const src = read("src/view/PdfViewer.tsx");
    expect(src).toMatch(/className="relative flex-1 min-w-0 overflow-hidden"/);
  });
});
