// SPEC: P3-ANN-008 (P3.D1) — the annotation sidebar renders the read-back list
// grouped by page, narrows on search/filter, and on click navigates + selects.
// IPC is mocked — this asserts the panel's wiring, not the Rust read path.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, waitFor } from "@testing-library/react";

vi.mock("@/ipc/annotations", async (orig) => ({
  ...(await orig<typeof import("@/ipc/annotations")>()),
  readAnnotations: vi.fn(),
}));

import { type AnnotationInfo, readAnnotations } from "@/ipc/annotations";
import { AnnotationPanel } from "@/panels/AnnotationPanel";
import { useAnnotationSelectionStore } from "@/state/annotation-selection-store";

const mockRead = vi.mocked(readAnnotations);

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

  it("shows an empty state with no annotations", async () => {
    mockRead.mockResolvedValueOnce([]);
    const { container } = render(<AnnotationPanel documentId="doc-1" epoch={0} onJump={vi.fn()} />);
    await waitFor(() => expect(container.textContent).toContain("No annotations."));
  });
});
