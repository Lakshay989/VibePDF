import { describe, expect, it } from "vitest";

import { isPdfPath, partitionPaths } from "@/app/drag-drop";

describe("isPdfPath", () => {
  it("accepts .pdf in any case", () => {
    expect(isPdfPath("/x/y.pdf")).toBe(true);
    expect(isPdfPath("/x/Y.PDF")).toBe(true);
    expect(isPdfPath("/x/y.PdF")).toBe(true);
  });

  it("rejects non-pdf extensions and extension-less paths", () => {
    expect(isPdfPath("/x/y.txt")).toBe(false);
    expect(isPdfPath("/x/y")).toBe(false);
    expect(isPdfPath("/x/y.pdfx")).toBe(false);
    expect(isPdfPath("/x/y.pdf.txt")).toBe(false);
  });
});

describe("partitionPaths", () => {
  it("splits a mixed list into pdfs and others, preserving input order", () => {
    const { pdfs, others } = partitionPaths([
      "/a.pdf",
      "/b.txt",
      "/c.PDF",
      "/d",
    ]);
    expect(pdfs).toEqual(["/a.pdf", "/c.PDF"]);
    expect(others).toEqual(["/b.txt", "/d"]);
  });

  it("handles empty input", () => {
    expect(partitionPaths([])).toEqual({ pdfs: [], others: [] });
  });
});
