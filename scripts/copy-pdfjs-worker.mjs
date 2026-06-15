#!/usr/bin/env node
// Copies PDF.js's runtime assets into public/ so they're served under /pdfjs/:
//   - pdf.worker.min.mjs — the worker URL `src/view/pdfjs-worker.ts` points at.
//   - standard_fonts/    — standard-14 font data. `getTextContent` (text
//     selection / markup, P3-ANN-001) needs it to map glyphs → Unicode; without
//     it text extraction throws. See `render-page.ts` loadDocument.
//   - cmaps/             — CMaps for CID-keyed (e.g. CJK) fonts.
//
// Why copies (not Vite imports or committed files):
//   - They're generated artifacts owned by the pdfjs-dist version in
//     node_modules; committing them would rot on every bump (public/pdfjs/ is
//     gitignored).
//   - Hosting under public/ (vs bundling through Vite) is the deliberate choice
//     in pdfjs-worker.ts — public assets are served verbatim by Vite in dev and
//     by Tauri's asset protocol in prod, sidestepping "fails to load over
//     tauri://" issues.
//
// Runs automatically via the postinstall / predev / prebuild hooks in
// package.json, and can be run by hand: `node scripts/copy-pdfjs-worker.mjs`.

import { copyFileSync, cpSync, existsSync, mkdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const pdfjsDist = join(repoRoot, "node_modules", "pdfjs-dist");
const worker = join(pdfjsDist, "build", "pdf.worker.min.mjs");
const destDir = join(repoRoot, "public", "pdfjs");

if (!existsSync(worker)) {
  // Not fatal: a fresh checkout before `npm install` won't have it yet.
  // The postinstall hook re-runs this once node_modules exists.
  console.warn(
    `copy-pdfjs-worker: source not found at ${worker} — skipping ` +
      "(run after `npm install`).",
  );
  process.exit(0);
}

mkdirSync(destDir, { recursive: true });
copyFileSync(worker, join(destDir, "pdf.worker.min.mjs"));

// Font data + CMaps (directories) — best-effort; older pdfjs-dist layouts may
// differ, and rendering still works without them (only text extraction needs
// the standard fonts).
for (const dir of ["standard_fonts", "cmaps"]) {
  const from = join(pdfjsDist, dir);
  if (existsSync(from)) cpSync(from, join(destDir, dir), { recursive: true });
}

console.log(`copy-pdfjs-worker: pdfjs-dist assets → ${destDir}`);
