// SPEC: P1.E5 — the one smoke test that proves the real app works
// end-to-end. The app is launched (by wdio.conf.ts) with
// tests/fixtures/basic/hello.pdf as a CLI argument; A2's CLI-open path
// buffers it and the frontend opens it on mount.
//
// Asserting that a page <canvas> actually renders is exactly what would
// have caught this session's "PDF.js worker missing → nothing renders"
// bug, which every unit/component test was blind to.

import { $, expect } from "@wdio/globals";

describe("VibePDF end-to-end smoke", () => {
  it("renders page 1 of the CLI-opened PDF", async () => {
    // PageVirtualizer emits `<div data-page="N"><canvas class="shadow">`
    // for each rendered page; page 1 is in view on launch.
    const pageCanvas = await $('[data-page="1"] canvas');
    await pageCanvas.waitForExist({ timeout: 60_000 });

    const { width, height } = await pageCanvas.getSize();
    expect(width).toBeGreaterThan(0);
    expect(height).toBeGreaterThan(0);
  });
});
