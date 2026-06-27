// SPEC: P4-EDIT-003 (P4.B2) — the addTextBox IPC wrapper marshals the box
// geometry + style to the Rust command and returns the history state.

import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/ipc/invoke", () => ({ invoke: vi.fn() }));

import { invoke } from "@/ipc/invoke";
import { addTextBox } from "@/ipc/text-box";

const mockInvoke = vi.mocked(invoke);

beforeEach(() => {
  mockInvoke.mockReset();
});

describe("addTextBox", () => {
  it("marshals rect + text + style and returns the history state", async () => {
    mockInvoke.mockResolvedValue({ canUndo: true, canRedo: false });

    const out = await addTextBox("doc-1", 2, [10, 20, 110, 60], "Hello", "Times", 18, "#102030", true, false, true);
    expect(mockInvoke).toHaveBeenCalledWith("pdf_add_text_box", {
      id: "doc-1",
      page: 2,
      rect: [10, 20, 110, 60],
      text: "Hello",
      fontFamily: "Times",
      fontSize: 18,
      color: "#102030",
      bold: true,
      italic: false,
      underline: true,
    });
    expect(out).toEqual({ canUndo: true, canRedo: false });
  });
});
