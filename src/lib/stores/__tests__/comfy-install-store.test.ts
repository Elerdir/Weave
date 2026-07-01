import { describe, it, expect, vi, beforeEach } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { comfyInstallStore } from "$lib/stores/comfy-install.svelte";

const mockInvoke = vi.mocked(invoke);
const mockListen = vi.mocked(listen);

describe("comfyInstallStore", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("load() nastaví status podle backendu", async () => {
    mockInvoke.mockResolvedValueOnce("Installed");
    await comfyInstallStore.load();
    expect(comfyInstallStore.status).toBe("Installed");
  });

  it("install() naslouchá progress eventům a aktualizuje log", async () => {
    let handler: ((e: { payload: unknown }) => void) | null = null;
    mockListen.mockImplementation(async (_event, cb) => {
      handler = cb as (e: { payload: unknown }) => void;
      return () => {};
    });
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === "install_comfyui") {
        // Simuluj progress eventy poté, co se posluchač zaregistruje
        handler?.({ payload: { type: "step", name: "Stahuji ComfyUI" } });
        handler?.({ payload: { type: "output", line: "Cloning into ComfyUI..." } });
        return undefined;
      }
      if (cmd === "get_comfyui_status") return "Installed";
      return undefined;
    });

    const promise = comfyInstallStore.install();
    await promise;

    expect(comfyInstallStore.log.some((l) => l.includes("Stahuji ComfyUI"))).toBe(true);
    expect(comfyInstallStore.log.some((l) => l.includes("Cloning into"))).toBe(true);
  });

  it("startServer() nastaví status na Running při úspěchu", async () => {
    mockInvoke.mockResolvedValueOnce(undefined);
    await comfyInstallStore.startServer();
    expect(comfyInstallStore.status).toBe("Running");
    expect(comfyInstallStore.starting).toBe(false);
  });

  it("startServer() zaznamená chybu při selhání", async () => {
    mockInvoke.mockRejectedValueOnce(new Error("ComfyUI není nainstalováno"));
    await comfyInstallStore.startServer();
    expect(comfyInstallStore.error).toContain("ComfyUI");
  });

  it("stopServer() nastaví status zpět na Installed", async () => {
    mockInvoke.mockResolvedValueOnce(undefined);
    await comfyInstallStore.stopServer();
    expect(comfyInstallStore.status).toBe("Installed");
  });
});
