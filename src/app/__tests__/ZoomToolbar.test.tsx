import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";

import { ZoomToolbar } from "@/app/ZoomToolbar";
import { useSettingsStore } from "@/state/settings-store";

afterEach(cleanup);
beforeEach(() => {
  // Reset shared singletons so tests don't leak into each other.
  useSettingsStore.setState({ theme: "system" });
  document.documentElement.classList.remove("dark");
});

// SPEC: P1-VIEW-010 — regression guard for the theme control.
//
// The dark/light/system machinery existed for a while with NO UI wired
// to it (the user was stuck following the OS). A render test asserting
// the control is present AND two-way-bound catches that "feature
// shipped but unreachable" class, which unit tests on the theme *logic*
// can't see.
describe("ZoomToolbar theme control", () => {
  it("renders a Theme selector with light / dark / system", () => {
    render(<ZoomToolbar />);
    const select = screen.getByLabelText("Theme") as HTMLSelectElement;
    const values = Array.from(select.options).map((o) => o.value);
    expect(values).toEqual(["system", "light", "dark"]);
  });

  it("reflects the store's current theme", () => {
    useSettingsStore.setState({ theme: "dark" });
    render(<ZoomToolbar />);
    expect((screen.getByLabelText("Theme") as HTMLSelectElement).value).toBe(
      "dark",
    );
  });

  it("writes the chosen theme back to the store on change", () => {
    render(<ZoomToolbar />);
    const select = screen.getByLabelText("Theme") as HTMLSelectElement;
    fireEvent.change(select, { target: { value: "light" } });
    expect(useSettingsStore.getState().theme).toBe("light");
  });

  it("also surfaces the Pages and Outline sidebar toggles", () => {
    render(<ZoomToolbar />);
    expect(screen.getByLabelText("Toggle thumbnails sidebar")).toBeTruthy();
    expect(screen.getByLabelText("Toggle outline sidebar")).toBeTruthy();
  });
});

// SPEC: P7-OCR-004 (P7.B1a) — the same "shipped but unreachable" guard as the
// theme control above, for the conversions.
//
// This is the failure P7.A4 was written to fix: Track A built an OCR engine
// that nothing in the app could reach, and the phase's own acceptance demo
// could not be performed. A handler that is never wired to a button is a
// feature that does not exist, and no amount of backend testing sees it.
describe("ZoomToolbar conversion entries", () => {
  it("shows an entry for each conversion only when a handler is supplied", () => {
    const { rerender } = render(<ZoomToolbar />);
    for (const label of [/Export to Word/, /Export to Excel/, /Export images/, /Export text/, /Compress/]) {
      expect(screen.queryByRole("button", { name: label })).toBeNull();
    }

    const calls: string[] = [];
    rerender(
      <ZoomToolbar
        onExportDocx={() => calls.push("docx")}
        onExportXlsx={() => calls.push("xlsx")}
        onExportImages={() => calls.push("images")}
        onExportText={() => calls.push("text")}
        onCompress={() => calls.push("compress")}
      />,
    );
    for (const [label, expected] of [
      [/Export to Word/, "docx"],
      [/Export to Excel/, "xlsx"],
      [/Export images/, "images"],
      [/Export text/, "text"],
      [/Compress/, "compress"],
    ] as const) {
      const button = screen.getByRole("button", { name: label });
      fireEvent.click(button);
      expect(calls).toContain(expected);
    }
  });
});
