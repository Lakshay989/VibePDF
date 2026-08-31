// SPEC: P4-EDIT-001 (P4.B1) — the click-to-edit overlay: in edit-text mode it
// lays a hit-zone over each run; clicking one opens an editor prefilled with the
// run's text; committing calls replaceTextRun. IPC mocked.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";

import type { TextRun } from "@/ipc/text-runs";

// Hoisted so the vi.mock factory (also hoisted) can reference it.
const { RUN } = vi.hoisted(() => ({
  RUN: {
    text: "Hello, VibePDF.",
    bbox: [72, 700, 200, 712],
    fontName: "Helvetica",
    embedded: false,
    fontSize: 12,
    color: "#000000",
    transform: [1, 0, 0, 1, 72, 700],
  } satisfies TextRun,
}));

vi.mock("@/ipc/text-runs", () => ({
  extractTextRuns: vi.fn().mockResolvedValue([RUN]),
}));
vi.mock("@/ipc/text-edit", () => ({
  replaceTextRun: vi.fn().mockResolvedValue({ canUndo: true, canRedo: false }),
  deleteTextRun: vi.fn().mockResolvedValue({ canUndo: true, canRedo: false }),
}));

import { extractTextRuns } from "@/ipc/text-runs";
import { deleteTextRun, replaceTextRun } from "@/ipc/text-edit";
import { useToolStore } from "@/state/tool-store";
import { TextEditLayer } from "@/view/text-edit-layer";

const DOC = "doc-1";
const mockReplace = vi.mocked(replaceTextRun);
const mockDelete = vi.mocked(deleteTextRun);

const layer = () => (
  <TextEditLayer documentId={DOC} page={0} displayedWidth={612} displayedHeight={792} scale={1} rotation={0} />
);

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});
beforeEach(() => {
  useToolStore.setState({ activeTool: "edit-text" });
  vi.mocked(extractTextRuns).mockResolvedValue([RUN]);
});

describe("TextEditLayer", () => {
  it("renders a hit-zone per run and opens an editor prefilled with the run text", async () => {
    render(layer());
    const zone = await screen.findByTitle("Click to edit this text");
    fireEvent.click(zone);

    const input = screen.getByLabelText("Edit text run") as HTMLInputElement;
    expect(input.value).toBe("Hello, VibePDF.");
    // The non-embedded cue is shown (A2 interplay).
    expect(screen.getByText(/font not embedded/i)).toBeTruthy();
  });

  it("commits an edit through replaceTextRun with the run index", async () => {
    render(layer());
    fireEvent.click(await screen.findByTitle("Click to edit this text"));

    const input = screen.getByLabelText("Edit text run");
    fireEvent.change(input, { target: { value: "Hello, a mainstream reader." } });
    fireEvent.click(screen.getByRole("button", { name: "Save text edit" }));

    expect(mockReplace).toHaveBeenCalledWith(DOC, 0, 0, "Hello, a mainstream reader.");
  });

  it("deletes a run via deleteTextRun with the run index", async () => {
    render(layer());
    fireEvent.click(await screen.findByTitle("Click to edit this text"));
    fireEvent.click(screen.getByRole("button", { name: "Delete text run" }));
    expect(mockDelete).toHaveBeenCalledWith(DOC, 0, 0);
    expect(mockReplace).not.toHaveBeenCalled();
  });

  it("does not call replaceTextRun for an unchanged edit", async () => {
    render(layer());
    fireEvent.click(await screen.findByTitle("Click to edit this text"));
    // Commit without changing anything.
    fireEvent.click(screen.getByRole("button", { name: "Save text edit" }));
    expect(mockReplace).not.toHaveBeenCalled();
  });

  it("clearing the text + Save deletes the run (not replace-with-empty)", async () => {
    render(layer());
    fireEvent.click(await screen.findByTitle("Click to edit this text"));
    fireEvent.change(screen.getByLabelText("Edit text run"), { target: { value: "" } });
    fireEvent.click(screen.getByRole("button", { name: "Save text edit" }));
    expect(mockDelete).toHaveBeenCalledWith(DOC, 0, 0);
    expect(mockReplace).not.toHaveBeenCalled();
  });

  it("renders nothing when the edit-text tool is inactive", () => {
    useToolStore.setState({ activeTool: null });
    const { container } = render(layer());
    expect(container.firstChild).toBeNull();
  });
});
