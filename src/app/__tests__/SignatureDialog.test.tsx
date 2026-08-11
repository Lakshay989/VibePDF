// SPEC: P6-SEC-001 (P6.A2) — draw a signature and save it to the library.
//
// `strokesToPng` is mocked: it is the one part of this feature that cannot run
// here, because jsdom has no canvas (`getContext` returns null). What these
// tests do cover is everything around it — when Save is available, what it
// sends, and that a failure does not cost you the drawing.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";

vi.mock("@/tools/signature/raster", () => ({
  strokesToPng: vi.fn(),
}));
vi.mock("@/app/report-error", () => ({ reportError: vi.fn() }));

import { reportError } from "@/app/report-error";
import { SignatureDialog } from "@/app/SignatureDialog";
import { useSignatureStore } from "@/state/signature-store";
import { strokesToPng } from "@/tools/signature/raster";

const mockPng = vi.mocked(strokesToPng);
const mockReport = vi.mocked(reportError);

const refresh = vi.fn().mockResolvedValue(undefined);
const add = vi.fn().mockResolvedValue({ id: "s1", kind: "draw", createdAt: 1 });

const dialog = () => <SignatureDialog open onClose={() => {}} />;

/** Draw a two-point stroke on the pad. */
const draw = (pad: HTMLElement) => {
  fireEvent.pointerDown(pad, { clientX: 10, clientY: 10, pointerId: 1 });
  fireEvent.pointerMove(pad, { clientX: 40, clientY: 30, pointerId: 1 });
  fireEvent.pointerUp(pad, { pointerId: 1 });
};

beforeEach(() => {
  useSignatureStore.setState({ entries: [], loading: false, refresh, add } as never);
  mockPng.mockResolvedValue(Uint8Array.from([0x89, 0x50, 0x4e, 0x47]));
  vi.clearAllMocks();
});
afterEach(cleanup);

describe("SignatureDialog", () => {
  it("renders nothing when closed", () => {
    const { container } = render(<SignatureDialog open={false} onClose={() => {}} />);
    expect(container.firstChild).toBeNull();
  });

  it("keeps Save disabled until something is drawn", () => {
    render(dialog());
    const save = screen.getByText("Save to library");
    expect(save.hasAttribute("disabled")).toBe(true);

    draw(screen.getByLabelText("Signature pad"));
    expect(screen.getByText("Save to library").hasAttribute("disabled")).toBe(false);
  });

  it("saves the drawing to the library as a draw signature", async () => {
    render(dialog());
    draw(screen.getByLabelText("Signature pad"));
    fireEvent.click(screen.getByText("Save to library"));

    await waitFor(() => expect(add).toHaveBeenCalledTimes(1));
    const [kind, png] = add.mock.calls[0]!;
    expect(kind).toBe("draw");
    expect(png).toBeInstanceOf(Uint8Array);

    // The captured strokes reached the rasteriser, not an empty list.
    const [strokes] = mockPng.mock.calls[0]!;
    expect(strokes).toHaveLength(1);
    expect(strokes[0]!.length).toBeGreaterThanOrEqual(2);
  });

  it("clears the pad after a successful save", async () => {
    render(dialog());
    draw(screen.getByLabelText("Signature pad"));
    fireEvent.click(screen.getByText("Save to library"));

    // An empty pad is the signal the save landed.
    await waitFor(() =>
      expect(screen.getByText("Save to library").hasAttribute("disabled")).toBe(true),
    );
  });

  it("Clear empties the pad and re-disables Save", () => {
    render(dialog());
    draw(screen.getByLabelText("Signature pad"));
    fireEvent.click(screen.getByText("Clear"));

    expect(screen.getByText("Save to library").hasAttribute("disabled")).toBe(true);
    expect(screen.getByText("Clear").hasAttribute("disabled")).toBe(true);
  });

  it("keeps the drawing when a save fails", async () => {
    mockPng.mockRejectedValue(new Error("no canvas"));
    render(dialog());
    draw(screen.getByLabelText("Signature pad"));
    fireEvent.click(screen.getByText("Save to library"));

    await waitFor(() => expect(mockReport).toHaveBeenCalled());
    expect(add).not.toHaveBeenCalled();
    // Losing someone's signature because the encoder failed would be the worst
    // possible response to a transient error.
    expect(screen.getByText("Save to library").hasAttribute("disabled")).toBe(false);
  });

  it("lists what is already in the library", async () => {
    useSignatureStore.setState({
      entries: [
        { id: "a", kind: "draw", createdAt: 1_700_000_000_000 },
        { id: "b", kind: "type", createdAt: 1_700_000_001_000 },
      ],
      loading: false,
      refresh,
      add,
    } as never);
    render(dialog());

    expect(screen.getByText(/Saved signatures \(2\)/)).toBeTruthy();
    expect(screen.getByText("type")).toBeTruthy();
  });

  it("reloads the library when opened", async () => {
    render(dialog());
    await waitFor(() => expect(refresh).toHaveBeenCalled());
  });
});
