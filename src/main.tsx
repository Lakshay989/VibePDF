import "@/polyfills";

import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "@/app/App";
import "@/styles/globals.css";
import { applyInitialTheme } from "@/app/theme";

applyInitialTheme();

const rootEl = document.getElementById("root");
if (!rootEl) throw new Error("VibePDF: #root not found in index.html");

createRoot(rootEl).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
