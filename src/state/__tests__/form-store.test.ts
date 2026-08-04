// SPEC: P5-FORM-001 (P5.A1) — the form-detection store: `detected` gates the
// "Form mode" entry point; `formMode` is the toggle the fill steps build on.

import { beforeEach, describe, expect, it } from "vitest";

import { useFormStore } from "@/state/form-store";

beforeEach(() => {
  useFormStore.setState({ detected: null, formMode: false });
});

describe("useFormStore", () => {
  it("starts with no detection and form mode off", () => {
    const s = useFormStore.getState();
    expect(s.detected).toBeNull();
    expect(s.formMode).toBe(false);
  });

  it("records a detected summary", () => {
    useFormStore.getState().setDetected({ fieldCount: 2, hasXfa: false });
    expect(useFormStore.getState().detected).toEqual({ fieldCount: 2, hasXfa: false });
  });

  it("enters and exits form mode", () => {
    useFormStore.getState().enterFormMode();
    expect(useFormStore.getState().formMode).toBe(true);
    useFormStore.getState().exitFormMode();
    expect(useFormStore.getState().formMode).toBe(false);
  });

  it("clears detection when set back to null (document switch)", () => {
    useFormStore.getState().setDetected({ fieldCount: 5, hasXfa: false });
    useFormStore.getState().setDetected(null);
    expect(useFormStore.getState().detected).toBeNull();
  });
});
