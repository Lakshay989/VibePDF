// SPEC: P5-FORM-001 (P5.A1) — the readFormSummary IPC wrapper marshals the
// document id to the Rust command and returns the typed summary.

import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/ipc/invoke", () => ({ invoke: vi.fn() }));

import { invoke } from "@/ipc/invoke";
import {
  addField,
  addTextField,
  deleteField,
  fillTextField,
  readPageFields,
  setTabOrder,
  updateFieldProperties,
  readButtonFields,
  readChoiceFields,
  readFormSummary,
  readTextFields,
  setButtonField,
  setChoiceField,
  stripXfa,
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

describe("stripXfa", () => {
  it("marshals the document id", async () => {
    mockInvoke.mockResolvedValue({ canUndo: true, canRedo: false });
    await stripXfa("doc-1");
    expect(mockInvoke).toHaveBeenCalledWith("pdf_strip_xfa", { id: "doc-1" });
  });
});

describe("addTextField", () => {
  it("marshals the field config", async () => {
    mockInvoke.mockResolvedValue({ canUndo: true, canRedo: false });
    await addTextField("doc-1", 0, [72, 700, 300, 724], {
      name: "email",
      defaultValue: "",
      maxLen: 64,
      multiline: false,
      required: true,
    });
    expect(mockInvoke).toHaveBeenCalledWith("pdf_add_text_field", {
      id: "doc-1",
      page: 0,
      rect: [72, 700, 300, 724],
      name: "email",
      defaultValue: "",
      maxLen: 64,
      multiline: false,
      required: true,
    });
  });
});

describe("addField", () => {
  it("flattens a radio spec to the wire", async () => {
    mockInvoke.mockResolvedValue({ canUndo: true, canRedo: false });
    await addField("doc-1", 0, [72, 620, 260, 700], "color", {
      kind: "radio",
      options: ["Red", "Green"],
    });
    expect(mockInvoke).toHaveBeenCalledWith("pdf_add_field", {
      id: "doc-1",
      page: 0,
      rect: [72, 620, 260, 700],
      name: "color",
      kind: "radio",
      options: ["Red", "Green"],
      defaultValue: "",
      multi: false,
      required: false,
      caption: "",
    });
  });

  it("flattens a pushbutton spec", async () => {
    mockInvoke.mockResolvedValue({ canUndo: true, canRedo: false });
    await addField("doc-1", 0, [0, 0, 10, 10], "go", { kind: "pushbutton", caption: "Go" });
    expect(mockInvoke).toHaveBeenCalledWith("pdf_add_field", {
      id: "doc-1",
      page: 0,
      rect: [0, 0, 10, 10],
      name: "go",
      kind: "pushbutton",
      options: [],
      defaultValue: "",
      multi: false,
      required: false,
      caption: "Go",
    });
  });
});

describe("readPageFields / setTabOrder / deleteField", () => {
  it("marshals readPageFields", async () => {
    mockInvoke.mockResolvedValue([]);
    await readPageFields("doc-1", 2);
    expect(mockInvoke).toHaveBeenCalledWith("pdf_read_page_fields", { id: "doc-1", page: 2 });
  });

  it("marshals setTabOrder", async () => {
    mockInvoke.mockResolvedValue({ canUndo: true, canRedo: false });
    await setTabOrder("doc-1", 0, ["b", "a"]);
    expect(mockInvoke).toHaveBeenCalledWith("pdf_set_tab_order", {
      id: "doc-1",
      page: 0,
      names: ["b", "a"],
    });
  });

  it("marshals deleteField", async () => {
    mockInvoke.mockResolvedValue({ canUndo: true, canRedo: false });
    await deleteField("doc-1", "a");
    expect(mockInvoke).toHaveBeenCalledWith("pdf_delete_field", { id: "doc-1", name: "a" });
  });
});

describe("updateFieldProperties", () => {
  it("sends a value for maxLen and leaves omitted keys null", async () => {
    mockInvoke.mockResolvedValue({ canUndo: true, canRedo: false });
    await updateFieldProperties("doc-1", "a", { newName: "first", maxLen: 5 });
    expect(mockInvoke).toHaveBeenCalledWith("pdf_update_field_properties", {
      id: "doc-1",
      name: "a",
      newName: "first",
      defaultValue: null,
      maxLen: 5,
      clearMaxLen: false,
      multiline: null,
      required: null,
      tooltip: null,
    });
  });

  it("clears maxLen when it is null", async () => {
    mockInvoke.mockResolvedValue({ canUndo: true, canRedo: false });
    await updateFieldProperties("doc-1", "a", { maxLen: null });
    const args = mockInvoke.mock.calls.at(-1)?.[1] as { maxLen: unknown; clearMaxLen: unknown };
    expect(args.maxLen).toBeNull();
    expect(args.clearMaxLen).toBe(true);
  });
});
