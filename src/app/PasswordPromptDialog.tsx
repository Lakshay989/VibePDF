// SPEC: P1-VIEW-003 — modal that collects a password for an
// encrypted PDF. Pure controlled component: parent owns the prompt
// state (path / attemptsLeft / lastError / open/closed), this file
// owns only the in-input password string.
//
// The password lives in `useState`; the `useEffect` cleanup clears
// it on unmount and on every prompt-args change. It is never written
// to disk, never sent through tracing, and never crosses any module
// boundary except via the parent-supplied `onSubmit` callback (which
// passes it directly to `openPdfPath` and lets it be GC'd).

import * as Dialog from "@radix-ui/react-dialog";
import { useEffect, useRef, useState } from "react";

import { basename } from "@/app/paths";
import type { PasswordPromptRequest } from "@/app/open-with-password";

export interface PasswordPromptDialogProps {
  /**
   * Non-null while the dialog should be mounted. `null` closes the
   * dialog; the parent uses this both for the cancel path and after a
   * successful open / terminal failure.
   */
  request: PasswordPromptRequest | null;
  onSubmit: (password: string) => void;
  onCancel: () => void;
}

export function PasswordPromptDialog({
  request,
  onSubmit,
  onCancel,
}: PasswordPromptDialogProps) {
  const [value, setValue] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);

  // Whenever the prompt args change (e.g. attemptsLeft ticks down after
  // a wrong password), clear the input and re-focus. On unmount we also
  // overwrite the state so the closed-over string is collectable.
  useEffect(() => {
    setValue("");
    if (request) {
      // Defer focus until the dialog mounts in the DOM.
      const id = window.setTimeout(() => inputRef.current?.focus(), 0);
      return () => {
        window.clearTimeout(id);
        setValue("");
      };
    }
    return () => setValue("");
  }, [request]);

  const open = request !== null;

  return (
    <Dialog.Root
      open={open}
      onOpenChange={(next) => {
        if (!next) onCancel();
      }}
    >
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 bg-black/40" />
        <Dialog.Content
          className="fixed left-1/2 top-1/2 w-[420px] -translate-x-1/2 -translate-y-1/2 rounded-lg bg-white p-5 shadow-xl dark:bg-neutral-900 dark:text-neutral-100"
          // Radix focuses the first focusable child on mount; that's
          // the input below, which is what we want.
        >
          <Dialog.Title className="text-base font-semibold">
            Password required
          </Dialog.Title>
          <Dialog.Description className="mt-1 text-sm text-neutral-600 dark:text-neutral-400">
            {request ? (
              <>
                <span className="font-mono">{basename(request.path)}</span> is
                encrypted. Enter the password to open it.
              </>
            ) : null}
          </Dialog.Description>

          <form
            className="mt-4 flex flex-col gap-3"
            onSubmit={(e) => {
              e.preventDefault();
              if (value.length === 0) return;
              onSubmit(value);
            }}
          >
            <input
              ref={inputRef}
              type="password"
              value={value}
              onChange={(e) => setValue(e.target.value)}
              autoComplete="off"
              spellCheck={false}
              aria-label="PDF password"
              aria-invalid={request?.lastError ? true : undefined}
              className="rounded border border-neutral-300 bg-white px-2 py-1.5 text-sm outline-none focus:border-neutral-500 dark:border-neutral-700 dark:bg-neutral-800"
            />

            {request?.lastError ? (
              <div
                role="alert"
                className="text-sm text-red-600 dark:text-red-400"
              >
                {request.lastError}{" "}
                {request.attemptsLeft === 1
                  ? "1 attempt left."
                  : `${request.attemptsLeft} attempts left.`}
              </div>
            ) : null}

            <div className="mt-1 flex justify-end gap-2">
              <button
                type="button"
                onClick={onCancel}
                className="rounded border border-neutral-300 px-3 py-1.5 text-sm hover:bg-neutral-100 dark:border-neutral-700 dark:hover:bg-neutral-800"
              >
                Cancel
              </button>
              <button
                type="submit"
                disabled={value.length === 0}
                className="rounded bg-neutral-900 px-3 py-1.5 text-sm text-white hover:bg-neutral-800 disabled:opacity-50 dark:bg-neutral-100 dark:text-neutral-900 dark:hover:bg-neutral-200"
              >
                Unlock
              </button>
            </div>
          </form>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
