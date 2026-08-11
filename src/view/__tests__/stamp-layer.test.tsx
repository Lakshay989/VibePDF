// SPEC: P3-ANN-006 (P3.C3a) — the stamp overlay: with the stamp tool active and
// a stamp armed, a click drops it (centred, default size) via addStamp. Inactive
// tool or no armed stamp = no-op. IPC is mocked.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render } from "@testing-library/react";

vi.mock("@/ipc/stamps", () => ({
  addStamp: vi.fn().mockResolvedValue({ canUndo: true, canRedo: false }),
  addImageStamp: vi.fn().mockResolvedValue({ canUndo: true, canRedo: false }),
}));

// P6.A5a — placing a saved signature, and reading the page's fields to find out
// whether the click landed on a signature widget.
vi.mock("@/ipc/signatures", () => ({
  placeSignature: vi.fn().mockResolvedValue({ canUndo: true, canRedo: false }),
  signatureBytes: vi.fn().mockResolvedValue(Uint8Array.from([1, 2, 3])),
}));
vi.mock("@/ipc/forms", () => ({ readPageFields: vi.fn().mockResolvedValue([]) }));
vi.mock("@/app/report-error", () => ({ reportError: vi.fn() }));

// Image stamps read the picked file for an optimistic <img> preview (P4.HF29);
// stub it so the async path is deterministic.
vi.mock("@/view/file-data-url", () => ({
  fileToDataUrl: vi.fn().mockResolvedValue("data:image/png;base64,AAAA"),
  bytesToDataUrl: vi.fn(() => "data:image/png;base64,AAAA"),
  imageAspect: vi.fn().mockResolvedValue(1),
}));

import { reportError } from "@/app/report-error";
import { readPageFields } from "@/ipc/forms";
import { placeSignature } from "@/ipc/signatures";
import { addImageStamp, addStamp } from "@/ipc/stamps";
import { useEditEpochStore } from "@/state/edit-epoch-store";
import { useStampStore } from "@/state/stamp-store";
import { useToolStore } from "@/state/tool-store";
import { SIGNATURE_HEIGHT } from "@/tools/signature/place";
import { StampLayer } from "@/view/stamp-layer";

const DOC = "doc-1";
const mockAddStamp = vi.mocked(addStamp);
const mockAddImageStamp = vi.mocked(addImageStamp);
const mockPlace = vi.mocked(placeSignature);
const mockFields = vi.mocked(readPageFields);
const mockReport = vi.mocked(reportError);

// Letter (612×792), 1× scale → screen (x,y) maps to PDF (x, 792−y).
const layer = () => (
  <StampLayer documentId={DOC} page={0} displayedWidth={612} displayedHeight={792} scale={1} rotation={0} />
);

const APPROVED = { kind: "text", name: "Approved", label: "APPROVED", color: "#1e8449" } as const;

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});
beforeEach(() => {
  useToolStore.setState({
    activeTool: "stamp",
    options: {
      color: "#000000",
      opacity: 1,
      strokeWidth: 2,
      fillColor: null,
      fontFamily: "Helvetica",
      fontSize: 14,
      bold: false,
      italic: false,
      underline: false,
    },
  });
  useStampStore.setState({ armed: APPROVED });
  useEditEpochStore.setState({ byDoc: {}, edited: {} });
  mockFields.mockResolvedValue([]);
});

/** Click the overlay at a screen point. Letter at 1×, so PDF y = 792 − y. */
const clickAt = (container: HTMLElement, clientX: number, clientY: number) =>
  fireEvent.pointerDown(container.querySelector("div") as Element, {
    clientX,
    clientY,
    pointerId: 1,
    button: 0,
  });

