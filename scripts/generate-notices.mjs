#!/usr/bin/env node
// Builds the third-party licence inventory that a binary release has to ship.
//
// Why generated and not hand-written: a hand-maintained list is wrong within a
// week of the next `npm install`, and a licence list nobody trusts is one
// nobody reads. This reads the actual resolved trees — `cargo metadata` for
// Rust, `package-lock.json` for npm — so the file always describes what the
// build would produce.
//
// It emits two things from one source of truth:
//   THIRD-PARTY-LICENSES.md            for a human, and for the repository
//   src/generated/third-party-licenses.ts   for the in-app licence view
//
// It also acts as a gate. `docs/01_VISION.md` promises no copyleft in the
// shipped binary; this is where that promise is checked rather than assumed.
//
//   node scripts/generate-notices.mjs          # write the files
//   node scripts/generate-notices.mjs --check  # fail if they are out of date
//
// Runtime dependencies only. A devDependency ships to nobody, so listing one
// would make the inventory longer and less true.

import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync, mkdirSync, existsSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.join(path.dirname(fileURLToPath(import.meta.url)), "..");
const CHECK_ONLY = process.argv.includes("--check");

// SPDX identifiers that impose no source-disclosure obligation on a binary.
// Anything outside this set is not automatically wrong — it is unreviewed, and
// the script stops so a human decides.
const PERMISSIVE = new Set([
  "0BSD",
  "Apache-2.0",
  "BSD-2-Clause",
  "BSD-3-Clause",
  "BSL-1.0",
  // Community Data License Agreement – Permissive 2.0. Reviewed 2026-09-22 for
  // `webpki-roots`, the certificate-authority list `ureq` links for the
  // language-pack download (P7-OCR-002). It is a *data* licence: it permits use
  // and redistribution of the data, with attribution, and imposes nothing on
  // the software that reads it — no source disclosure, no copyleft. Recorded
  // here rather than waved through, because this set is the gate that keeps
  // docs/01_VISION.md's "no copyleft in the shipped binary" honest.
  "CDLA-Permissive-2.0",
  "CC0-1.0",
  "ISC",
  "MIT",
  "MIT-0",
  "MPL-2.0", // file-level copyleft: no obligation unless we modify those files
  "NCSA",
  "OFL-1.1",
  "OpenSSL",
  "Unicode-3.0",
  "Unicode-DFS-2016",
  "Unlicense",
  "Zlib",
]);

/**
 * Is an SPDX expression satisfiable with permissive terms alone?
 *
 * `A OR B` lets us pick, so one permissive branch is enough — this is why
 * "GPL-2.0 OR MIT" is fine and must not be reported. `A AND B` binds us to
 * both. `WITH <exception>` only ever widens permission, so the exception is
 * dropped before the lookup.
 */
function isPermissive(expression) {
  if (!expression) return false;
  // Crates published before SPDX expressions settled use a slash for OR
  // ("MIT/Apache-2.0"). Cargo still accepts it, so a lot of the tree carries it.
  return expression
    .replace(/\s*\/\s*/g, " OR ")
    .split(/\s+OR\s+/i)
    .some((alternative) =>
      alternative
        .replace(/[()]/g, "")
        .split(/\s+AND\s+/i)
        .every((term) => PERMISSIVE.has(term.trim().split(/\s+WITH\s+/i)[0])),
    );
}

/** The platforms a release targets. The inventory is their union. */
const RUST_TARGETS = [
  "aarch64-apple-darwin",
  "x86_64-apple-darwin",
  "x86_64-unknown-linux-gnu",
  "aarch64-unknown-linux-gnu",
  "x86_64-pc-windows-msvc",
];

/**
 * Every crate that actually links into a build, per target, unioned.
 *
 * `cargo tree` rather than walking `cargo metadata`'s resolve graph: that
 * graph unions features across the whole workspace, so an *optional* edge
 * counts as taken. Adding the OCR build-dependency exposed this (2026-09-21) —
 * its build script uses reqwest, which put reqwest in the lockfile, after
 * which the old walk reported tauri's optional reqwest, rustls and a TLS root
 * store as shipped. `cargo tree -e normal --target <t>` resolves features the
 * way a build does, and says those are linked on no target we ship.
 */
