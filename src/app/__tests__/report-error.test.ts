import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { reportError, toastMessage } from "@/app/report-error";
import { CommandError } from "@/ipc/invoke";
import { useToastStore } from "@/state/toast-store";

describe("toastMessage", () => {
  it("shows InvalidInput messages verbatim (they're user-authored)", () => {
    const err = new CommandError({ code: "InvalidInput", message: "text has characters we can't render" });
    expect(toastMessage("Couldn't add text", err)).toBe("text has characters we can't render");
  });

  it("prefixes other CommandError codes with the context", () => {
    const err = new CommandError({ code: "PdfError", message: "malformed page" });
    expect(toastMessage("Couldn't add link", err)).toBe("Couldn't add link: malformed page");
  });

  it("prefixes a plain Error with the context", () => {
    expect(toastMessage("Couldn't save", new Error("disk full"))).toBe("Couldn't save: disk full");
  });

  it("falls back to the bare context for non-error throws", () => {
    expect(toastMessage("Couldn't add note", "weird")).toBe("Couldn't add note");
  });
});

describe("reportError", () => {
  beforeEach(() => {
    useToastStore.getState().clear();
    vi.spyOn(console, "warn").mockImplementation(() => {});
  });
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("pushes an error toast and logs for developers", () => {
    reportError("Couldn't add link", new CommandError({ code: "PdfError", message: "boom" }));
    const toasts = useToastStore.getState().toasts;
    expect(toasts).toHaveLength(1);
    expect(toasts[0]).toMatchObject({ kind: "error", message: "Couldn't add link: boom" });
    expect(console.warn).toHaveBeenCalledOnce();
  });
});
