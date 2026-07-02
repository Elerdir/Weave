/**
 * Odhad spotřeby kontextového okna. Bez načteného modelu nejde tokenizovat
 * přesně — používáme heuristiku ~4 znaky na token (funguje rozumně pro
 * češtinu i angličtinu u Mistral/LLama tokenizérů) plus režii chat šablony
 * na každou zprávu. UI hodnotu značí vlnovkou jako odhad.
 */

/** Režie ChatML šablony na jednu zprávu (<|im_start|>role … <|im_end|>). */
export const MESSAGE_OVERHEAD_TOKENS = 8;

const CHARS_PER_TOKEN = 4;

export function estimateTokens(text: string): number {
  if (!text) return 0;
  return Math.ceil(text.length / CHARS_PER_TOKEN);
}

export interface TokenizableMessage {
  content: string;
}

/** Odhad tokenů celé historie vč. právě streamované odpovědi. */
export function estimateConversationTokens(
  messages: TokenizableMessage[],
  streamingContent: string | null = null
): number {
  let total = 0;
  for (const msg of messages) {
    total += estimateTokens(msg.content) + MESSAGE_OVERHEAD_TOKENS;
  }
  if (streamingContent) {
    total += estimateTokens(streamingContent) + MESSAGE_OVERHEAD_TOKENS;
  }
  return total;
}

/** Vyčerpání okna v procentech (0–100, zastropováno). */
export function contextUsagePercent(usedTokens: number, contextLength: number): number {
  if (contextLength <= 0) return 0;
  return Math.min(100, Math.round((usedTokens / contextLength) * 100));
}

/** Stupeň zaplnění pro barvu ukazatele. */
export function usageSeverity(percent: number): "ok" | "warn" | "danger" {
  if (percent >= 90) return "danger";
  if (percent >= 70) return "warn";
  return "ok";
}