function rustDependencies() {
  const out = [];
  for (const target of RUST_TARGETS) {
    const raw = execFileSync(
      "cargo",
      [
        "tree", "-e", "normal", "--target", target, "--prefix", "none",
        "--format", "{p}|{l}|{r}",
        "--manifest-path", path.join(ROOT, "src-tauri", "Cargo.toml"),
      ],
      { encoding: "utf8", maxBuffer: 64 * 1024 * 1024 },
    );
    for (const line of raw.split("\n")) {
      // "name v1.2.3 (path)? (*)?|<licence>|<repository>"; "(*)" marks a
      // subtree cargo already printed.
      const [pkg = "", license = "", repository = ""] = line.split("|");
      const match = /^(\S+) v(\S+)/.exec(pkg.trim());
      if (!match) continue;
      const [, name, version] = match;
      if (name === "vibepdf") continue; // the crate being inventoried
      out.push({ name, version, license: license.trim(), repository: repository.trim() });
    }
  }
  return out;
}

/** Every npm package the lockfile does not mark dev-only. */
function npmDependencies() {
  const lock = JSON.parse(readFileSync(path.join(ROOT, "package-lock.json"), "utf8"));
  const out = [];
  for (const [where, entry] of Object.entries(lock.packages ?? {})) {
    if (where === "" || entry.dev || entry.devOptional) continue;
    const name = entry.name ?? where.replace(/^.*node_modules\//, "");

    // The lockfile carries a license field only sometimes; the installed
    // package.json is the authority when it is present.
    let license = entry.license ?? "";
    const manifest = path.join(ROOT, where, "package.json");
    if (existsSync(manifest)) {
      const pkg = JSON.parse(readFileSync(manifest, "utf8"));
      license = pkg.license ?? pkg.licenses?.[0]?.type ?? license;
    }
    out.push({
      name,
      version: entry.version ?? "",
      license,
      repository:
        typeof entry.repository === "string" ? entry.repository : (entry.repository?.url ?? ""),
    });
  }
  return out;
}

/** Shipped but resolved by neither package manager — fetched or vendored by hand. */
const PDFJS_VERSION =
  JSON.parse(readFileSync(path.join(ROOT, "package-lock.json"), "utf8")).packages?.[
    "node_modules/pdfjs-dist"
  ]?.version ?? "";

const BUNDLED = [
  {
    name: "PDFium",
    version: "chromium/7857",
    license: "BSD-3-Clause",
    repository: "https://pdfium.googlesource.com/pdfium/",
    note: "Prebuilt binary fetched by scripts/fetch-pdfium.sh. Ships its own LICENSE and a licenses/ directory covering the components it vendors (FreeType, ICU, libjpeg-turbo, libpng, libtiff, OpenJPEG, zlib, Abseil, and others); the fetch script copies both next to the library so they travel with it.",
  },
  {
    name: "pdfium-binaries packaging",
    version: "chromium/7857",
    license: "MIT",
    repository: "https://github.com/bblanchon/pdfium-binaries",
    note: "Copyright Benoit Blanchon. The build scripts that produce the binary above; its MIT licence is the LICENSE file at the archive root.",
  },
  // Third-party code compiled into PDF.js's WebAssembly image decoders. They
  // arrive inside pdfjs-dist (listed above as Apache-2.0) but carry their own
  // upstream licences, so they are listed separately. scripts/copy-pdfjs-worker.mjs
  // copies each LICENSE_* file into public/pdfjs/wasm/ beside the module it
  // covers, and the build bundles them together.
  {
    name: "PDFium JBIG2 and CCITT fax decoder (PDF.js jbig2.wasm)",
    version: PDFJS_VERSION,
    license: "BSD-3-Clause",
    repository: "https://pdfium.googlesource.com/pdfium/",
    note: "Copyright The PDFium Authors. Compiled to WebAssembly by Mozilla, whose build glue is Apache-2.0. Licences: wasm/LICENSE_JBIG2 and wasm/LICENSE_PDFJS_JBIG2.",
  },
  {
    name: "OpenJPEG JPEG 2000 decoder (PDF.js openjpeg.wasm)",
    version: PDFJS_VERSION,
    license: "BSD-2-Clause",
    repository: "https://github.com/uclouvain/openjpeg",
    note: "Copyright the OpenJPEG contributors. Compiled to WebAssembly by Mozilla (glue also BSD-2-Clause). Licences: wasm/LICENSE_OPENJPEG and wasm/LICENSE_PDFJS_OPENJPEG.",
  },
  {
    name: "qcms colour management (PDF.js qcms_bg.wasm)",
    version: PDFJS_VERSION,
    license: "MIT",
    repository: "https://github.com/FirefoxGraphics/qcms",
    note: "Copyright Mozilla Corporation and Marti Maria. Compiled to WebAssembly by Mozilla (glue also MIT). Licences: wasm/LICENSE_QCMS and wasm/LICENSE_PDFJS_QCMS.",
  },
  // P7.A1: the OCR engine. Compiled from source into the binary by the
  // tesseract-rs build (see src-tauri/Cargo.toml), from the checksum-pinned
  // archives scripts/fetch-tesseract-src.sh places for it.
  {
    name: "Tesseract OCR",
    version: "5.5.2",
    license: "Apache-2.0",
    repository: "https://github.com/tesseract-ocr/tesseract",
    note: "Copyright the Tesseract contributors, Apache-2.0 (attribution in NOTICE). Statically linked; its LICENSE travels in the source archive pinned by scripts/fetch-tesseract-src.sh.",
  },
  {
    name: "Leptonica",
    version: "1.87.0",
    license: "BSD-2-Clause",
    repository: "https://github.com/DanBloomberg/leptonica",
    note: "Copyright Dan Bloomberg. The image library Tesseract is built on; statically linked alongside it.",
  },
  {
    name: "Tesseract English language data (tessdata_fast)",
    version: "87416418",
    license: "Apache-2.0",
    repository: "https://github.com/tesseract-ocr/tessdata_fast",
    note: "The eng.traineddata model, fetched by scripts/fetch-tessdata.sh and bundled as a Tauri resource. Its LICENSE ships beside it in resources/tessdata/.",
  },
  {
    name: "CGATS001Compat-v2-micro ICC profile (PDF.js iccs)",
    version: PDFJS_VERSION,
    license: "CC0-1.0",
    repository: "https://github.com/saucecontrol/Compact-ICC-Profiles",
    note: "The CMYK profile PDF.js uses for DeviceCMYK colour. Public-domain dedication: iccs/LICENSE.",
  },
];

function byName(a, b) {
  return a.name.localeCompare(b.name) || a.version.localeCompare(b.version);
}

function dedupe(entries) {
  const seen = new Map();
  for (const e of entries) seen.set(`${e.name}@${e.version}`, e);
  return [...seen.values()].sort(byName);
}

function table(entries) {
  const rows = entries.map(
    (e) =>
      `| ${e.repository ? `[${e.name}](${e.repository})` : e.name} | ${e.version} | ${e.license || "**unknown**"} |`,
  );
  return ["| Package | Version | Licence |", "|---|---|---|", ...rows].join("\n");
}

const rust = dedupe(rustDependencies());
const npm = dedupe(npmDependencies());

const unreviewed = [...rust, ...npm, ...BUNDLED].filter((e) => !isPermissive(e.license));

const counts = {};
for (const e of [...rust, ...npm, ...BUNDLED]) {
  counts[e.license || "unknown"] = (counts[e.license || "unknown"] ?? 0) + 1;
}

const markdown = `# Third-party licences

Generated by \`scripts/generate-notices.mjs\`. **Do not edit by hand** — run
\`npm run licenses\` after changing a dependency.

This is the inventory of what a **shipped binary** contains: Rust crates reached
from the root crate through normal dependency edges, npm packages the lockfile
does not mark dev-only, and the components fetched or vendored outside either
package manager. Build- and dev-only dependencies are excluded, because they
ship to nobody and listing them would make this longer and less true.

Cargo resolves dependencies for every target platform, so the crate list
includes the Windows- and Linux-only crates that a macOS build never compiles.
That is deliberate: this file describes what the project ships across all three
platforms, not one machine's build.

Obligations that a *release* must satisfy, as opposed to this inventory, are in
[\`THIRD-PARTY-NOTICES.md\`](THIRD-PARTY-NOTICES.md). Attribution text for the
Apache-2.0 components is in [\`NOTICE\`](NOTICE).

- **${rust.length}** Rust crates
- **${npm.length}** npm packages
- **${BUNDLED.length}** bundled components
- **${unreviewed.length}** with a licence outside the reviewed-permissive set

## Bundled components

${BUNDLED.map((b) => `### ${b.name} ${b.version}\n\n${b.license} — <${b.repository}>\n\n${b.note}`).join("\n\n")}

## Rust crates

${table(rust)}

## npm packages

${table(npm)}

## Licence totals

| Licence | Count |
|---|---|
${Object.entries(counts)
  .sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))
  .map(([license, n]) => `| ${license} | ${n} |`)
  .join("\n")}

