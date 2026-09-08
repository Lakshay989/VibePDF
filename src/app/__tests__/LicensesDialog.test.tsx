// The in-app licence list — the fourth obligation in THIRD-PARTY-NOTICES.md.
//
// What is worth pinning here is not the rendering. It is that the list is
// *complete* and *generated*: a licence view that silently drops a section, or
// that someone starts editing by hand, makes a false claim about what the
// binary contains, and nothing downstream catches it. So the tests assert the
// bundled components are present alongside the package trees, and that the
// counts shown come from the generated data rather than a literal.

import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";

import { LicensesDialog } from "@/app/LicensesDialog";
import {
  BUNDLED_COMPONENTS,
  NPM_DEPENDENCIES,
  RUST_DEPENDENCIES,
} from "@/generated/third-party-licenses";

afterEach(cleanup);

describe("LicensesDialog", () => {
  it("renders nothing when closed", () => {
    render(<LicensesDialog open={false} onClose={vi.fn()} />);
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("lists all three groups, bundled components included", () => {
    render(<LicensesDialog open onClose={vi.fn()} />);

    // PDFium is fetched rather than resolved by either package manager, so it
    // is exactly the entry a list built only from lockfiles would omit — and
    // it is the one carrying a real redistribution obligation.
    expect(screen.getByText(/^PDFium$/)).toBeTruthy();

    expect(screen.getByText(new RegExp(`Bundled components \\(${BUNDLED_COMPONENTS.length}`))).toBeTruthy();
    expect(screen.getByText(new RegExp(`Rust crates \\(${RUST_DEPENDENCIES.length}`))).toBeTruthy();
    expect(screen.getByText(new RegExp(`npm packages \\(${NPM_DEPENDENCIES.length}`))).toBeTruthy();
  });

  it("states the total, taken from the generated data", () => {
    render(<LicensesDialog open onClose={vi.fn()} />);
    const total = BUNDLED_COMPONENTS.length + RUST_DEPENDENCIES.length + NPM_DEPENDENCIES.length;
    expect(screen.getByText(new RegExp(`${total} third-party`))).toBeTruthy();
  });

  it("filters by name and by licence", () => {
    render(<LicensesDialog open onClose={vi.fn()} />);
    const box = screen.getByLabelText("Filter licences");

    fireEvent.change(box, { target: { value: "pdfium" } });
    expect(screen.getByText(/^PDFium$/)).toBeTruthy();
    expect(screen.queryByText(/^react$/)).toBeNull();

    // A licence match, not a name match — someone auditing asks "what is
    // Apache here?", not "is package X Apache?".
    fireEvent.change(box, { target: { value: "BSD-3-Clause" } });
    expect(screen.getByText(/^PDFium$/)).toBeTruthy();
  });

  it("hides a section that the filter empties rather than showing a zero", () => {
    render(<LicensesDialog open onClose={vi.fn()} />);
    fireEvent.change(screen.getByLabelText("Filter licences"), {
      target: { value: "zzzz-no-such-package" },
    });
    expect(screen.queryByText(/Rust crates \(/)).toBeNull();
    expect(screen.queryByText(/npm packages \(/)).toBeNull();
  });

  it("closes", () => {
    const onClose = vi.fn();
    render(<LicensesDialog open onClose={onClose} />);
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
