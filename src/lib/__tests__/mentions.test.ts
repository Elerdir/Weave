import { describe, it, expect } from "vitest";
import { activeMention, removeMentionToken } from "$lib/mentions";

describe("activeMention", () => {
  it("najde @token na konci textu", () => {
    const text = "shrň mi @pozn";
    const m = activeMention(text, text.length);
    expect(m).not.toBeNull();
    expect(m!.query).toBe("pozn");
    expect(m!.start).toBe(8);
    expect(m!.end).toBe(text.length);
  });

  it("najde @token na začátku", () => {
    const m = activeMention("@readme", 7);
    expect(m?.query).toBe("readme");
    expect(m?.start).toBe(0);
  });

  it("vrátí null mimo @token", () => {
    expect(activeMention("normální text", 5)).toBeNull();
  });

  it("vrátí null když @ není po mezeře (e-mail)", () => {
    expect(activeMention("user@domain", 11)).toBeNull();
  });

  it("vrátí null když je za @ mezera (token ukončen)", () => {
    expect(activeMention("@soubor potom", 13)).toBeNull();
  });

  it("prázdný query hned po @", () => {
    const m = activeMention("ahoj @", 6);
    expect(m?.query).toBe("");
  });
});

describe("removeMentionToken", () => {
  it("odebere @token z textu", () => {
    const text = "shrň mi @pozn";
    const m = activeMention(text, text.length)!;
    expect(removeMentionToken(text, m)).toBe("shrň mi ");
  });

  it("odebere @token uprostřed a sloučí mezery", () => {
    const text = "před @ref po";
    const m = activeMention(text, 9)!; // '@ref'
    expect(removeMentionToken(text, m)).toBe("před po");
  });
});