### On the MPL-2.0 entries

\`docs/01_VISION.md\` promises no copyleft in the shipped binary. A handful of
crates in Tauri's WebView stack are MPL-2.0, which is *file-level* copyleft: the
obligation attaches to the MPL-licensed files themselves, and only if they are
modified. We consume them unmodified as dependencies, so nothing is triggered
and no source disclosure is required for our own code. This is the one place
the "no copyleft" line is interpreted rather than applied literally, and it is
recorded here so the decision is visible rather than buried in a script.
`;

const generated = `// GENERATED by scripts/generate-notices.mjs — do not edit.
// Run \`npm run licenses\` to regenerate. See THIRD-PARTY-LICENSES.md.

export interface ThirdPartyLicence {
  readonly name: string;
  readonly version: string;
  readonly license: string;
  readonly repository: string;
  readonly note?: string;
}

export const BUNDLED_COMPONENTS: readonly ThirdPartyLicence[] = ${JSON.stringify(BUNDLED, null, 2)};

export const RUST_DEPENDENCIES: readonly ThirdPartyLicence[] = ${JSON.stringify(rust, null, 2)};

export const NPM_DEPENDENCIES: readonly ThirdPartyLicence[] = ${JSON.stringify(npm, null, 2)};
`;

const targets = [
  [path.join(ROOT, "THIRD-PARTY-LICENSES.md"), markdown],
  [path.join(ROOT, "src", "generated", "third-party-licenses.ts"), generated],
];

