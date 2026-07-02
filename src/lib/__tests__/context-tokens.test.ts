import { describe, it, expect } from "vitest";
import {
  estimateTokens,
  estimateConversationTokens,
  contextUsagePercent,
  usageSeverity,
  MESSAGE_OVERHEAD_TOKENS,
} from "$lib/context-tokens";

describe("estimateTokens", () => {
  it("prázdný text = 0 tokenů", () => {
    expect(estimateTokens("")).toBe(0);
  });

  it("odhaduje ~4 znaky na token se zaokrouhlením nahoru", () => {
    expect(estimateTokens("abcd")).toBe(1);
    expect(estimateTokens("abcde")).toBe(2);
    expect(estimateTokens("a".repeat(400))).toBe(100);
  });
});

describe("estimateConversationTokens", () => {
  it("sčítá zprávy včetně režie šablony", () => {
    const messages = [{ content: "a".repeat(40) }, { content: "b".repeat(40) }];
    expect(estimateConversationTokens(messages)).toBe(
      10 + MESSAGE_OVERHEAD_TOKENS + 10 + MESSAGE_OVERHEAD_TOKENS
    );
  });

  it("započítá i právě streamovanou odpověď", () => {
    const base = estimateConversationTokens([{ content: "otázka" }]);
    const withStream = estimateConversationTokens([{ content: "otázka" }], "a".repeat(40));
    expect(withStream).toBe(base + 10 + MESSAGE_OVERHEAD_TOKENS);
  });
});

describe("contextUsagePercent", () => {
  it("počítá procenta a stropuje na 100", () => {
    expect(contextUsagePercent(2048, 8192)).toBe(25);
    expect(contextUsagePercent(10000, 8192)).toBe(100);
    expect(contextUsagePercent(100, 0)).toBe(0);
  });
});

describe("usageSeverity", () => {
  it("mapuje procenta na stupeň", () => {
    expect(usageSeverity(10)).toBe("ok");
    expect(usageSeverity(69)).toBe("ok");
    expect(usageSeverity(70)).toBe("warn");
    expect(usageSeverity(89)).toBe("warn");
    expect(usageSeverity(90)).toBe("danger");
  });
});
