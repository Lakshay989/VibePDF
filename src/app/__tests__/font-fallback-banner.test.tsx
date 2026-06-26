// SPEC: P4-EDIT-002 (P4.A2) — the banner is the once-per-document warning. It
// must appear only when a font needs fallback, name every substitution, offer
// re-flow as a *disabled* affordance (the action lands with P4.B1), and hide
// on dismiss. A render test guards the "shipped but unreachable / misleading"
// class that logic unit tests can't see.

import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";

import { FontFallbackBanner } from "@/app/FontFallbackBanner";
import type { FontReport } from "@/ipc/fonts";

afterEach(cleanup);

const reportWithFallback: FontReport = {
  needsFallback: true,
  fonts: [
    { fontName: "Helvetica", embedded: false, status: "standard", substitute: null },
    { fontName: "Calibri", embedded: false, status: "fallback", substitute: "Helvetica" },
    { fontName: "Garamond", embedded: false, status: "fallback", substitute: "Times-Roman" },
  ],
};

describe("FontFallbackBanner", () => {
  it("lists each substituted font and offers a disabled re-flow", () => {
    render(
      <FontFallbackBanner report={reportWithFallback} dismissed={false} onDismiss={() => {}} />,
    );
    // Only the two fallback fonts are listed — the standard one is silent.
    expect(screen.getByText("Calibri → Helvetica")).toBeTruthy();
    expect(screen.getByText("Garamond → Times-Roman")).toBeTruthy();
    expect(screen.queryByText(/Helvetica → /)).toBeNull();

    const reflow = screen.getByRole("button", { name: /re-flow affected text/i });
    expect(reflow.hasAttribute("disabled")).toBe(true);
  });

  it("invokes onDismiss when the user dismisses it", () => {
    const onDismiss = vi.fn();
    render(
      <FontFallbackBanner report={reportWithFallback} dismissed={false} onDismiss={onDismiss} />,
    );
    fireEvent.click(screen.getByRole("button", { name: /dismiss font warning/i }));
    expect(onDismiss).toHaveBeenCalledOnce();
  });

  it("renders nothing when dismissed", () => {
    const { container } = render(
      <FontFallbackBanner report={reportWithFallback} dismissed onDismiss={() => {}} />,
    );
    expect(container.firstChild).toBeNull();
  });

  it("renders nothing when no font needs fallback", () => {
    const safe: FontReport = {
      needsFallback: false,
      fonts: [{ fontName: "Helvetica", embedded: false, status: "standard", substitute: null }],
    };
    const { container } = render(
      <FontFallbackBanner report={safe} dismissed={false} onDismiss={() => {}} />,
    );
    expect(container.firstChild).toBeNull();
  });

  it("renders nothing while the report is still loading", () => {
    const { container } = render(
      <FontFallbackBanner report={null} dismissed={false} onDismiss={() => {}} />,
    );
    expect(container.firstChild).toBeNull();
  });
});
