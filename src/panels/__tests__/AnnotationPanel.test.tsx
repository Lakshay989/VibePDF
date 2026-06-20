// SPEC: P3-ANN-008 (P3.D1) — the annotation sidebar renders the read-back list
// grouped by page, narrows on search/filter, and on click navigates + selects.
// IPC is mocked — this asserts the panel's wiring, not the Rust read path.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, waitFor } from "@testing-library/react";

vi.mock("@/ipc/annotations", async (orig) => ({
  ...(await orig<typeof import("@/ipc/annotations")>()),
  readAnnotations: vi.fn(),
  deleteAnnotation: vi.fn().mockResolvedValue({ canUndo: true, canRedo: false }),
}));
vi.mock("@/ipc/freetext", () => ({
  readFreeText: vi.fn().mockResolvedValue({
    rect: [10, 20, 110, 60],
    text: "cap",
    fontFamily: "Helvetica",
    fontSize: 14,
    color: "#000000",
    bold: false,
    italic: false,
  }),
}));

import { type AnnotationInfo, deleteAnnotation, readAnnotations } from "@/ipc/annotations";
import { readFreeText } from "@/ipc/freetext";
import { AnnotationPanel } from "@/panels/AnnotationPanel";
import { useAnnotationEditStore } from "@/state/annotation-edit-store";
import { useAnnotationSelectionStore } from "@/state/annotation-selection-store";

const mockRead = vi.mocked(readAnnotations);
const mockDelete = vi.mocked(deleteAnnotation);
const mockReadFt = vi.mocked(readFreeText);

const rows: AnnotationInfo[] = [
  { id: "h1", page: 0, kind: "highlight", rect: [10, 20, 110, 30], contents: "important", author: "Bo", modified: null },
  { id: "n1", page: 2, kind: "note", rect: [100, 700, 118, 718], contents: "hello world", author: "Ada", modified: 2000 },
];

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});
beforeEach(() => {
  useAnnotationSelectionStore.setState({ selected: null });
  useAnnotationEditStore.setState({ editing: null });
  mockRead.mockResolvedValue(rows);
});

describe("AnnotationPanel", () => {
  it("lists annotations grouped by page", async () => {
    const { container } = render(<AnnotationPanel documentId="doc-1" epoch={0} onJump={vi.fn()} />);
    await waitFor(() => expect(container.textContent).toContain("important"));
    expect(container.textContent).toContain("Page 1");
    expect(container.textContent).toContain("Page 3");
    expect(container.textContent).toContain("hello world");
  });

  it("navigates and selects on click", async () => {
    const onJump = vi.fn();
    const { container } = render(<AnnotationPanel documentId="doc-1" epoch={0} onJump={onJump} />);
    await waitFor(() => expect(container.textContent).toContain("hello world"));

    fireEvent.click(container.querySelector('button[aria-label="Note on page 3"]') as Element);
    expect(onJump).toHaveBeenCalledWith(3); // page index 2 → 1-based 3
    expect(useAnnotationSelectionStore.getState().selected?.id).toBe("n1");
  });

  it("narrows the list as you search", async () => {
    const { container } = render(<AnnotationPanel documentId="doc-1" epoch={0} onJump={vi.fn()} />);
    await waitFor(() => expect(container.textContent).toContain("important"));

    fireEvent.change(container.querySelector('input[aria-label="Search annotations"]') as Element, {
      target: { value: "hello" },
    });
    expect(container.textContent).toContain("hello world");
    expect(container.textContent).not.toContain("important");
  });

  it("filters by kind via the chips", async () => {
    const { container } = render(<AnnotationPanel documentId="doc-1" epoch={0} onJump={vi.fn()} />);
    await waitFor(() => expect(container.textContent).toContain("important"));

    // Click the "Note" kind chip → only the note remains.
    const chip = [...container.querySelectorAll("button")].find((b) => b.textContent === "Note");
    fireEvent.click(chip as Element);
    expect(container.textContent).toContain("hello world");
    expect(container.textContent).not.toContain("important");
  });

  it("deletes a row: calls deleteAnnotation with its handle and clears selection", async () => {
    const { container } = render(<AnnotationPanel documentId="doc-1" epoch={0} onJump={vi.fn()} />);
    await waitFor(() => expect(container.textContent).toContain("hello world"));

    // Select then delete the note row.
    fireEvent.click(container.querySelector('button[aria-label="Note on page 3"]') as Element);
    expect(useAnnotationSelectionStore.getState().selected?.id).toBe("n1");

    fireEvent.click(container.querySelector('button[aria-label="Delete Note on page 3"]') as Element);
    expect(mockDelete).toHaveBeenCalledWith("doc-1", "n1");
    expect(useAnnotationSelectionStore.getState().selected).toBeNull();
  });

  it("deletes the selected annotation on the Delete key", async () => {
    const { container } = render(<AnnotationPanel documentId="doc-1" epoch={0} onJump={vi.fn()} />);
    await waitFor(() => expect(container.textContent).toContain("important"));

    fireEvent.click(container.querySelector('button[aria-label="Highlight on page 1"]') as Element);
    fireEvent.keyDown(window, { key: "Delete" });
    expect(mockDelete).toHaveBeenCalledWith("doc-1", "h1");
  });

  it("edits a free-text row: reads it and posts an edit request", async () => {
    const ft: AnnotationInfo = {
      id: "f1",
      page: 0,
      kind: "freetext",
      rect: [10, 20, 110, 60],
      contents: "cap",
      author: "",
      modified: null,
    };
    mockRead.mockResolvedValueOnce([ft]);
    const onJump = vi.fn();
    const { container } = render(<AnnotationPanel documentId="doc-1" epoch={0} onJump={onJump} />);
    await waitFor(() => expect(container.textContent).toContain("cap"));

    fireEvent.click(container.querySelector('button[aria-label="Edit free text on page 1"]') as Element);
    expect(mockReadFt).toHaveBeenCalledWith("doc-1", "f1");
    await waitFor(() => expect(useAnnotationEditStore.getState().editing?.nm).toBe("f1"));
    expect(onJump).toHaveBeenCalledWith(1);
  });

  it("shows an empty state with no annotations", async () => {
    mockRead.mockResolvedValueOnce([]);
    const { container } = render(<AnnotationPanel documentId="doc-1" epoch={0} onJump={vi.fn()} />);
    await waitFor(() => expect(container.textContent).toContain("No annotations."));
  });
});
