// SPEC: P3-ANN-008 — the sidebar's pure filter/group helpers: search + filter by
// type / author / date, and grouping by page.

import { describe, expect, it } from "vitest";

import type { AnnotationInfo } from "@/ipc/annotations";
import {
  distinctAuthors,
  distinctKinds,
  EMPTY_FILTER,
  filterAnnotations,
  groupByPage,
} from "@/panels/annotation-filter";

const a = (over: Partial<AnnotationInfo>): AnnotationInfo => ({
  id: "1 0",
  page: 0,
  kind: "note",
  rect: [0, 0, 10, 10],
  contents: "",
  author: "",
  modified: null,
  ...over,
});

const list: AnnotationInfo[] = [
  a({ id: "1", page: 2, kind: "note", contents: "hello world", author: "Ada", modified: 2000 }),
  a({ id: "2", page: 0, kind: "highlight", contents: "important", author: "Bo" }),
  a({ id: "3", page: 0, kind: "freetext", contents: "a caption", author: "Ada", modified: 5000 }),
];

describe("filterAnnotations", () => {
  it("returns everything with the empty filter", () => {
    expect(filterAnnotations(list, EMPTY_FILTER)).toHaveLength(3);
  });

  it("filters by search across contents, author, and kind label", () => {
    expect(filterAnnotations(list, { ...EMPTY_FILTER, search: "hello" }).map((x) => x.id)).toEqual(["1"]);
    expect(filterAnnotations(list, { ...EMPTY_FILTER, search: "ada" }).map((x) => x.id)).toEqual(["1", "3"]);
    expect(filterAnnotations(list, { ...EMPTY_FILTER, search: "free text" }).map((x) => x.id)).toEqual(["3"]);
  });

  it("filters by kind", () => {
    expect(filterAnnotations(list, { ...EMPTY_FILTER, kinds: ["highlight"] }).map((x) => x.id)).toEqual(["2"]);
    expect(
      filterAnnotations(list, { ...EMPTY_FILTER, kinds: ["note", "freetext"] }).map((x) => x.id),
    ).toEqual(["1", "3"]);
  });

  it("filters by author", () => {
    expect(filterAnnotations(list, { ...EMPTY_FILTER, author: "Bo" }).map((x) => x.id)).toEqual(["2"]);
  });

  it("filters by modifiedAfter, dropping undated annotations", () => {
    const out = filterAnnotations(list, { ...EMPTY_FILTER, modifiedAfter: 3000 });
    expect(out.map((x) => x.id)).toEqual(["3"]); // id 2 has no date, id 1 is older
  });
});

describe("groupByPage", () => {
  it("groups ascending by page, keeping input order within a page", () => {
    const groups = groupByPage(list);
    expect(groups.map((g) => g.page)).toEqual([0, 2]);
    expect(groups[0].items.map((x) => x.id)).toEqual(["2", "3"]);
    expect(groups[1].items.map((x) => x.id)).toEqual(["1"]);
  });
});

describe("distinct helpers", () => {
  it("lists distinct non-empty authors sorted", () => {
    expect(distinctAuthors(list)).toEqual(["Ada", "Bo"]);
  });

  it("lists present kinds in display order", () => {
    expect(distinctKinds(list)).toEqual(["highlight", "note", "freetext"]);
  });
});
