// SPEC: P5-FORM-009 (P5.C2) — the import action's pure helpers: picking the
// format from the chosen path, and rendering the report line that carries the
// spec's two "SHALL be reported" lists.

import { describe, expect, it } from "vitest";

import { describeImport, formatFromPath } from "@/tools/form-author/import-report";

describe("formatFromPath", () => {
  it("reads each supported extension", () => {
    expect(formatFromPath("/tmp/data.fdf")).toBe("fdf");
    expect(formatFromPath("/tmp/data.xfdf")).toBe("xfdf");
    expect(formatFromPath("/tmp/data.json")).toBe("json");
    expect(formatFromPath("/tmp/data.csv")).toBe("csv");
  });

  it("is case-insensitive and handles Windows paths", () => {
    expect(formatFromPath("C:\\Users\\ada\\Form Data.JSON")).toBe("json");
  });

  it("returns null for anything we don't read", () => {
    expect(formatFromPath("/tmp/data.xlsx")).toBeNull();
    expect(formatFromPath("/tmp/noextension")).toBeNull();
    // A dot only in a directory segment is not an extension.
    expect(formatFromPath("/tmp/v1.2/data")).toBeNull();
    expect(formatFromPath("/tmp/.json")).toBeNull();
  });
});

describe("describeImport", () => {
  it("reports the applied count alone when nothing was rejected", () => {
    expect(describeImport(3, [], [])).toBe("Filled 3 fields");
    expect(describeImport(1, [], [])).toBe("Filled 1 field");
  });

  it("names the unmatched fields", () => {
    expect(describeImport(2, ["nope", "gone"], [])).toContain("2 not in this form (nope, gone)");
  });

  it("spells out each type mismatch rather than counting them", () => {
    const line = describeImport(0, [], [{ name: "who", expected: "checkbox", got: "text" }]);
    expect(line).toContain("who: data says checkbox, field is text");
  });

  it("labels a typeless source (FDF/XFDF) honestly", () => {
    const line = describeImport(0, [], [{ name: "who", expected: "", got: "signature" }]);
    expect(line).toContain("who: data says no type, field is signature");
  });
});
