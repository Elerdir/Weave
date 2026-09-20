import { describe, it, expect, vi, beforeEach } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { i18n } from "$lib/i18n/index.svelte";

const mockInvoke = vi.mocked(invoke);

function backendWith(stored: string | null) {
  mockInvoke.mockImplementation(async (cmd: string) => {
    if (cmd === "get_app_setting") return stored;
    if (cmd === "set_app_setting") return null;
    throw new Error(`neočekávaný příkaz: ${cmd}`);
  });
}

describe("i18n — uložení jazyka", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    localStorage.clear();
    // Výchozí stav: backend odpovídá, v databázi nic není. Jednotlivé testy
    // si ho přepíšou. Bez toho by se implementace mocku přenášela mezi testy
    // a výsledek by závisel na jejich pořadí.
    backendWith(null);
  });

  it("načte jazyk z databáze", async () => {
    i18n.setLocale("cs");
    backendWith("en");

    await i18n.hydrate();

    expect(i18n.locale).toBe("en");
  });

  it("neznámý jazyk z databáze ignoruje", async () => {
    i18n.setLocale("cs");
    backendWith("klingon");

    await i18n.hydrate();

    expect(i18n.locale).toBe("cs");
  });

  it("při prázdné databázi tam přenese dosavadní volbu", async () => {
    i18n.setLocale("en");
    vi.clearAllMocks();
    backendWith(null);

    await i18n.hydrate();

    expect(mockInvoke).toHaveBeenCalledWith("set_app_setting", {
      key: "app.locale",
      value: "en",
    });
  });

  it("uložení jde do databáze i do keše", () => {
    backendWith(null);
    i18n.setLocale("cs");

    expect(mockInvoke).toHaveBeenCalledWith("set_app_setting", {
      key: "app.locale",
      value: "cs",
    });
    expect(localStorage.getItem("weave.locale")).toBe("cs");
  });

  it("nedostupný backend jazyk neshodí", async () => {
    i18n.setLocale("cs");
    mockInvoke.mockRejectedValue(new Error("mimo Tauri"));

    await expect(i18n.hydrate()).resolves.toBeUndefined();
    expect(i18n.locale).toBe("cs");
  });
});
