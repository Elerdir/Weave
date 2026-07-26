import { describe, it, expect, vi, beforeEach } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  openvinoInstallStore,
  type OpenvinoRuntimeStatus,
  type OpenvinoModelProfile,
} from "$lib/stores/openvino-install.svelte";

const mockInvoke = vi.mocked(invoke);
const mockListen = vi.mocked(listen);

function status(overrides: Partial<OpenvinoRuntimeStatus> = {}): OpenvinoRuntimeStatus {
  return {
    installed: true,
    serverRunning: false,
    installDir: "C:/weave/openvino",
    pythonPath: "C:/weave/openvino/venv/Scripts/python.exe",
    requirementsPath: "C:/weave/openvino/requirements-openvino.txt",
    serverLogPath: "C:/weave/openvino/weave_openvino_server.log",
    defaultModelDir: "C:/weave/openvino/models/qwen3-8b-int4-cw-ov",
    savedModelDir: "",
    deviceCheck: { devices: ["CPU", "GPU", "NPU"], hasNpu: true, openvino: "2026.2.1" },
    ...overrides,
  };
}

const profiles: OpenvinoModelProfile[] = [
  {
    id: "qwen3-8b-int4-cw-ov",
    name: "Qwen3 8B INT4 OpenVINO",
    description: "",
    targetDir: "C:/weave/openvino/models/qwen3-8b-int4-cw-ov",
    repoId: "OpenVINO/Qwen3-8B-int4-cw-ov",
    sourceUrl: null,
    autoDownloadable: true,
    sizeHint: "",
    qualityTier: "",
  },
];

function mockLoad(next: OpenvinoRuntimeStatus) {
  mockInvoke.mockImplementation(async (cmd: string) => {
    if (cmd === "get_openvino_runtime_status") return next;
    if (cmd === "list_openvino_model_profiles") return profiles;
    throw new Error(`neočekávaný příkaz: ${cmd}`);
  });
}

describe("openvinoInstallStore", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    // Store je singleton — modelDir z předchozího testu by zamaskoval obnovu.
    openvinoInstallStore.setModelDir("");
  });

  it("obnoví uloženou složku modelu místo výchozí cesty profilu", async () => {
    mockLoad(status({ savedModelDir: "D:/modely/gemma-3-4b-it-int4-cw-ov" }));

    await openvinoInstallStore.load();

    expect(openvinoInstallStore.modelDir).toBe("D:/modely/gemma-3-4b-it-int4-cw-ov");
  });

  it("bez uložené složky spadne zpět na cestu vybraného profilu", async () => {
    mockLoad(status());

    await openvinoInstallStore.load();

    expect(openvinoInstallStore.modelDir).toBe(profiles[0].targetDir);
  });

  it("hlásí chybějící NPU, když ho ověření nenašlo", async () => {
    mockLoad(
      status({
        deviceCheck: { devices: ["CPU", "GPU"], hasNpu: false, openvino: "2026.2.1" },
      }),
    );

    await openvinoInstallStore.load();

    expect(openvinoInstallStore.npuMissing).toBe(true);
    expect(openvinoInstallStore.availableDevices).toEqual(["CPU", "GPU"]);
  });

  it("nehlásí chybějící NPU, dokud runtime není nainstalovaný ani ověřený", async () => {
    mockLoad(status({ installed: false, deviceCheck: null }));
    await openvinoInstallStore.load();
    expect(openvinoInstallStore.npuMissing).toBe(false);

    mockLoad(status({ installed: true, deviceCheck: null }));
    await openvinoInstallStore.load();
    expect(openvinoInstallStore.npuMissing).toBe(false);
  });

  // Regrese: stahování modelu (jednotky GB) dřív neposlouchalo progress kanál
  // vůbec — UI ukazovalo jen "načítám" a vypadalo zaseknutě.
  it("stahování modelu streamuje průběh do logu", async () => {
    const unlisten = vi.fn();
    let emit: ((e: { payload: unknown }) => void) | undefined;
    mockListen.mockImplementation(async (_event: string, cb: any) => {
      emit = cb;
      return unlisten;
    });
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === "download_openvino_model_profile") {
        emit?.({ payload: { type: "step", name: "Stahuji Qwen3 8B" } });
        emit?.({ payload: { type: "output", line: "Fetching 12 files:  40%" } });
        return status();
      }
      if (cmd === "get_openvino_runtime_status") return status();
      if (cmd === "list_openvino_model_profiles") return profiles;
      throw new Error(`neočekávaný příkaz: ${cmd}`);
    });

    await openvinoInstallStore.downloadRecommendedModel();

    expect(openvinoInstallStore.log).toContain("> Stahuji Qwen3 8B");
    expect(openvinoInstallStore.log).toContain("Fetching 12 files:  40%");
    // Posluchač se musí odhlásit, jinak by se při opakovaném stahování hromadil.
    expect(unlisten).toHaveBeenCalled();
  });
});
