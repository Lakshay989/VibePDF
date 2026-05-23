// SPEC: P1-VIEW-010 — light / dark / system themes.
// The chosen theme is persisted to localStorage; resolution to the
// effective .dark class on <html> happens on every startup AND whenever
// the OS theme changes while "system" is selected.

export type Theme = "light" | "dark" | "system";

const STORAGE_KEY = "vibepdf.theme";

export function getStoredTheme(): Theme {
  const raw = localStorage.getItem(STORAGE_KEY);
  return raw === "light" || raw === "dark" || raw === "system" ? raw : "system";
}

export function setStoredTheme(theme: Theme): void {
  localStorage.setItem(STORAGE_KEY, theme);
  applyTheme(theme);
}

function prefersDark(): boolean {
  return window.matchMedia("(prefers-color-scheme: dark)").matches;
}

export function applyTheme(theme: Theme): void {
  const effectiveDark = theme === "dark" || (theme === "system" && prefersDark());
  document.documentElement.classList.toggle("dark", effectiveDark);
}

export function applyInitialTheme(): void {
  const theme = getStoredTheme();
  applyTheme(theme);

  // Keep "system" responsive to OS-level changes.
  window
    .matchMedia("(prefers-color-scheme: dark)")
    .addEventListener("change", () => {
      if (getStoredTheme() === "system") applyTheme("system");
    });
}
