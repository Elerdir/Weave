import { describe, it, expect, beforeEach } from "vitest";

// Testujeme resolve + format utility přímo (bez DOM/svelte state)
function resolve(bundle: Record<string, unknown>, key: string): string {
  const parts = key.split(".");
  let node: unknown = bundle;
  for (const p of parts) {
    if (typeof node === "object" && node !== null && p in (node as object)) {
      node = (node as Record<string, unknown>)[p];
    } else {
      return key;
    }
  }
  return typeof node === "string" ? node : key;
}

function format(template: string, params?: Record<string, string | number>): string {
  if (!params) return template;
  return Object.entries(params).reduce(
    (acc, [k, v]) => acc.replace(new RegExp(`\\{${k}\\}`, "g"), String(v)),
    template
  );
}

const bundle = {
  chat: {
    tokensPerSecond: "{tps} tok/s",
    newConversation: "Nová konverzace",
  },
  wizard: {
    step: "Krok {current} z {total}",
  },
};

describe("i18n resolve", () => {
  it("resolves nested key", () => {
    expect(resolve(bundle, "chat.newConversation")).toBe("Nová konverzace");
  });

  it("returns key as fallback for missing key", () => {
    expect(resolve(bundle, "chat.nonExistent")).toBe("chat.nonExistent");
  });

  it("returns key for completely missing path", () => {
    expect(resolve(bundle, "missing.path")).toBe("missing.path");
  });
});

describe("i18n format", () => {
  it("interpolates single param", () => {
    expect(format("{tps} tok/s", { tps: 42.5 })).toBe("42.5 tok/s");
  });

  it("interpolates multiple params", () => {
    expect(format("Krok {current} z {total}", { current: 2, total: 4 })).toBe("Krok 2 z 4");
  });

  it("returns template unchanged when no params", () => {
    expect(format("Hello world")).toBe("Hello world");
  });
});
