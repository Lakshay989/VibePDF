// SPEC: P1-VIEW-003 — the password retry loop. Pure-ish glue: it takes
// an injected `askForPassword` and only depends on a mockable
// `openPdfPath`, so the "retry up to 3 times" contract is unit-testable
// without any DOM or Tauri runtime.

import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  MAX_PASSWORD_ATTEMPTS,
  openWithPasswordPrompt,
} from "@/app/open-with-password";
import { CommandError } from "@/ipc/invoke";
import type { OpenedDocument } from "@/ipc/pdf";

vi.mock("@/ipc/pdf", () => ({ openPdfPath: vi.fn() }));

// Imported after the mock factory above so this binding is the mock.
import { openPdfPath } from "@/ipc/pdf";
const mockOpen = vi.mocked(openPdfPath);

function doc(path: string): OpenedDocument {
  return {
    id: `id:${path}`,
    path,
    name: path,
    pageCount: 1,
    title: null,
    author: null,
    pdfVersion: null,
  };
}

const passwordRequired = () =>
  new CommandError({ code: "PasswordRequired", message: "/enc.pdf" });

beforeEach(() => {
  mockOpen.mockReset();
});

describe("openWithPasswordPrompt", () => {
  it("opens an unencrypted PDF silently, never prompting", async () => {
    mockOpen.mockResolvedValueOnce(doc("/a.pdf"));
    const ask = vi.fn();

    const result = await openWithPasswordPrompt("/a.pdf", ask);

    expect(result).toEqual({ outcome: "opened", doc: doc("/a.pdf") });
    expect(ask).not.toHaveBeenCalled();
    expect(mockOpen).toHaveBeenCalledTimes(1);
    expect(mockOpen).toHaveBeenCalledWith("/a.pdf");
  });

  it("prompts once and opens when the first password is correct", async () => {
    mockOpen
      .mockRejectedValueOnce(passwordRequired()) // silent attempt
      .mockResolvedValueOnce(doc("/enc.pdf")); // with password
    const ask = vi.fn().mockResolvedValueOnce("correct");

    const result = await openWithPasswordPrompt("/enc.pdf", ask);

    expect(result).toEqual({ outcome: "opened", doc: doc("/enc.pdf") });
    expect(ask).toHaveBeenCalledTimes(1);
    expect(ask).toHaveBeenCalledWith({
      path: "/enc.pdf",
      attemptsLeft: 3,
      lastError: null,
    });
    expect(mockOpen).toHaveBeenNthCalledWith(2, "/enc.pdf", "correct");
  });

  it("returns 'failed' after exactly 3 wrong passwords, with a ticking counter", async () => {
    mockOpen.mockRejectedValue(passwordRequired()); // every attempt fails
    const ask = vi.fn().mockResolvedValue("wrong");

    const result = await openWithPasswordPrompt("/enc.pdf", ask);

    expect(result).toEqual({ outcome: "failed" });
    // Prompted exactly 3 times — the spec's "retry up to 3 times".
    expect(ask).toHaveBeenCalledTimes(3);
    expect(ask).toHaveBeenNthCalledWith(1, {
      path: "/enc.pdf",
      attemptsLeft: 3,
      lastError: null,
    });
    expect(ask).toHaveBeenNthCalledWith(2, {
      path: "/enc.pdf",
      attemptsLeft: 2,
      lastError: "Incorrect password.",
    });
    expect(ask).toHaveBeenNthCalledWith(3, {
      path: "/enc.pdf",
      attemptsLeft: 1,
      lastError: "Incorrect password.",
    });
    // 1 silent + 3 prompted opens.
    expect(mockOpen).toHaveBeenCalledTimes(4);
  });

  it("returns 'cancelled' when the user dismisses the prompt", async () => {
    mockOpen.mockRejectedValueOnce(passwordRequired());
    const ask = vi.fn().mockResolvedValueOnce(null);

    const result = await openWithPasswordPrompt("/enc.pdf", ask);

    expect(result).toEqual({ outcome: "cancelled" });
    expect(ask).toHaveBeenCalledTimes(1);
    // No second open attempt after cancel.
    expect(mockOpen).toHaveBeenCalledTimes(1);
  });

  it("propagates a non-password error from the silent attempt without prompting", async () => {
    const notFound = new CommandError({ code: "NotFound", message: "/gone.pdf" });
    mockOpen.mockRejectedValueOnce(notFound);
    const ask = vi.fn();

    await expect(openWithPasswordPrompt("/gone.pdf", ask)).rejects.toBe(notFound);
    expect(ask).not.toHaveBeenCalled();
  });

  it("propagates a non-password error that surfaces mid-retry", async () => {
    mockOpen
      .mockRejectedValueOnce(passwordRequired()) // silent → prompt
      .mockRejectedValueOnce(new CommandError({ code: "PdfError", message: "boom" }));
    const ask = vi.fn().mockResolvedValueOnce("whatever");

    await expect(openWithPasswordPrompt("/enc.pdf", ask)).rejects.toThrow("boom");
  });

  it("caps prompts at MAX_PASSWORD_ATTEMPTS (= 3, the spec clause)", () => {
    expect(MAX_PASSWORD_ATTEMPTS).toBe(3);
  });
});
