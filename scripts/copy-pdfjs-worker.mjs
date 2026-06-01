#!/usr/bin/env node
// Copies PDF.js's worker bundle into public/ so it's served at
// /pdfjs/pdf.worker.min.mjs — the URL `src/view/pdfjs-worker.ts` points
// `GlobalWorkerOptions.workerSrc` at.
//
// Why a copy (not a Vite import or committed file):
//   - The worker is a ~1.2 MB generated artifact owned by the pdfjs-dist
//     version in node_modules; committing it would rot on every bump.
//   - Hosting it under public/ (vs bundling it through Vite) is the
//     deliberate choice in pdfjs-worker.ts — public assets are served
//     verbatim by Vite in dev and by Tauri's asset protocol in prod,
//     sidestepping "worker fails to load over tauri://" issues.
//
// Runs automatically via the postinstall / predev / prebuild hooks in
// package.json, and can be run by hand: `node scripts/copy-pdfjs-worker.mjs`.

import { copyFileSync, existsSync, mkdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const src = join(
  repoRoot,
  "node_modules",
  "pdfjs-dist",
  "build",
  "pdf.worker.min.mjs",
);
const destDir = join(repoRoot, "public", "pdfjs");
const dest = join(destDir, "pdf.worker.min.mjs");

if (!existsSync(src)) {
  // Not fatal: a fresh checkout before `npm install` won't have it yet.
  // The postinstall hook re-runs this once node_modules exists.
  console.warn(
    `copy-pdfjs-worker: source not found at ${src} — skipping ` +
      "(run after `npm install`).",
  );
  process.exit(0);
}

mkdirSync(destDir, { recursive: true });
copyFileSync(src, dest);
console.log(`copy-pdfjs-worker: ${src} → ${dest}`);
