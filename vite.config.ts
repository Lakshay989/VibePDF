import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { fileURLToPath, URL } from "node:url";

// Tauri exposes the dev-server host via env vars; we forward them so HMR
// works in the Tauri webview on macOS/Linux/Windows alike.
const host = process.env.TAURI_DEV_HOST;

export default defineConfig(async () => ({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      "@": fileURLToPath(new URL("./src", import.meta.url)),
    },
  },
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host ?? false,
    hmr: host
      ? { protocol: "ws", host, port: 1421 }
      : undefined,
    // Don't reload on changes to the git-ignored sample-PDF folders (the
    // downloaded corpus + scratch can be large and shouldn't drive HMR).
    watch: { ignored: ["**/src-tauri/**", "**/Sample PDFs/**", "**/TestPDFs/**"] },
  },
  // PDF.js ships its worker as an ESM file. Vite needs to know not to
  // pre-bundle it; the worker is loaded by URL at runtime from /public.
  optimizeDeps: {
    exclude: ["pdfjs-dist"],
  },
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["./src/test-setup.ts"],
    // PDF.js explicitly tells you to use the `legacy` build in Node-ish
    // environments (jsdom counts). The default ESM build assumes a
    // browser runtime with DOMMatrix, `Uint8Array.prototype.toHex`,
    // `Promise.try`, etc. — none of which are guaranteed on Node 22.4
    // (pdfjs-dist@5.7.284 declares `engines: { node: ">=22.13.0" }`).
    // The legacy bundle self-polyfills the lot via core-js, so we
    // redirect bare `pdfjs-dist` imports to it. Regex `find` matches
    // exactly so explicit subpath imports (e.g. the worker preload in
    // src/test-setup.ts) still resolve normally.
    alias: [
      { find: /^pdfjs-dist$/, replacement: "pdfjs-dist/legacy/build/pdf.mjs" },
    ],
  },
}));
