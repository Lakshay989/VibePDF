import { useEffect, useState } from "react";

// SPEC: P1-VIEW-010 (P1.C5).
//
// React hook that returns `true` while the `.dark` class is set on
// <html>. The class is owned by src/app/theme.ts; this hook only
// observes it. A MutationObserver picks up changes whether they came
// from the user toggling the theme or from the "system" mode
// reacting to the OS-level preference change.

const isDarkNow = () =>
  typeof document !== "undefined" &&
  document.documentElement.classList.contains("dark");

export function useDarkMode(): boolean {
  const [dark, setDark] = useState<boolean>(isDarkNow);

  useEffect(() => {
    const update = () => setDark(isDarkNow());
    const observer = new MutationObserver(update);
    observer.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ["class"],
    });
    update();
    return () => observer.disconnect();
  }, []);

  return dark;
}
