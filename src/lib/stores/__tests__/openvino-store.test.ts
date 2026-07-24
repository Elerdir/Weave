import { describe, it, expect, vi, beforeEach } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import {
  openvinoInstallStore,
  type OpenvinoRuntimeStatus,
  type OpenvinoModelProfile,
} from "$lib/stores/openvino-install.svelte";

const mockInvoke = vi.mocked(invoke);

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
    savedDevice: "",
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
    supportedDevices: ["NPU", "GPU", "CPU"],
  },
  {
    id: "qwen3-30b-a3b-int4-ov",
    name: "Qwen3 30B-A3B INT4 (MoE)",
    description: "",
    targetDir: "C:/weave/openvino/models/Qwen3-30B-A3B-int4-ov",
    repoId: "OpenVINO/Qwen3-30B-A3B-int4-ov",
    sourceUrl: null,
    autoDownloadable: true,
    sizeHint: "",
    qualityTier: "",
    supportedDevices: ["CPU"],
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
    // Store je singleton — modelDir/device z předchozího testu by zamaskoval obnovu.
    openvinoInstallStore.setModelDir("");
    openvinoInstallStore.setDevice("NPU");
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

  it("obnoví uložené zařízení", async () => {
    mockLoad(status({ savedDevice: "GPU.0" }));
    await openvinoInstallStore.load();
    expect(openvinoInstallStore.device).toBe("GPU.0");
  });

  it("bez uloženého zařízení a bez NPU vybere první dostupné", async () => {
    mockLoad(
      status({
        savedDevice: "",
        deviceCheck: { devices: ["CPU", "GPU.0"], hasNpu: false, openvino: "2026.2.1" },
      }),
    );
    await openvinoInstallStore.load();
    expect(openvinoInstallStore.device).toBe("CPU");
  });

  // Uzivatel nesmi videt (a stahnout 16 GB) model, ktery mu na zvolenem
  // zarizeni spadne az pri startu serveru.
  it("nabídne jen modely použitelné na zvoleném zařízení", async () => {
    mockLoad(status());
    await openvinoInstallStore.load();

    openvinoInstallStore.setDevice("NPU");
    expect(openvinoInstallStore.profilesForDevice.map((p) => p.id)).toEqual([
      "qwen3-8b-int4-cw-ov",
    ]);

    openvinoInstallStore.setDevice("CPU");
    expect(openvinoInstallStore.profilesForDevice.map((p) => p.id)).toEqual([
      "qwen3-8b-int4-cw-ov",
      "qwen3-30b-a3b-int4-ov",
    ]);
  });

  it("GPU.0 se filtruje jako rodina GPU", async () => {
    mockLoad(status());
    await openvinoInstallStore.load();

    openvinoInstallStore.setDevice("GPU.0");
    expect(openvinoInstallStore.profilesForDevice.map((p) => p.id)).toEqual([
      "qwen3-8b-int4-cw-ov",
    ]);
  });

  it("přepnutí na zařízení bez podpory modelu vybere jiný model", async () => {
    mockLoad(status());
    await openvinoInstallStore.load();

    openvinoInstallStore.setDevice("CPU");
    openvinoInstallStore.setSelectedProfile("qwen3-30b-a3b-int4-ov");
    expect(openvinoInstallStore.selectedProfile?.id).toBe("qwen3-30b-a3b-int4-ov");

    // 30B na NPU nejede — volba musí spadnout na model, který tam běží.
    openvinoInstallStore.setDevice("NPU");
    expect(openvinoInstallStore.selectedProfile?.id).toBe("qwen3-8b-int4-cw-ov");
  });

  it("deviceOptions preferuje reálně zjištěná zařízení", async () => {
    mockLoad(status({ deviceCheck: { devices: ["CPU", "GPU.0", "NPU"], hasNpu: true, openvino: "2026.2.1" } }));
    await openvinoInstallStore.load();
    expect(openvinoInstallStore.deviceOptions).toEqual(["CPU", "GPU.0", "NPU"]);
  });
});
