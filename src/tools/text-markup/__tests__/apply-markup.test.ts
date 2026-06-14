import { describe, expect, it } from "vitest";

import type { PageGeometry } from "@/tools/_framework";

import { buildMarkupDrafts, type PageGroup } from "../apply-markup";

const geo: PageGeometry = { page: 2, width: 612, height: 792, scale: 1, rotation: 0 };

describe("buildMarkupDrafts", () => {
  it("produces one markup draft per page group with the request's style", () => {
    const groups: PageGroup[] = [
      { geo, rects: [{ left: 10, top: 20, right: 110, bottom: 32 }] },
    ];
    const drafts = buildMarkupDrafts(groups, {
      documentId: "d",
      subtype: "highlight",
      color: "#ffd400",
      opacity: 0.8,
    });

    expect(drafts).toHaveLength(1);
    const d = drafts[0];
    expect(d?.type).toBe("markup");
    expect(d?.subtype).toBe("highlight");
    expect(d?.page).toBe(2);
    expect(d?.color).toBe("#ffd400");
    expect(d?.opacity).toBe(0.8);
    expect(d?.quads).toHaveLength(1);
  });

  it("skips a group whose rects are all degenerate", () => {
    const groups: PageGroup[] = [{ geo, rects: [{ left: 5, top: 5, right: 5, bottom: 5 }] }];
    expect(buildMarkupDrafts(groups, {
      documentId: "d",
      subtype: "underline",
      color: "#000",
      opacity: 1,
    })).toHaveLength(0);
  });

  it("emits a draft per page for a multi-page selection", () => {
    const groups: PageGroup[] = [
      { geo, rects: [{ left: 10, top: 20, right: 110, bottom: 32 }] },
      { geo: { ...geo, page: 3 }, rects: [{ left: 10, top: 40, right: 90, bottom: 52 }] },
    ];
    const drafts = buildMarkupDrafts(groups, {
      documentId: "d",
      subtype: "strikethrough",
      color: "#f00",
      opacity: 1,
    });
    expect(drafts.map((d) => d.page)).toEqual([2, 3]);
  });
});
