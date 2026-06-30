export interface MentionMatch {
  /** Text za @ až po kurzor. */
  query: string;
  /** Index znaku '@'. */
  start: number;
  /** Index kurzoru (konec tokenu). */
  end: number;
}

/**
 * Najde aktivní @zmínku pod kurzorem v textu.
 * Zmínka je `@` na začátku nebo po mezeře, následované znaky bez mezery.
 * Vrátí null, pokud kurzor není uvnitř @tokenu.
 */
export function activeMention(text: string, cursor: number): MentionMatch | null {
  if (cursor < 0 || cursor > text.length) return null;

  let i = cursor - 1;
  while (i >= 0 && !/\s/.test(text[i]) && text[i] !== "@") i--;

  if (i < 0 || text[i] !== "@") return null;
  // '@' musí být na začátku nebo po mezeře
  if (i > 0 && !/\s/.test(text[i - 1])) return null;

  return { query: text.slice(i + 1, cursor), start: i, end: cursor };
}

/** Odebere @token z textu (nahradí rozsah [start,end) prázdným). */
export function removeMentionToken(text: string, match: MentionMatch): string {
  const before = text.slice(0, match.start);
  const after = text.slice(match.end);
  // Sloučíme případné dvojité mezery na hranici
  return (before + after).replace(/ {2,}/g, " ");
}
