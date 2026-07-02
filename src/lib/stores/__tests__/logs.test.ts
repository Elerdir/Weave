import { describe, it, expect, vi, beforeEach } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { logsStore } from "$lib/stores/logs.svelte";
import type { LogEntry } from "$lib/stores/logs.svelte";

const mockedInvoke = vi.mocked(invoke);

const sample: LogEntry[] = [
  {
    timestamp: "2026-07-02T13:00:00Z",
    level: "INFO",
    target: "weave_shell::commands::message",
    message: "Zpráva odeslána",
  },
  {
    timestamp: "2026-07-02T13:00:01Z",
    level: "ERROR",
    target: "weave_infrastructure::comfyui",
    message: "Workflow selhal",
  },
];

describe("logsStore", () => {
  beforeEach(() => {
    mockedInvoke.mockReset();
  });

  it("load() volá get_app_logs s aktivními filtry", async () => {
    mockedInvoke.mockResolvedValue(sample);
    logsStore.minLevel = "warn";
    logsStore.target = "comfy";
    logsStore.search = "workflow";

    await logsStore.load();

    expect(mockedInvoke).toHaveBeenCalledWith("get_app_logs", {
      minLevel: "warn",
      target: "comfy",
      search: "workflow",
      limit: 500,
    });
    expect(logsStore.entries).toHaveLength(2);
  });

  it("prázdné filtry se posílají jako null", async () => {
    mockedInvoke.mockResolvedValue([]);
    logsStore.minLevel = "";
    logsStore.target = "";
    logsStore.search = "";

    await logsStore.load();

    expect(mockedInvoke).toHaveBeenCalledWith("get_app_logs", {
      minLevel: null,
      target: null,
      search: null,
      limit: 500,
    });
  });

  it("targets vrací unikátní kořenové moduly seřazené", async () => {
    mockedInvoke.mockResolvedValue([
      ...sample,
      {
        timestamp: "2026-07-02T13:00:02Z",
        level: "INFO",
        target: "weave_shell::commands::models",
        message: "další",
      },
    ]);
    await logsStore.load();

    expect(logsStore.targets).toEqual(["weave_infrastructure", "weave_shell"]);
  });
});