describe("StampLayer", () => {
  it("drops the armed stamp on click, centred at the point", () => {
    const { container } = render(layer());
    const div = container.querySelector("div") as Element;
    fireEvent.pointerDown(div, { clientX: 300, clientY: 400, pointerId: 1, button: 0 });

    // screen (300,400) → PDF (300, 392); default 150×46 box centred + clamped.
    expect(mockAddStamp).toHaveBeenCalledWith(
      DOC,
      0,
      [225, 369, 375, 415],
      "APPROVED",
      "Approved",
      "#1e8449",
      1,
    );
  });

  // SPEC: P3-ANN-006 (P3.C3b) — an armed image stamp routes to addImageStamp with
  // the click point (the backend derives the aspect-correct rect).
  it("drops an armed image stamp at the click point", async () => {
    useStampStore.setState({
      armed: { kind: "image", name: "sig.png", imagePath: "/tmp/sig.png" },
    });
    const { container } = render(layer());
    fireEvent.pointerDown(container.querySelector("div") as Element, {
      clientX: 300,
      clientY: 400,
      pointerId: 1,
      button: 0,
    });
    // The embed is awaited behind the preview read; screen (300,400) → PDF
    // (300, 392); height 64, no label.
    await vi.waitFor(() =>
      expect(mockAddImageStamp).toHaveBeenCalledWith(DOC, 0, 300, 392, 64, "/tmp/sig.png", null, 1),
    );
    expect(mockAddStamp).not.toHaveBeenCalled();
  });

  it("does nothing when no stamp is armed", () => {
    useStampStore.setState({ armed: null });
    const { container } = render(layer());
    fireEvent.pointerDown(container.querySelector("div") as Element, {
      clientX: 300,
      clientY: 400,
      pointerId: 1,
      button: 0,
    });
    expect(mockAddStamp).not.toHaveBeenCalled();
  });

  it("does nothing when the stamp tool is not active", () => {
    useToolStore.setState({ activeTool: null });
    const { container } = render(layer());
    fireEvent.pointerDown(container.querySelector("div") as Element, {
      clientX: 300,
      clientY: 400,
      pointerId: 1,
      button: 0,
    });
    expect(mockAddStamp).not.toHaveBeenCalled();
  });

  // SPEC: P6-SEC-004 (P6.A5a) — placing a saved signature.

  const SIG = { kind: "signature", name: "s1", signatureId: "s1" } as const;
  const sigField = {
    name: "Signature1",
    kind: "signature" as const,
    rect: [100, 500, 300, 560] as [number, number, number, number],
  };

  it("places an armed signature at the click point, by id", async () => {
    useStampStore.setState({ armed: SIG });
    const { container } = render(layer());
    clickAt(container, 300, 400);

    // screen (300,400) → PDF (300, 392). The id crosses, never the bytes.
    await vi.waitFor(() =>
      expect(mockPlace).toHaveBeenCalledWith(DOC, 0, 300, 392, SIGNATURE_HEIGHT, "s1", 1),
    );
    expect(mockAddStamp).not.toHaveBeenCalled();
    expect(mockAddImageStamp).not.toHaveBeenCalled();
  });

  it("refuses to stamp over a signature field, and writes nothing", async () => {
    // The whole point of the guard: a picture on a /Sig widget reads as a
    // signature to every viewer and is not one. Until P6.B1 can sign, decline.
    mockFields.mockResolvedValue([sigField]);
    useStampStore.setState({ armed: SIG });
    const { container } = render(layer());
    // PDF (200, 530) is inside the field → screen y = 792 − 530 = 262.
    clickAt(container, 200, 262);

    await vi.waitFor(() => expect(mockReport).toHaveBeenCalled());
    expect(mockPlace).not.toHaveBeenCalled();
    // The message has to name the field and explain, not just say "no".
    const [, err] = mockReport.mock.calls[0]!;
    expect(String((err as Error).message)).toContain("Signature1");
  });

  it("places normally when the click misses the signature field", async () => {
    mockFields.mockResolvedValue([sigField]);
    useStampStore.setState({ armed: SIG });
    const { container } = render(layer());
    // PDF (400, 392) — same page, well clear of the field.
    clickAt(container, 400, 400);

    await vi.waitFor(() => expect(mockPlace).toHaveBeenCalled());
    expect(mockReport).not.toHaveBeenCalled();
  });

  it("still places when the page's fields cannot be read", async () => {
    // A PDF with no AcroForm at all is the common case; failing to read fields
    // must not block placement, because there is no field to collide with.
    mockFields.mockRejectedValue(new Error("no form"));
    useStampStore.setState({ armed: SIG });
    const { container } = render(layer());
    clickAt(container, 300, 400);

    await vi.waitFor(() => expect(mockPlace).toHaveBeenCalled());
  });
});