if (unreviewed.length > 0) {
  console.error("generate-notices: licences outside the reviewed-permissive set:\n");
  for (const e of unreviewed) {
    console.error(`  ${e.name}@${e.version}  ${e.license || "(none declared)"}`);
  }
  console.error(
    "\nThis is not automatically a problem — it means nobody has looked yet.\n" +
      "Check the terms, then either add the identifier to PERMISSIVE in this\n" +
      "script (with the reason) or drop the dependency. docs/01_VISION.md\n" +
      "promises no copyleft in the shipped binary.",
  );
  process.exit(1);
}

let stale = false;
for (const [file, content] of targets) {
  const current = existsSync(file) ? readFileSync(file, "utf8") : null;
  if (current === content) continue;
  stale = true;
  if (CHECK_ONLY) {
    console.error(`generate-notices: ${path.relative(ROOT, file)} is out of date.`);
  } else {
    mkdirSync(path.dirname(file), { recursive: true });
    writeFileSync(file, content);
    console.log(`generate-notices: wrote ${path.relative(ROOT, file)}`);
  }
}

if (CHECK_ONLY && stale) {
  console.error("Run `npm run licenses` and commit the result.");
  process.exit(1);
}

console.log(
  `generate-notices: ${rust.length} crates, ${npm.length} npm packages, ${BUNDLED.length} bundled — all permissive.`,
);
