import { existsSync } from "node:fs";
import { relative, resolve } from "node:path";

import ts from "typescript";
import { describe, expect, it } from "vitest";

// Vite looks for `vite.config.js` before `vite.config.ts`. `npm run build`
// runs `tsc -b`, which builds `tsconfig.node.json`, and while that project
// emitted JavaScript every build left a compiled `vite.config.js` beside the
// source. From then on dev, build and these tests all loaded that snapshot, and
// edits to `vite.config.ts` quietly stopped taking effect (found 2026-09-15:
// Vite's `resolveConfig` named the stale `.js`).
//
// TypeScript won't let a referenced project set `noEmit`, so the project emits
// declarations only, into `node_modules/.tmp`. These tests ask the compiler
// what it would write rather than running a build.

const root = resolve(__dirname, "../..");

function outputsOf(configFile: string, source: string): readonly string[] {
  const host: ts.ParseConfigFileHost = {
    ...ts.sys,
    onUnRecoverableConfigFileDiagnostic: (d) => {
      throw new Error(ts.flattenDiagnosticMessageText(d.messageText, "\n"));
    },
  };
  const parsed = ts.getParsedCommandLineOfConfigFile(resolve(root, configFile), {}, host);
  if (!parsed) throw new Error(`could not parse ${configFile}`);
  return ts.getOutputFileNames(parsed, resolve(root, source), !ts.sys.useCaseSensitiveFileNames);
}

describe("building vite.config.ts", () => {
  it("emits no JavaScript", () => {
    const js = outputsOf("tsconfig.node.json", "vite.config.ts").filter((f) => /\.[cm]?js$/.test(f));
    expect(js).toEqual([]);
  });

  it("writes whatever it does emit under node_modules/.tmp", () => {
    for (const file of outputsOf("tsconfig.node.json", "vite.config.ts")) {
      expect(relative(root, file)).toMatch(/^node_modules\/\.tmp\//);
    }
  });

  it("leaves no compiled config beside the source for Vite to pick up", () => {
    for (const name of ["vite.config.js", "vite.config.mjs", "vite.config.cjs"]) {
      expect(existsSync(resolve(root, name)), `${name} shadows vite.config.ts — delete it`).toBe(false);
    }
  });
});
