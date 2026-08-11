// Source guard: a React state updater must not dereference `e.currentTarget`.
//
// This exists because of a real crash. `SignatureDialog`'s pointer-down handler
// did:
//
//     setStrokes((prev) => [...prev, [sample(e)]]);   // sample reads e.currentTarget
//
// React nulls `currentTarget` as soon as the handler returns — it is only
// meaningful during dispatch, since it changes as the event bubbles. A state
// updater is a closure React invokes *later*, during the render phase, so by
// then it is null and the dereference takes down the whole tree (white screen).
//
// **jsdom does not reproduce it.** The component test fired pointerDown, passed,
// and shipped; the WebView crashed on the first click. The flush timing differs
// enough that the updater still saw a live `currentTarget` under vitest. So a
// behavioural test cannot guard this — it would pass either way. Checking the
// source can.
//
// Two shapes are flagged, and the second is the one that actually shipped:
//
//   1. `e.currentTarget` read directly inside the updater;
//   2. the event passed *whole* to a helper — `sample(e)` — where the
//      dereference is one call away and invisible here.
//
// The first version of this guard only checked (1), passed against the real
// bug, and was therefore worthless. It is worth re-checking a guard by
// reintroducing the defect and watching it fail.
//
// `e.target` is deliberately not covered: React leaves it intact after dispatch,
// so reading it late is legitimate (`AnnotationPanel`'s search box does) — and
// that is a property read, not the whole event escaping into a callee.
//
// The fix is always the same: read what you need before calling setState, and
// close over the value rather than the event.

import { readdirSync, readFileSync } from "node:fs";
import { join, resolve } from "node:path";

import { describe, expect, it } from "vitest";

/**
 * Matches `setSomething((x) => …)` and captures the updater body, roughly. A
 * regex is the right tool here despite the usual objection: this guards a
 * lexical shape, not program semantics, and a false positive is a comment away
 * from being resolved.
 */
const UPDATER = /set[A-Z]\w*\(\s*\(\s*\w*\s*\)\s*=>\s*\{?([\s\S]{0,500}?)\)\s*;/g;

describe("no synthetic event inside a state updater", () => {
  it("never dereferences e.currentTarget from a setState callback", () => {
    // Node's own recursive readdir rather than a glob dependency — one test is
    // not a reason to add a package.
    const root = resolve(process.cwd(), "src");
    const files = readdirSync(root, { recursive: true, encoding: "utf8" })
      .filter((f) => f.endsWith(".tsx"))
      .map((f) => join(root, f));
    const offenders: string[] = [];

    for (const file of files) {
      const src = readFileSync(file, "utf8");
      for (const match of src.matchAll(UPDATER)) {
        const body = match[1] ?? "";
        // (1) direct dereference, or (2) the whole event handed to a helper.
        const direct = /\b\w+\.currentTarget\b/.test(body);
        const escapes = /\w+\(\s*(?:e|ev|evt|event)\s*[,)]/.test(body);
        if (direct || escapes) {
          const line = src.slice(0, match.index).split("\n").length;
          offenders.push(`${file.replace(`${process.cwd()}/`, "")}:${line}`);
        }
      }
    }

    expect(
      offenders,
      `A state updater runs during render, after React has nulled currentTarget.\n` +
        `Read the value before calling setState and close over it instead:\n` +
        offenders.join("\n"),
    ).toEqual([]);
  });
});
