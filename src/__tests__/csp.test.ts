import { readFileSync } from "node:fs";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

// FABLE_REVIEW §3.8 (P4.HF14) — regression guard for the webview
// Content-Security-Policy. The webview renders hostile input (arbitrary
// PDFs via PDF.js) *and* holds IPC access, so it must not run unfenced.
// CSP is enforced by the webview at runtime, which no other automated test
// exercises — this guard at least stops the config silently regressing to
// `null` or dropping a directive the frontend depends on.
//
// If this fails after a legitimate change, update the expectations here
// AND re-run the manual in-app smoke test (load a normal + a scanned PDF,
// confirm zero CSP violations in DevTools) — see the P4.HF14 plan.

/** Parse a CSP string into `{ directive: sources[] }`. */
function parseCsp(csp: string): Record<string, string[]> {
  const out: Record<string, string[]> = {};
  for (const clause of csp.split(";")) {
    const [name, ...sources] = clause.trim().split(/\s+/).filter(Boolean);
    if (name) out[name] = sources;
  }
  return out;
}

function loadSecurity(): { csp: unknown; devCsp: unknown } {
  const path = resolve(__dirname, "../../src-tauri/tauri.conf.json");
  const conf = JSON.parse(readFileSync(path, "utf8")) as {
    app?: { security?: { csp?: unknown; devCsp?: unknown } };
  };
  const security = conf.app?.security ?? {};
  return { csp: security.csp, devCsp: security.devCsp };
}

describe("tauri webview CSP", () => {
  it("production csp is a non-null string with the expected fences", () => {
    const { csp } = loadSecurity();
    expect(typeof csp, "app.security.csp must be set (was null?)").toBe("string");
    const d = parseCsp(csp as string);

    expect(d["default-src"]).toEqual(["'self'"]);
    // PDF.js v5 instantiates WASM decoders — allow that, but nothing wider.
    expect(d["script-src"]).toContain("'self'");
    expect(d["script-src"]).toContain("'wasm-unsafe-eval'");
    expect(d["script-src"]).not.toContain("'unsafe-eval'");
    expect(d["script-src"]).not.toContain("'unsafe-inline'"); // prod stays strict
    // Same-origin PDF.js worker; Tauri IPC; blob thumbnails; Tailwind styles.
    expect(d["worker-src"]).toContain("'self'");
    expect(d["connect-src"]).toEqual(
      expect.arrayContaining(["'self'", "ipc:", "http://ipc.localhost"]),
    );
    expect(d["img-src"]).toEqual(expect.arrayContaining(["'self'", "blob:", "data:"]));
    expect(d["style-src"]).toEqual(expect.arrayContaining(["'self'", "'unsafe-inline'"]));
    expect(d["object-src"]).toEqual(["'none'"]);
    expect(d["base-uri"]).toEqual(["'self'"]);
  });

  it("devCsp relaxes only what Vite HMR needs", () => {
    const { devCsp } = loadSecurity();
    expect(typeof devCsp, "a separate devCsp is required so a strict csp doesn't break HMR").toBe(
      "string",
    );
    const d = parseCsp(devCsp as string);

    // Vite's dev preamble is an inline module script; HMR uses a websocket.
    expect(d["script-src"]).toContain("'unsafe-inline'");
    expect(d["connect-src"].some((s) => s.startsWith("ws://localhost"))).toBe(true);
    // …but the hardening directives still hold in dev.
    expect(d["object-src"]).toEqual(["'none'"]);
    expect(d["default-src"]).toEqual(["'self'"]);
  });
});
