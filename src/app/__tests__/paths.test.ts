import { describe, expect, it } from "vitest";

import { basename } from "@/app/paths";

// FABLE_REVIEW §3.9 (P4.HF15) — `basename` must be correct on Windows, where
// paths use `\` (a plain `split("/")` returns the whole string). This is the
// regression guard for the bug the review found in the watermark/background
// dialogs, which now route their filename display through this helper.
describe("basename", () => {
  it("returns the last segment of a POSIX path", () => {
    expect(basename("/Users/x/docs/file.pdf")).toBe("file.pdf");
  });

  it("returns the last segment of a Windows backslash path", () => {
    expect(basename("C:\\Users\\x\\docs\\file.pdf")).toBe("file.pdf");
  });

  it("handles UNC paths", () => {
    expect(basename("\\\\server\\share\\report.pdf")).toBe("report.pdf");
  });

  it("takes whichever separator appears rightmost in a mixed path", () => {
    expect(basename("C:/Users\\x/mixed.pdf")).toBe("mixed.pdf");
    expect(basename("/home/x\\weird")).toBe("weird");
  });

  it("returns the input unchanged when there is no separator", () => {
    expect(basename("file.pdf")).toBe("file.pdf");
    expect(basename("")).toBe("");
  });

  it("yields an empty string for a trailing separator", () => {
    expect(basename("/Users/x/")).toBe("");
    expect(basename("C:\\Users\\x\\")).toBe("");
  });
});
