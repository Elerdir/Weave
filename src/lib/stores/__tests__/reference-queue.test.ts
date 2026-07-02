import { describe, it, expect } from "vitest";
import { referenceQueue } from "$lib/stores/reference-queue.svelte";

describe("referenceQueue", () => {
  it("add() přidává a drain() vrátí a vyprázdní frontu", () => {
    referenceQueue.add("/gallery/a.png");
    referenceQueue.add("/gallery/b.png");
    expect(referenceQueue.pending).toHaveLength(2);

    const drained = referenceQueue.drain();
    expect(drained).toEqual(["/gallery/a.png", "/gallery/b.png"]);
    expect(referenceQueue.pending).toHaveLength(0);
  });
});
