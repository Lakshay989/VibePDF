import { describe, expect, it, vi } from "vitest";

import {
  countOutlineEntries,
  normalizeOutline,
  type RawOutlineNode,
} from "@/panels/outline-tree";

const resolverByRef = (map: Record<string, number | null>) => {
  return async (dest: string | unknown[]) => {
    const key = typeof dest === "string" ? dest : String((dest as unknown[])[0]);
    return key in map ? map[key] : null;
  };
};

describe("normalizeOutline", () => {
  it("returns [] for null / undefined / empty", async () => {
    const r = async () => null;
    expect(await normalizeOutline(null, r)).toEqual([]);
    expect(await normalizeOutline(undefined, r)).toEqual([]);
    expect(await normalizeOutline([], r)).toEqual([]);
  });

  it("normalises a single-level outline with resolved destinations", async () => {
    const raw: RawOutlineNode[] = [
      { title: "Cover", dest: ["ref-cover"] },
      { title: "Chapter 1", dest: ["ref-c1"] },
      { title: "Chapter 2", dest: ["ref-c2"] },
    ];
    const result = await normalizeOutline(
      raw,
      resolverByRef({ "ref-cover": 1, "ref-c1": 5, "ref-c2": 23 }),
    );
    expect(result).toEqual([
      { title: "Cover", page: 1, children: [] },
      { title: "Chapter 1", page: 5, children: [] },
      { title: "Chapter 2", page: 23, children: [] },
    ]);
  });

  it("recursively normalises nested children", async () => {
    const raw: RawOutlineNode[] = [
      {
        title: "Part I",
        dest: ["ref-pt1"],
        items: [
          { title: "Ch 1", dest: ["ref-ch1"] },
          { title: "Ch 2", dest: ["ref-ch2"], items: [
            { title: "§ 2.1", dest: ["ref-s21"] },
          ]},
        ],
      },
    ];
    const result = await normalizeOutline(
      raw,
      resolverByRef({
        "ref-pt1": 1, "ref-ch1": 2, "ref-ch2": 10, "ref-s21": 12,
      }),
    );
    expect(result).toEqual([
      {
        title: "Part I",
        page: 1,
        children: [
          { title: "Ch 1", page: 2, children: [] },
          {
            title: "Ch 2",
            page: 10,
            children: [{ title: "§ 2.1", page: 12, children: [] }],
          },
        ],
      },
    ]);
  });

  it("yields page: null for nodes with no dest", async () => {
    const raw: RawOutlineNode[] = [{ title: "No target", dest: null }];
    const result = await normalizeOutline(raw, async () => 999);
    expect(result[0].page).toBeNull();
  });

  it("yields page: null when the resolver returns null", async () => {
    const raw: RawOutlineNode[] = [
      { title: "Broken link", dest: ["unknown-ref"] },
    ];
    const result = await normalizeOutline(raw, async () => null);
    expect(result[0].page).toBeNull();
  });

  it("accepts string (named) destinations", async () => {
    const raw: RawOutlineNode[] = [{ title: "Named", dest: "my-named-dest" }];
    const resolver = vi.fn(async (d) => (d === "my-named-dest" ? 42 : null));
    const result = await normalizeOutline(raw, resolver);
    expect(result[0].page).toBe(42);
    expect(resolver).toHaveBeenCalledWith("my-named-dest");
  });
});

describe("countOutlineEntries", () => {
  it("counts a flat tree", () => {
    expect(
      countOutlineEntries([
        { title: "a", page: 1, children: [] },
        { title: "b", page: 2, children: [] },
      ]),
    ).toBe(2);
  });

  it("counts a deep tree", () => {
    expect(
      countOutlineEntries([
        {
          title: "a",
          page: 1,
          children: [
            { title: "a1", page: 2, children: [] },
            {
              title: "a2",
              page: 3,
              children: [{ title: "a2.1", page: 4, children: [] }],
            },
          ],
        },
        { title: "b", page: 5, children: [] },
      ]),
    ).toBe(5);
  });

  it("counts [] as 0", () => {
    expect(countOutlineEntries([])).toBe(0);
  });
});
