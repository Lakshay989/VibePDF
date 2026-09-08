import { describe, it, expect } from "vitest";
import { readdir, readFile } from "node:fs/promises";
import path from "node:path";

// A PDF can carry its own JavaScript. PDF.js executes it — but only through the
// pieces that own that behaviour: its viewer (`PDFScriptingManager`) and its
// `AnnotationLayer`, both of which take an `enableScripting` option that
// defaults to on. That is the exposure behind GHSA-hq66-cqwq-w95j
// (CVE-2026-16633), which reports arbitrary script execution in the host
// context on opening a malicious document.
//
// VibePDF is not exposed to it, and the reason is architectural rather than
// configured: the app uses only PDF.js's *core* API. `getDocument` parses,
// `TextLayer` positions selectable text, and the on-page annotation UI is our
// own React SVG component in `src/view/annotation-layer.tsx` — which shares a
// name with PDF.js's class and nothing else. Nothing here ever constructs the
// PDF.js component that would run a document's script.
//
// "It happens not to be imported" is a property that a single future import
// silently repeals, so this test pins it. It reads the frontend source rather
// than exercising behaviour, because the security-relevant fact *is* the import
// surface: there is no runtime moment at which a missing scripting manager can
// be observed.
//
// If this test fails, the fix is not to widen the list. It is to pass
// `enableScripting: false` wherever the new component is constructed, and then
// widen the list deliberately.

const ALLOWED_RUNTIME_IMPORTS = new Set([
  "getDocument",
  "GlobalWorkerOptions",
  "TextLayer",
]);

const SRC = path.join(__dirname, "../..");

async function tsSources(dir: string): Promise<string[]> {
  const entries = await readdir(dir, { withFileTypes: true });
  const out: string[] = [];
  for (const entry of entries) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      out.push(...(await tsSources(full)));
    } else if (/\.tsx?$/.test(entry.name)) {
      out.push(full);
    }
  }
  return out;
}

/** Every `import ... from "pdfjs-dist..."` statement in one file. */
function pdfjsImports(source: string): { clause: string; module: string }[] {
  // Anchored at a line start, and the clause may not contain `;` — otherwise a
  // lazy match happily starts at an earlier `import` and swallows whole
  // statements to reach this one's `from`.
  const re = /^import\s+([^;]*?)\s+from\s+["'](pdfjs-dist[^"']*)["']/gm;
  const found: { clause: string; module: string }[] = [];
  for (const m of source.matchAll(re)) {
    found.push({ clause: m[1] ?? "", module: m[2] ?? "" });
  }
  return found;
}

/** The named bindings a clause pulls in as *values*, skipping `import type`. */
function runtimeBindings(clause: string): string[] {
  if (/^\s*type\s/.test(clause)) return []; // `import type { ... }`
  const braces = /\{([\s\S]*?)\}/.exec(clause);
  const named = braces
    ? (braces[1] ?? "")
        .split(",")
        .map((s) => s.trim())
        .filter(Boolean)
        // `{ type PDFDocumentProxy, TextLayer }` — the inline `type` modifier
        // marks a binding that is erased at compile time.
        .filter((s) => !/^type\s/.test(s))
        .map((s) => (s.split(/\s+as\s+/)[0] ?? "").trim())
    : [];
  const defaultOrNamespace = clause
    .replace(/\{[\s\S]*?\}/, "")
    .split(",")
    .map((s) => s.trim())
    .filter(Boolean);
  return [...defaultOrNamespace, ...named].filter(Boolean);
}

describe("PDF.js import surface", () => {
  it("imports no PDF.js component that executes a document's JavaScript", async () => {
    const files = await tsSources(SRC);
    expect(files.length).toBeGreaterThan(50); // the walk actually found the tree

    const offenders: string[] = [];
    for (const file of files) {
      const source = await readFile(file, "utf8");
      for (const { clause, module } of pdfjsImports(source)) {
        const where = path.relative(SRC, file);

        // The viewer bundle is where PDFScriptingManager lives.
        if (module !== "pdfjs-dist") {
          offenders.push(`${where}: imports "${module}"`);
          continue;
        }
        for (const binding of runtimeBindings(clause)) {
          if (!ALLOWED_RUNTIME_IMPORTS.has(binding)) {
            offenders.push(`${where}: runtime import "${binding}"`);
          }
        }
      }
    }

    expect(offenders).toEqual([]);
  });
});
