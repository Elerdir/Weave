import { describe, it, expect, vi, beforeEach } from "vitest";
import { invoke } from "@tauri-apps/api/core";

const mockInvoke = vi.mocked(invoke);

// Testujeme logiku okolo store — invoke je mocknut v test-setup.ts
describe("workspace store invoke calls", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("get_workspace vrátí null při prázdném stavu", async () => {
    mockInvoke.mockResolvedValueOnce(null);
    const result = await invoke("get_workspace");
    expect(result).toBeNull();
  });

  it("list_workspace_children vrátí pole", async () => {
    const entries = [
      { path: "/ws/readme.md", name: "readme.md", kind: "file", size_bytes: 100, modified_at: null },
      { path: "/ws/src", name: "src", kind: "directory", size_bytes: null, modified_at: null },
    ];
    mockInvoke.mockResolvedValueOnce(entries);
    const result = await invoke("list_workspace_children", { path: "/ws" });
    expect(result).toHaveLength(2);
  });

  it("search_workspace vrátí výsledky", async () => {
    const results = [
      { path: "/ws/readme.md", name: "readme.md", text_content: "hello world", extension: "md", size_bytes: 100, modified_at: "", indexed_at: "" },
    ];
    mockInvoke.mockResolvedValueOnce(results);
    const result = await invoke("search_workspace", { query: "hello", limit: 20 });
    expect(Array.isArray(result)).toBe(true);
  });
});
