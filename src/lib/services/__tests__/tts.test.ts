import { describe, it, expect, vi, beforeEach } from "vitest";
import { tts, stripForSpeech } from "$lib/services/tts.svelte";

class MockUtterance {
  text: string;
  lang = "";
  onend: (() => void) | null = null;
  onerror: (() => void) | null = null;
  constructor(text: string) {
    this.text = text;
  }
}

describe("stripForSpeech", () => {
  it("nahradí code blok a inline kód", () => {
    expect(stripForSpeech("Text ```rust\nfn x(){}\n``` konec")).toContain("(blok kódu)");
    expect(stripForSpeech("Zkus `let x`")).toBe("Zkus let x");
  });

  it("nahradí obrázky a odkazy", () => {
    expect(stripForSpeech("![alt](http://x.png)")).toContain("(obrázek)");
    expect(stripForSpeech("viz [dokumentace](http://x)")).toBe("viz dokumentace");
  });

  it("odstraní markdown značky", () => {
    expect(stripForSpeech("**tučně** a *kurzíva* # nadpis")).toBe("tučně a kurzíva nadpis");
  });
});

describe("tts", () => {
  let spoken: MockUtterance[];

  beforeEach(() => {
    spoken = [];
    (globalThis as unknown as Record<string, unknown>).SpeechSynthesisUtterance = MockUtterance;
    (window as unknown as Record<string, unknown>).speechSynthesis = {
      speak: vi.fn((u: MockUtterance) => spoken.push(u)),
      cancel: vi.fn(),
    };
    tts.stop();
  });

  it("speak nastaví speakingId a předá správný jazyk", () => {
    tts.speak("m1", "Ahoj světe", "cs");
    expect(tts.speakingId).toBe("m1");
    expect(spoken).toHaveLength(1);
    expect(spoken[0].lang).toBe("cs-CZ");
  });

  it("opětovné volání se stejným id čtení zastaví (toggle)", () => {
    tts.speak("m1", "Ahoj", "cs");
    tts.speak("m1", "Ahoj", "cs");
    expect(tts.speakingId).toBeNull();
  });

  it("stop() vynuluje speakingId", () => {
    tts.speak("m2", "Něco", "en");
    tts.stop();
    expect(tts.speakingId).toBeNull();
  });
});
