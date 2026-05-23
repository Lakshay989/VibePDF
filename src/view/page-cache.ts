// SPEC: P1-VIEW-005 / NFR-PERF-003 — bounded page-bitmap cache so
// rapid scroll-back doesn't re-rasterize pages we just unmounted.
//
// Pure Map-backed LRU: insertion order = recency, most-recent at the
// tail. get() promotes to tail; over-capacity inserts evict the head.
//
// Keys are opaque strings (the virtualizer constructs them from
// documentId + pageNumber + scale + DPR). Values are anything
// renderable; in practice ImageBitmap or null (the "we've started
// rendering but it isn't ready" placeholder).

export class LruCache<V> {
  private readonly map = new Map<string, V>();

  constructor(public readonly capacity: number) {
    if (capacity <= 0) throw new Error("LruCache capacity must be > 0");
  }

  get(key: string): V | undefined {
    const v = this.map.get(key);
    if (v === undefined) return undefined;
    this.map.delete(key);
    this.map.set(key, v);
    return v;
  }

  set(key: string, value: V): void {
    if (this.map.has(key)) this.map.delete(key);
    this.map.set(key, value);
    if (this.map.size > this.capacity) {
      const oldest = this.map.keys().next().value;
      if (oldest !== undefined) this.map.delete(oldest);
    }
  }

  has(key: string): boolean {
    return this.map.has(key);
  }

  get size(): number {
    return this.map.size;
  }

  clear(): void {
    this.map.clear();
  }

  /** Test/debug helper — iteration order matches LRU order (oldest first). */
  keys(): string[] {
    return Array.from(this.map.keys());
  }
}
