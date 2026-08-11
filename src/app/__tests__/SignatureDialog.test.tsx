// SPEC: P6-SEC-001 (P6.A2) — draw a signature and save it to the library.
//
// `strokesToPng` is mocked: it is the one part of this feature that cannot run
// here, because jsdom has no canvas (`getContext` returns null). What these
// tests do cover is everything around it — when Save is available, what it
// sends, and that a failure does not cost you the drawing.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";

vi.mock("@/tools/signature/raster", () => ({
  strokesToPng: vi.fn(),
  textToPng: vi.fn(),
  imageToPng: vi.fn(),
}));
// The file picker and the file read are the two things the dialog cannot do
// under test; everything between them is real.
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));
vi.mock("@tauri-apps/plugin-fs", () => ({ readFile: vi.fn() }));
// Font detection needs a canvas to measure with; jsdom has none, so the set of
// available families is supplied directly.
vi.mock("@/tools/signature/fonts", async (orig) => ({
  ...(await orig<typeof import("@/tools/signature/fonts")>()),
  canvasMeasurer: () => () => 0,
  availableFonts: vi.fn(() => [
    { family: "Snell Roundhand", label: "Snell Roundhand" },
    { family: "Segoe Script", label: "Segoe Script" },
  ]),
}));
vi.mock("@/app/report-error", () => ({ reportError: vi.fn() }));

import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
import { readFile } from "@tauri-apps/plugin-fs";

import { reportError } from "@/app/report-error";
import { SignatureDialog } from "@/app/SignatureDialog";
import { useSignatureStore } from "@/state/signature-store";
import { useStampStore } from "@/state/stamp-store";
import { useToolStore } from "@/state/tool-store";
import { imageToPng, strokesToPng, textToPng } from "@/tools/signature/raster";

const mockPng = vi.mocked(strokesToPng);
const mockText = vi.mocked(textToPng);
const mockImage = vi.mocked(imageToPng);
const mockOpen = vi.mocked(openFileDialog);
const mockRead = vi.mocked(readFile);
const mockReport = vi.mocked(reportError);

