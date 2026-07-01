import type { Locale } from "$lib/i18n/index.svelte";

const LANG_MAP: Record<Locale, string> = {
  cs: "cs-CZ",
  en: "en-US",
};

/** Očistí markdown na čitelný text pro předčítání. */
export function stripForSpeech(text: string): string {
  return text
    .replace(/```[\s\S]*?```/g, " (blok kódu) ") // code bloky
    .replace(/`([^`]+)`/g, "$1") // inline kód
    .replace(/!\[[^\]]*\]\([^)]*\)/g, " (obrázek) ") // obrázky
    .replace(/\[([^\]]+)\]\([^)]*\)/g, "$1") // odkazy → text
    .replace(/[*_#>]/g, "") // markdown značky
    .replace(/\s+/g, " ")
    .trim();
}

function isSupported(): boolean {
  return typeof window !== "undefined" && "speechSynthesis" in window;
}

function createTtsStore() {
  let speakingId = $state<string | null>(null);

  return {
    get speakingId() {
      return speakingId;
    },
    get supported() {
      return isSupported();
    },

    /** Přečte text; opětovné volání se stejným id čtení zastaví (toggle). */
    speak(id: string, text: string, locale: Locale) {
      if (!isSupported()) return;

      const synth = window.speechSynthesis;
      synth.cancel();

      if (speakingId === id) {
        speakingId = null;
        return;
      }

      const utterance = new SpeechSynthesisUtterance(stripForSpeech(text));
      utterance.lang = LANG_MAP[locale] ?? "en-US";
      utterance.onend = () => {
        if (speakingId === id) speakingId = null;
      };
      utterance.onerror = () => {
        if (speakingId === id) speakingId = null;
      };

      speakingId = id;
      synth.speak(utterance);
    },

    stop() {
      if (isSupported()) window.speechSynthesis.cancel();
      speakingId = null;
    },
  };
}

export const tts = createTtsStore();
