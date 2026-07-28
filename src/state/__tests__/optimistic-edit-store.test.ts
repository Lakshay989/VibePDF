// P4.HF29 — optimistic edit store lifecycle: add → tie → prune-on-render.

import { beforeEach, describe, expect, it } from "vitest";

import { useOptimisticEditStore } from "@/state/optimistic-edit-store";

const store = useOptimisticEditStore;
const held = (doc: string) => store.getState().byDoc[doc] ?? [];

beforeEach(() => {
  store.setState({ byDoc: {}, renderedEpoch: {} });
});

describe("optimistic edit store", () => {
  it("shows a committed edit immediately, untied (epoch null)", () => {
    const key = store.getState().add("d", 0, "ink", { n: 1 });
    expect(held("d")).toHaveLength(1);
    expect(held("d")[0]).toMatchObject({ key, page: 0, kind: "ink", epoch: null });
  });

  it("does not prune an untied edit when a reload renders", () => {
    store.getState().add("d", 0, "ink", {});
    store.getState().markRendered("d", 5); // reload landed, but this edit isn't tied yet
    expect(held("d")).toHaveLength(1);
  });

  it("prunes a tied edit once its bake epoch has rendered", () => {
    const key = store.getState().add("d", 0, "ink", {});
    store.getState().tie("d", key, 3);
    store.getState().markRendered("d", 2); // earlier reload — not yet baked
    expect(held("d")).toHaveLength(1);
    store.getState().markRendered("d", 3); // the baking reload
    expect(held("d")).toHaveLength(0);
  });

  it("keeps rapid strokes independent — each clears on its own bake", () => {
    const a = store.getState().add("d", 0, "ink", { s: "a" });
    const b = store.getState().add("d", 0, "ink", { s: "b" });
    store.getState().tie("d", a, 4);
    store.getState().tie("d", b, 5);
    store.getState().markRendered("d", 4); // only stroke a is baked
    expect(held("d").map((h) => h.key)).toEqual([b]);
    store.getState().markRendered("d", 5);
    expect(held("d")).toHaveLength(0);
  });

  it("tie prunes immediately if the bake already rendered (fast small-doc edit)", () => {
    const key = store.getState().add("d", 0, "ink", {});
    store.getState().markRendered("d", 7); // reload raced ahead of the tie
    store.getState().tie("d", key, 7);
    expect(held("d")).toHaveLength(0);
  });

  it("remove drops a held edit (e.g. its write rejected)", () => {
    const key = store.getState().add("d", 0, "ink", {});
    store.getState().remove("d", key);
    expect(held("d")).toHaveLength(0);
  });

  it("markRendered never regresses the rendered epoch", () => {
    store.getState().markRendered("d", 9);
    store.getState().markRendered("d", 4);
    expect(store.getState().renderedEpoch.d).toBe(9);
  });

  it("keeps documents isolated", () => {
    store.getState().add("d1", 0, "ink", {});
    store.getState().add("d2", 1, "ink", {});
    store.getState().markRendered("d1", 100);
    // d2's untied edit is untouched by d1's reload.
    expect(held("d2")).toHaveLength(1);
  });
});