const refresh = vi.fn().mockResolvedValue(undefined);
const add = vi.fn().mockResolvedValue({ id: "s1", kind: "draw", createdAt: 1 });
// P6.A5a — the list fetches a thumbnail per entry; that is IPC, so it is stubbed.
const loadThumb = vi.fn().mockResolvedValue(undefined);

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
  mockText.mockResolvedValue(Uint8Array.from([0x89, 0x50, 0x4e, 0x47]));
  // Default: an opaque import, which is what a JPEG photo looks like.
  mockImage.mockResolvedValue({
    png: Uint8Array.from([1, 2]),
    erased: 0,
    total: 100,
    transparent: 0,
  });
  mockOpen.mockResolvedValue("/tmp/sig.png" as never);
  mockRead.mockResolvedValue(Uint8Array.from([0x89, 0x50]));
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
      thumbs: {},
      loadThumb,
    } as never);
    render(dialog());

    expect(screen.getByText(/Saved signatures \(2\)/)).toBeTruthy();
    // Scoped to the list: "type" is also the label of the mode-switch button.
    const list = screen.getByLabelText("Saved signatures");
    expect(within(list).getByText("type")).toBeTruthy();
    expect(within(list).getByText("draw")).toBeTruthy();
  });

  it("reloads the library when opened", async () => {
    render(dialog());
    await waitFor(() => expect(refresh).toHaveBeenCalled());
  });

  // SPEC: P6-SEC-002 (P6.A3) — typed signatures.
  it("switches to Type and offers the detected fonts", async () => {
    render(dialog());
    fireEvent.click(screen.getByRole("tab", { name: "type" }));

    expect(screen.getByLabelText("Signature text")).toBeTruthy();
    const picker = (await screen.findByLabelText("Handwriting font")) as HTMLSelectElement;
    // "several handwriting-style fonts", and the user picks — the spec's words.
    expect(picker.options.length).toBeGreaterThan(1);
  });

  it("keeps Save disabled until something is typed", async () => {
    render(dialog());
    fireEvent.click(screen.getByRole("tab", { name: "type" }));
    expect(screen.getByText("Save to library").hasAttribute("disabled")).toBe(true);

    fireEvent.change(screen.getByLabelText("Signature text"), { target: { value: "Ada" } });
    expect(screen.getByText("Save to library").hasAttribute("disabled")).toBe(false);
  });

  it("will not save whitespace as a signature", () => {
    render(dialog());
    fireEvent.click(screen.getByRole("tab", { name: "type" }));
    fireEvent.change(screen.getByLabelText("Signature text"), { target: { value: "   " } });
    expect(screen.getByText("Save to library").hasAttribute("disabled")).toBe(true);
  });

  it("saves typed text with the chosen font, as a type signature", async () => {
    render(dialog());
    fireEvent.click(screen.getByRole("tab", { name: "type" }));
    fireEvent.change(screen.getByLabelText("Signature text"), { target: { value: "Ada" } });
    fireEvent.change(await screen.findByLabelText("Handwriting font"), {
      target: { value: "Segoe Script" },
    });
    fireEvent.click(screen.getByText("Save to library"));

    await waitFor(() => expect(add).toHaveBeenCalledTimes(1));
    expect(add.mock.calls[0]![0]).toBe("type");
    const [text, opts] = mockText.mock.calls[0]!;
    expect(text).toBe("Ada");
    expect(opts).toMatchObject({ family: "Segoe Script" });
  });

  it("switching modes does not discard the other draft", () => {
    render(dialog());
    draw(screen.getByLabelText("Signature pad"));

    fireEvent.click(screen.getByRole("tab", { name: "type" }));
    fireEvent.change(screen.getByLabelText("Signature text"), { target: { value: "Ada" } });
    fireEvent.click(screen.getByRole("tab", { name: "draw" }));

    // The drawing survived the round trip — only Clear discards.
    expect(screen.getByText("Save to library").hasAttribute("disabled")).toBe(false);
    fireEvent.click(screen.getByRole("tab", { name: "type" }));
    expect((screen.getByLabelText("Signature text") as HTMLInputElement).value).toBe("Ada");
  });

  it("keeps the text when a typed save fails", async () => {
    mockText.mockRejectedValue(new Error("no glyphs"));
    render(dialog());
    fireEvent.click(screen.getByRole("tab", { name: "type" }));
    fireEvent.change(screen.getByLabelText("Signature text"), { target: { value: "Ada" } });
    fireEvent.click(screen.getByText("Save to library"));

    await waitFor(() => expect(mockReport).toHaveBeenCalled());
    expect((screen.getByLabelText("Signature text") as HTMLInputElement).value).toBe("Ada");
  });

  // SPEC: P6-SEC-003 (P6.A4) — image signatures.

  /** Switch to Image mode and pick a file, waiting for the threshold to run. */
  const pickImage = async () => {
    fireEvent.click(screen.getByRole("tab", { name: "image" }));
    fireEvent.click(screen.getByText("Choose image…"));
    await waitFor(() => expect(mockImage).toHaveBeenCalled());
  };

  it("offers the three formats the spec names", async () => {
    render(dialog());
    await pickImage();
    expect(mockOpen.mock.calls[0]![0]).toMatchObject({
      multiple: false,
      filters: [{ name: "Image", extensions: ["png", "jpg", "jpeg", "bmp"] }],
    });
  });

  it("keeps Save and the slider inert until an image is chosen", () => {
    render(dialog());
    fireEvent.click(screen.getByRole("tab", { name: "image" }));

    expect(screen.getByText("Save to library").hasAttribute("disabled")).toBe(true);
    expect((screen.getByLabelText("Background removal") as HTMLInputElement).disabled).toBe(true);
    expect(screen.getByText("No image chosen")).toBeTruthy();
  });

  it("runs the chosen file through the threshold and previews the result", async () => {
    render(dialog());
    await pickImage();

    expect(mockRead).toHaveBeenCalledWith("/tmp/sig.png");
    expect(mockImage.mock.calls[0]![0]).toBeInstanceOf(Uint8Array);
    await waitFor(() => expect(screen.queryByText("No image chosen")).toBeNull());
    // The file name stays visible, so it is clear what is being worked on.
    expect(screen.getByText("sig.png")).toBeTruthy();
  });

  it("saves the imported image as an image signature", async () => {
    render(dialog());
    await pickImage();
    await waitFor(() =>
      expect(screen.getByText("Save to library").hasAttribute("disabled")).toBe(false),
    );
    fireEvent.click(screen.getByText("Save to library"));

    await waitFor(() => expect(add).toHaveBeenCalledTimes(1));
    const [kind, png] = add.mock.calls[0]!;
    expect(kind).toBe("image");
    // Byte-for-byte what the preview showed — the preview is the artifact,
    // not an impression of it.
    expect(Array.from(png as Uint8Array)).toEqual([1, 2]);
  });

  it("re-runs the threshold when the slider moves", async () => {
    render(dialog());
    await pickImage();

    fireEvent.change(screen.getByLabelText("Background removal"), { target: { value: "60" } });
    await waitFor(() => expect(mockImage).toHaveBeenCalledTimes(2));
    expect(mockImage.mock.calls[1]![1]).toMatchObject({ strength: 60 });
  });

  it("warns when the import has no transparency to place", async () => {
    render(dialog());
    await pickImage();
    // A JPEG placed as-is is an opaque rectangle sitting on the page.
    await waitFor(() => expect(screen.getByText(/solid rectangle/)).toBeTruthy());
  });

  it("says nothing about solid rectangles once the background is gone", async () => {
    mockImage.mockResolvedValue({
      png: Uint8Array.from([1, 2]),
      erased: 40,
      total: 100,
      transparent: 40,
    });
    render(dialog());
    await pickImage();

    await waitFor(() => expect(screen.queryByText("No image chosen")).toBeNull());
    expect(screen.queryByText(/solid rectangle/)).toBeNull();
  });

  it("keeps the chosen file when the threshold leaves nothing", async () => {
    mockImage.mockRejectedValue(new Error("the background removal erased the whole image"));
    render(dialog());
    await pickImage();

    await waitFor(() => expect(screen.getByText(/erased the whole image/)).toBeTruthy());
    // Re-picking the same file to undo a slider drag would be absurd.
    expect(screen.getByText("sig.png")).toBeTruthy();
    expect(screen.getByText("Save to library").hasAttribute("disabled")).toBe(true);
  });

  it("does nothing when the picker is dismissed", async () => {
    mockOpen.mockResolvedValue(null as never);
    render(dialog());
    fireEvent.click(screen.getByRole("tab", { name: "image" }));
    fireEvent.click(screen.getByText("Choose image…"));

    await waitFor(() => expect(mockOpen).toHaveBeenCalled());
    expect(mockRead).not.toHaveBeenCalled();
    expect(mockImage).not.toHaveBeenCalled();
  });

  // SPEC: P6-SEC-004 (P6.A5a) — choosing a saved signature to place.

  const twoEntries = () =>
    useSignatureStore.setState({
      entries: [
        { id: "a", kind: "draw", createdAt: 1_700_000_000_000 },
        { id: "b", kind: "image", createdAt: 1_700_000_001_000 },
      ],
      loading: false,
      refresh,
      add,
      thumbs: { a: "data:image/png;base64,AAA" },
      loadThumb,
    } as never);

  it("fetches a thumbnail for every saved signature", async () => {
    twoEntries();
    render(dialog());
    // A list of kinds and dates is not something anyone can pick from.
    await waitFor(() => expect(loadThumb).toHaveBeenCalledWith("a"));
    expect(loadThumb).toHaveBeenCalledWith("b");
  });

  it("shows the thumbnail it has and leaves a placeholder for the rest", () => {
    twoEntries();
    render(dialog());
    const list = screen.getByLabelText("Saved signatures");
    const imgs = within(list).getAllByRole("img");
    expect(imgs).toHaveLength(1);
    expect(imgs[0]!.getAttribute("src")).toBe("data:image/png;base64,AAA");
  });

  it("Place arms the signature in its own mode, and closes", () => {
    twoEntries();
    const onClose = vi.fn();
    render(<SignatureDialog open onClose={onClose} />);
    fireEvent.click(screen.getByLabelText("Place image signature"));

    expect(useStampStore.getState().armed).toMatchObject({
      kind: "signature",
      signatureId: "b",
    });
    // Not "stamp": that opened the rubber-stamp tool and its APPROVED/DRAFT
    // palette, which is not what the user picked.
    expect(useToolStore.getState().activeTool).toBe("signature");
    // Placement is a click on the page, so the modal has to get out of the way.
    expect(onClose).toHaveBeenCalled();
  });

  it("Clear discards the import but not the other drafts", async () => {
    render(dialog());
    draw(screen.getByLabelText("Signature pad"));
    await pickImage();

    fireEvent.click(screen.getByText("Clear"));
    expect(screen.getByText("No image chosen")).toBeTruthy();

    fireEvent.click(screen.getByRole("tab", { name: "draw" }));
    expect(screen.getByText("Save to library").hasAttribute("disabled")).toBe(false);
  });
});
