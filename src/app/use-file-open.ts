// SPEC: P1-VIEW-001 + P1-VIEW-003 + P1-VIEW-012 — the "open a file"
// subsystem, extracted from App.tsx so the component is just layout.
//
// Owns everything involved in turning a path (or a user gesture) into
// an open document tab: the password-prompt state machine, the toast
// surface those flows write to, the single `openByPath` orchestrator,
// the file dialog (Cmd/Ctrl+O + header button), and the drag-drop
// listener. Recents are hydrated here too since this is the subsystem
// that writes them.

import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useRef, useState } from "react";

import { registerDragDrop } from "@/app/drag-drop";
import {
  openWithPasswordPrompt,
  type AskForPassword,
  type PasswordPromptRequest,
} from "@/app/open-with-password";
import { useDocumentStore } from "@/state/document-store";
import { useSettingsStore } from "@/state/settings-store";

export interface PasswordDialogProps {
  request: PasswordPromptRequest | null;
  onSubmit: (password: string) => void;
  onCancel: () => void;
}

export interface UseFileOpen {
  /** Open one PDF by path: password retry, recents, toast on failure. */
  openByPath: (path: string) => Promise<void>;
  /** Show the native file picker, then open the selection. */
  pickAndOpen: () => Promise<void>;
  /** Transient status message; `null` when nothing is shown. */
  toast: string | null;
  /** Spread onto `<PasswordPromptDialog>`. */
  passwordDialogProps: PasswordDialogProps;
}

export function useFileOpen(): UseFileOpen {
  const openDoc = useDocumentStore((s) => s.openDoc);
  const hydrateRecents = useSettingsStore((s) => s.hydrateRecents);
  const pushRecent = useSettingsStore((s) => s.pushRecent);

  const [toast, setToast] = useState<string | null>(null);

  // SPEC: P1-VIEW-012 — load the persisted recents once on mount.
  useEffect(() => {
    void hydrateRecents();
  }, [hydrateRecents]);

  // SPEC: P1-VIEW-003 — password-prompt state.
  // `prompt` non-null means the dialog is mounted with these args.
  // `resolveRef` carries the in-flight `Promise<string | null>`'s
  // resolver so the dialog buttons can settle the awaiting retry loop.
  const [prompt, setPrompt] = useState<PasswordPromptRequest | null>(null);
  const resolveRef = useRef<((pwd: string | null) => void) | null>(null);

  const askForPassword: AskForPassword = useCallback(
    (req) =>
      new Promise<string | null>((resolve) => {
        resolveRef.current = resolve;
        setPrompt(req);
      }),
    [],
  );

  const handleDialogSubmit = useCallback((pwd: string) => {
    // The retry loop will either succeed (we'll clear `prompt` below)
    // or re-prompt with new args (which replaces `prompt` and clears
    // the dialog input via PasswordPromptDialog's effect).
    resolveRef.current?.(pwd);
    resolveRef.current = null;
  }, []);

  const handleDialogCancel = useCallback(() => {
    resolveRef.current?.(null);
    resolveRef.current = null;
    setPrompt(null);
  }, []);

  // SPEC: P1-VIEW-001 + P1-VIEW-003 — single entry point for any
  // path-driven open. The Cmd/Ctrl+O path, the header button, the
  // drag-drop callback, recents clicks, and the CLI drain all converge
  // here. Encrypted opens retry up to 3 times via
  // `openWithPasswordPrompt`; terminal failure surfaces as a toast.
  const openByPath = useCallback(
    async (path: string) => {
      try {
        const result = await openWithPasswordPrompt(path, askForPassword);
        switch (result.outcome) {
          case "opened":
            openDoc(result.doc);
            setPrompt(null);
            // SPEC: P1-VIEW-012 — only successful opens count as recent.
            void pushRecent(result.doc.path);
            break;
          case "cancelled":
            // User dismissed the dialog. handleDialogCancel already
            // cleared `prompt`. No toast — explicit user action.
            break;
          case "failed":
            setPrompt(null);
            setToast("Could not unlock.");
            break;
        }
      } catch (err) {
        // Non-password errors (NotFound, PdfError, etc.). Keep the
        // pre-B2 behaviour: log + best-effort toast.
        setPrompt(null);
        console.warn("openByPath failed", path, err);
        setToast(err instanceof Error ? err.message : "Could not open file.");
      }
    },
    [askForPassword, openDoc, pushRecent],
  );

  const pickAndOpen = useCallback(async () => {
    const selected = await openFileDialog({
      multiple: false,
      filters: [{ name: "PDF", extensions: ["pdf"] }],
    });
    if (selected && typeof selected === "string") {
      await openByPath(selected);
    }
  }, [openByPath]);

  // Cmd/Ctrl+O → file picker.
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      const cmd = e.metaKey || e.ctrlKey;
      if (cmd && e.key.toLowerCase() === "o") {
        e.preventDefault();
        void pickAndOpen();
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [pickAndOpen]);

  // SPEC: P1-VIEW-001 (P1.A1) — drag-drop file open.
  // SPEC: P1-VIEW-003 — encrypted drops route through the same prompt.
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    void registerDragDrop(
      ({ opened, rejected }) => {
        for (const doc of opened) {
          openDoc(doc);
          // SPEC: P1-VIEW-012 — dropped opens count as recent too.
          void pushRecent(doc.path);
        }
        if (rejected.length > 0) {
          setToast(
            rejected.length === 1
              ? "Only .pdf files are accepted."
              : `${rejected.length} files were ignored — only .pdf is accepted.`,
          );
        }
      },
      askForPassword,
    ).then((u) => {
      unlisten = u;
    });
    return () => {
      unlisten?.();
    };
  }, [openDoc, askForPassword, pushRecent]);

  // Auto-dismiss the toast.
  useEffect(() => {
    if (!toast) return;
    const id = window.setTimeout(() => setToast(null), 3000);
    return () => window.clearTimeout(id);
  }, [toast]);

  return {
    openByPath,
    pickAndOpen,
    toast,
    passwordDialogProps: {
      request: prompt,
      onSubmit: handleDialogSubmit,
      onCancel: handleDialogCancel,
    },
  };
}
