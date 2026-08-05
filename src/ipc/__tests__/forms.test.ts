// SPEC: P5-FORM-001 (P5.A1) — the readFormSummary IPC wrapper marshals the
// document id to the Rust command and returns the typed summary.

import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/ipc/invoke", () => ({ invoke: vi.fn() }));

import { invoke } from "@/ipc/invoke";
import {
  fillTextField,
  readButtonFields,
  readChoiceFields,
  readFormSummary,
  readTextFields,
  setButtonField,
  setChoiceField,
  type ButtonField,
  type ChoiceField,
  type FormField,
  type FormSummary,
} from "@/ipc/forms";

const mockInvoke = vi.mocked(invoke);

beforeEach(() => {
  mockInvoke.mockReset();
});

describe("readFormSummary", () => {
  it("marshals the document id and returns the summary", async () => {
    const summary: FormSummary = { fieldCount: 3, hasXfa: false };
    mockInvoke.mockResolvedValue(summary);

    const out = await readFormSummary("doc-1");
    expect(mockInvoke).toHaveBeenCalledWith("pdf_read_form_summary", { id: "doc-1" });
    expect(out).toEqual(summary);
  });

  it("carries the XFA flag through", async () => {
    mockInvoke.mockResolvedValue({ fieldCount: 0, hasXfa: true } satisfies FormSummary);
    const out = await readFormSummary("doc-2");
    expect(out.hasXfa).toBe(true);
    expect(out.fieldCount).toBe(0);
  });
});

describe("readTextFields", () => {
  it("marshals id + page and returns the fields", async () => {
    const fields: FormField[] = [
      { name: "name", rect: [72, 700, 300, 724], value: "", maxLen: null, multiline: false },
    ];
    mockInvoke.mockResolvedValue(fields);
    const out = await readTextFields("doc-1", 0);
    expect(mockInvoke).toHaveBeenCalledWith("pdf_read_text_fields", { id: "doc-1", page: 0 });
    expect(out).toEqual(fields);
  });
});

describe("fillTextField", () => {
  it("marshals id, name, and value", async () => {
    mockInvoke.mockResolvedValue({ canUndo: true, canRedo: false });
    const out = await fillTextField("doc-1", "name", "Ada");
    expect(mockInvoke).toHaveBeenCalledWith("pdf_fill_text_field", {
      id: "doc-1",
      name: "name",
      value: "Ada",
    });
    expect(out).toEqual({ canUndo: true, canRedo: false });
  });
});

describe("readButtonFields", () => {
  it("marshals id + page and returns the buttons", async () => {
    const buttons: ButtonField[] = [
      { fieldName: "agree", kind: "checkbox", rect: [72, 700, 90, 718], onState: "Yes", checked: false },
    ];
    mockInvoke.mockResolvedValue(buttons);
    const out = await readButtonFields("doc-1", 0);
    expect(mockInvoke).toHaveBeenCalledWith("pdf_read_button_fields", { id: "doc-1", page: 0 });
    expect(out).toEqual(buttons);
  });
});

describe("setButtonField", () => {
  it("marshals id, name, onState, and checked", async () => {
    mockInvoke.mockResolvedValue({ canUndo: true, canRedo: false });
    await setButtonField("doc-1", "color", "Green", true);
    expect(mockInvoke).toHaveBeenCalledWith("pdf_set_button_field", {
      id: "doc-1",
      name: "color",
      onState: "Green",
      checked: true,
    });
  });
});

describe("readChoiceFields", () => {
  it("marshals id + page and returns the choices", async () => {
    const fields: ChoiceField[] = [
      {
        name: "fruit",
        kind: "combo",
        rect: [72, 700, 250, 724],
        options: [{ export: "chy", label: "Cherry" }],
        selected: ["chy"],
        multi: false,
      },
    ];
    mockInvoke.mockResolvedValue(fields);
    const out = await readChoiceFields("doc-1", 0);
    expect(mockInvoke).toHaveBeenCalledWith("pdf_read_choice_fields", { id: "doc-1", page: 0 });
    expect(out).toEqual(fields);
  });
});

describe("setChoiceField", () => {
  it("marshals id, name, and values", async () => {
    mockInvoke.mockResolvedValue({ canUndo: true, canRedo: false });
    await setChoiceField("doc-1", "colors", ["Red", "Blue"]);
    expect(mockInvoke).toHaveBeenCalledWith("pdf_set_choice_field", {
      id: "doc-1",
      name: "colors",
      values: ["Red", "Blue"],
    });
  });
});
