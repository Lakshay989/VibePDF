// SPEC: P3-ANN-008 — the sidebar's pure filter/group helpers: search + filter by
// type / author / date, and grouping by page.

import { describe, expect, it } from "vitest";

import type { AnnotationInfo } from "@/ipc/annotations";
import {
  buildThreads,
  dateInputToMs,
  distinctAuthors,
  distinctKinds,
  EMPTY_FILTER,
  filterAnnotations,
  groupByPage,
  msToDateInput,
} from "@/panels/annotation-filter";

const a = (over: Partial<AnnotationInfo>): AnnotationInfo => ({
  id: "1 0",
  page: 0,
  kind: "note",
  rect: [0, 0, 10, 10],
  contents: "",
  author: "",
  modified: null,
  inReplyTo: null,
  ...over,
});

const list: AnnotationInfo[] = [
  a({ id: "1", page: 2, kind: "note", contents: "hello world", author: "Ada", modified: 2000 }),
  a({ id: "2", page: 0, kind: "highlight", contents: "important", author: "Bo" }),
  a({ id: "3", page: 0, kind: "freetext", contents: "a caption", author: "Ada", modified: 5000 }),
];

describe("date input helpers", () => {
  it("parses a date input value as local midnight (not UTC)", () => {
    const ms = dateInputToMs("2026-06-21");
    expect(ms).not.toBeNull();
    const dt = new Date(ms as number);
    // Local Y/M/D must match the picked day regardless of timezone — a UTC
    // parse would shift the date in zones west/east of UTC.
    expect(dt.getFullYear()).toBe(2026);
    expect(dt.getMonth()).toBe(5); // June (0-based)
    expect(dt.getDate()).toBe(21);
    expect(dt.getHours()).toBe(0);
  });

  it("treats an empty value as no filter", () => {
    expect(dateInputToMs("")).toBeNull();
  });

  it("round-trips through msToDateInput in local time", () => {
    const ms = dateInputToMs("2026-01-05");
    expect(msToDateInput(ms)).toBe("2026-01-05");
    expect(msToDateInput(null)).toBe("");
  });

  it("includes an annotation modified later the same local day", () => {
    const after = dateInputToMs("2026-06-21") as number;
    // 14:30 local on the picked day → must pass "modified on or after" the day.
    const sameDayAfternoon = new Date(2026, 5, 21, 14, 30).getTime();
    const filtered = filterAnnotations([a({ id: "x", modified: sameDayAfternoon })], {
      ...EMPTY_FILTER,
      modifiedAfter: after,
    });
    expect(filtered.map((x) => x.id)).toEqual(["x"]);
  });
});

describe("buildThreads", () => {
  it("nests replies under their parent, ordered by modified", () => {
    const items = [
      a({ id: "root", contents: "question", modified: 100 }),
      a({ id: "r2", contents: "second reply", inReplyTo: "root", modified: 300 }),
      a({ id: "r1", contents: "first reply", inReplyTo: "root", modified: 200 }),
    ];
    const threads = buildThreads(items);
    expect(threads).toHaveLength(1);
    expect(threads[0].root.id).toBe("root");
    expect(threads[0].replies.map((r) => r.id)).toEqual(["r1", "r2"]); // chronological
  });

  it("flattens a reply-to-a-reply under the thread root", () => {
    const items = [
      a({ id: "root", modified: 1 }),
      a({ id: "r1", inReplyTo: "root", modified: 2 }),
      a({ id: "r1a", inReplyTo: "r1", modified: 3 }),
    ];
    const threads = buildThreads(items);
    expect(threads).toHaveLength(1);
    expect(threads[0].replies.map((r) => r.id)).toEqual(["r1", "r1a"]);
  });

  it("treats an orphan reply (missing parent) as its own root", () => {
    const threads = buildThreads([a({ id: "x", inReplyTo: "gone" })]);
    expect(threads).toHaveLength(1);
    expect(threads[0].root.id).toBe("x");
    expect(threads[0].replies).toHaveLength(0);
  });

  it("is cycle-safe (mutual replies don't loop or drop)", () => {
    const threads = buildThreads([
      a({ id: "a", inReplyTo: "b" }),
      a({ id: "b", inReplyTo: "a" }),
    ]);
    // Both survive as roots; the exact rooting is unspecified for malformed input.
    const ids = threads.flatMap((t) => [t.root.id, ...t.replies.map((r) => r.id)]);
    expect(new Set(ids)).toEqual(new Set(["a", "b"]));
  });

  it("each top-level annotation with no replies is its own thread", () => {
    const threads = buildThreads(list);
    expect(threads).toHaveLength(3);
    expect(threads.every((t) => t.replies.length === 0)).toBe(true);
  });
});

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
