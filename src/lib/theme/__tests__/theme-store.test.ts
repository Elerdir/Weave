import { describe, it, expect, vi, beforeEach } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { themeStore } from "$lib/theme/index.svelte";

const mockInvoke = vi.mocked(invoke);

/** Odpovědi backendu: `get_app_setting` vrátí zadanou hodnotu, zápis projde. */
function backendWith(stored: string | null) {
  mockInvoke.mockImplementation(async (cmd: string) => {
    if (cmd === "get_app_setting") return stored;
    if (cmd === "set_app_setting") return null;
    throw new Error(`neočekávaný příkaz: ${cmd}`);
  });
}

describe("themeStore", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    localStorage.clear();
    // Výchozí stav: backend odpovídá, v databázi nic není. Jednotlivé testy
    // si ho přepíšou. Bez toho by se implementace mocku přenášela mezi testy
    // a výsledek by závisel na jejich pořadí.
    backendWith(null);
  });

  it("načte motiv z databáze", async () => {
    backendWith("light");
    await themeStore.hydrate();
    expect(themeStore.theme).toBe("light");
  });

  it("nesmyslnou hodnotu z databáze ignoruje", async () => {
    themeStore.setTheme("dark");
    backendWith("nesmysl");
    await themeStore.hydrate();
    expect(themeStore.theme).toBe("dark");
  });

  it("při prázdné databázi tam přenese dosavadní volbu", async () => {
    themeStore.setTheme("light");
    vi.clearAllMocks();
    backendWith(null);

    await themeStore.hydrate();

    expect(mockInvoke).toHaveBeenCalledWith("set_app_setting", {
      key: "app.theme",
      value: "light",
    });
  });

  it("uložení jde do databáze i do keše", () => {
    backendWith(null);
    themeStore.setTheme("dark");

    expect(mockInvoke).toHaveBeenCalledWith("set_app_setting", {
      key: "app.theme",
      value: "dark",
    });
    expect(localStorage.getItem("weave.theme")).toBe("dark");
  });

  it("nedostupný backend motiv neshodí", async () => {
    // Regrese: bez try/catch by pád invoke zastavil start aplikace.
    themeStore.setTheme("light");
    mockInvoke.mockRejectedValue(new Error("mimo Tauri"));

    await expect(themeStore.hydrate()).resolves.toBeUndefined();
    expect(themeStore.theme).toBe("light");
  });
});
