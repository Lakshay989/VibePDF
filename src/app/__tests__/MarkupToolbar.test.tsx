// SPEC: P6-SEC-004 (P6.A5a) — the toolbar's disarm-on-leave effect.
//
// This file exists because of one bug, and it is worth stating plainly. The
// toolbar has always disarmed the stamp when you leave the stamp tool. P6.A5a
// added a second tool that drives the same layer — `"signature"` — and updated
// the layer to accept it without updating the toolbar. So the toolbar saw
// "not the stamp tool" and cleared the signature the dialog had just armed.
//
// The symptom was maddening rather than obvious: the first Place after any tool
// change silently did nothing, and a second Place worked — because by then the
// tool was already `"signature"`, so the effect's dependencies did not change
// and it never re-ran. Two places encoded the same fact and only one was
// changed; `usesStampLayer` now holds it once, and these tests pin the pairing.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, render } from "@testing-library/react";

vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));
vi.mock("@/ipc/annotations", () => ({
  addTextMarkup: vi.fn(),
  clearTextMarkup: vi.fn(),
}));
vi.mock("@/app/SignatureDialog", () => ({ SignatureDialog: () => null }));

import { MarkupToolbar } from "@/app/MarkupToolbar";
import { useStampStore } from "@/state/stamp-store";
import { useToolStore } from "@/state/tool-store";
import { signatureStamp, usesStampLayer } from "@/tools/stamp/stamps";

const APPROVED = { kind: "text", name: "Approved", label: "APPROVED", color: "#1e8449" } as const;

beforeEach(() => {
  useToolStore.setState({ activeTool: null });
  useStampStore.setState({ armed: null });
});
afterEach(cleanup);

describe("usesStampLayer", () => {
  it("covers both tools that place through the stamp layer", () => {
    expect(usesStampLayer("stamp")).toBe(true);
    expect(usesStampLayer("signature")).toBe(true);
  });

  it("is false for everything else, including no tool", () => {
    expect(usesStampLayer(null)).toBe(false);
    expect(usesStampLayer("ink")).toBe(false);
    expect(usesStampLayer("measure")).toBe(false);
  });
});

describe("MarkupToolbar disarming", () => {
  it("keeps a signature armed when the signature tool is entered", () => {
    // The regression. `place()` in the dialog arms and then sets the tool; if
    // the toolbar treats that as leaving the stamp tool, the signature is gone
    // before the user's first click.
    render(<MarkupToolbar documentId="doc-1" />);
    act(() => {
      useStampStore.setState({ armed: signatureStamp("sig-1") });
    });
    act(() => {
      useToolStore.setState({ activeTool: "signature" });
    });

    expect(useStampStore.getState().armed).toMatchObject({ signatureId: "sig-1" });
  });

  it("keeps a rubber stamp armed while the stamp tool is active", () => {
    render(<MarkupToolbar documentId="doc-1" />);
    act(() => {
      useStampStore.setState({ armed: APPROVED });
    });
    act(() => {
      useToolStore.setState({ activeTool: "stamp" });
    });

    expect(useStampStore.getState().armed).toEqual(APPROVED);
  });

  it("disarms when a tool that does not place stamps takes over", () => {
    render(<MarkupToolbar documentId="doc-1" />);
    act(() => {
      useToolStore.setState({ activeTool: "signature" });
    });
    act(() => {
      useStampStore.setState({ armed: signatureStamp("sig-1") });
    });

    act(() => {
      useToolStore.setState({ activeTool: "ink" });
    });
    expect(useStampStore.getState().armed).toBeNull();
  });

  it("disarms when every tool is switched off", () => {
    render(<MarkupToolbar documentId="doc-1" />);
    act(() => {
      useToolStore.setState({ activeTool: "stamp" });
    });
    act(() => {
      useStampStore.setState({ armed: APPROVED });
    });

    act(() => {
      useToolStore.setState({ activeTool: null });
    });
    expect(useStampStore.getState().armed).toBeNull();
  });
});
