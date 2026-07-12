import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { TOAST_TTL_MS, useToastStore } from "@/state/toast-store";

describe("toast-store", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    useToastStore.getState().clear();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("pushes a toast and returns its id", () => {
    const id = useToastStore.getState().push("error", "boom");
    const toasts = useToastStore.getState().toasts;
    expect(toasts).toHaveLength(1);
    expect(toasts[0]).toMatchObject({ id, kind: "error", message: "boom" });
  });

  it("auto-dismisses after the TTL", () => {
    useToastStore.getState().push("info", "hi");
    expect(useToastStore.getState().toasts).toHaveLength(1);
    vi.advanceTimersByTime(TOAST_TTL_MS - 1);
    expect(useToastStore.getState().toasts).toHaveLength(1);
    vi.advanceTimersByTime(1);
    expect(useToastStore.getState().toasts).toHaveLength(0);
  });

  it("dismisses by id without touching siblings", () => {
    const a = useToastStore.getState().push("error", "a");
    useToastStore.getState().push("error", "b");
    useToastStore.getState().dismiss(a);
    const msgs = useToastStore.getState().toasts.map((t) => t.message);
    expect(msgs).toEqual(["b"]);
  });

  it("assigns distinct ids across pushes", () => {
    const a = useToastStore.getState().push("error", "a");
    const b = useToastStore.getState().push("error", "b");
    expect(a).not.toBe(b);
  });
});
