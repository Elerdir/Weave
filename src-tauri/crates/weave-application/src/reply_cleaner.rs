//! Odstranění řídicích tokenů z odpovědi modelu.
//!
//! Lokální modely (Gemma, Qwen, gpt-oss…) občas prosáknou vlastní šablonové
//! tokeny do textu odpovědi — typicky hned na začátku, např.
//! `<|channel><|channel>thought` nebo `<start_of_turn>model`. Uživatele to
//! nezajímá a v markdownu to vypadá jako rozbitá zpráva.
//!
//! Čistí se ve streamu, takže se musí počítat s tím, že jeden token modelu
//! může přijít rozsekaný na několik kousků (`<|cha` + `nnel|>`). Rozdělaný
//! začátek tokenu proto zůstává v bufferu, dokud není jasné, jestli jde
//! o řídicí token nebo obyčejný text.

/// Nejdelší řídicí token, který ještě rozpoznáváme. Delší `<…>` sekvence
/// bereme jako běžný text (typicky HTML nebo matematika).
const MAX_TOKEN_LEN: usize = 40;

/// Řídicí tokeny bez svislítek. S `|` uvnitř se pozná cokoli (HTML značka
/// svislítko nikdy neobsahuje), tady je potřeba výčet.
const NAMED_TOKENS: &[&str] = &[
    "bos",
    "eos",
    "pad",
    "unk",
    "s",
    "/s",
    "start_of_turn",
    "end_of_turn",
    "end_of_text",
    "end_of_utterance",
];

/// Slova, která po odstranění řídicích tokenů zbydou na začátku odpovědi
/// jako smetí — názvy kanálů (`thought`, `final`) a role (`model`,
/// `assistant`). Zahazují se jen tehdy, když tvoří celý první řádek.
const PREFIX_NOISE: &[&str] = &[
    "thought",
    "thoughts",
    "thinking",
    "analysis",
    "commentary",
    "final",
    "assistant",
    "model",
    "###",
    "##",
    "#",
];

/// Dokud nic neodešlo, čeká se na tolik znaků (nebo na konec řádku), aby
/// šlo vyhodnotit smetí na začátku. Bez toho by se z „thought" stihlo
/// odeslat „tho" dřív, než je jasné, o co jde.
const PREFIX_LOOKAHEAD: usize = 24;

/// Průběžná čistička odpovědi. Vytváří se jedna na zprávu.
#[derive(Debug, Default)]
pub struct ReplyCleaner {
    buffer: String,
    emitted: bool,
}

impl ReplyCleaner {
    pub fn new() -> Self {
        Self::default()
    }

    /// Přidá kousek streamu a vrátí text, který je bezpečné poslat dál
    /// (může být prázdný, když se čeká na dokončení tokenu).
    pub fn push(&mut self, chunk: &str) -> String {
        self.buffer.push_str(chunk);
        self.take(false)
    }

    /// Dovyprázdní buffer na konci odpovědi. Idempotentní.
    pub fn finish(&mut self) -> String {
        self.take(true)
    }

    fn take(&mut self, final_flush: bool) -> String {
        self.buffer = remove_control_tokens(&self.buffer);

        // Rozdělaný token na konci si necháme na příště — jinak by se
        // `<|cha` odeslalo jako text a druhá půlka by dorazila zvlášť.
        let split = if final_flush {
            self.buffer.len()
        } else {
            pending_token_start(&self.buffer).unwrap_or(self.buffer.len())
        };

        if !self.emitted {
            let head = &self.buffer[..split];
            // Na začátku počkáme na celý první řádek, aby šlo poznat smetí.
            if !final_flush && !head.contains('\n') && head.chars().count() < PREFIX_LOOKAHEAD {
                return String::new();
            }
            let cleaned = strip_prefix_noise(head).to_string();
            self.buffer = self.buffer.split_off(split);
            if !cleaned.is_empty() {
                self.emitted = true;
            }
            return cleaned;
        }

        let rest = self.buffer.split_off(split);
        std::mem::replace(&mut self.buffer, rest)
    }
}

/// Vyhodí všechny kompletní řídicí tokeny z textu.
fn remove_control_tokens(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;

    while let Some(start) = rest.find('<') {
        out.push_str(&rest[..start]);
        let candidate = &rest[start..];
        match control_token_len(candidate) {
            Some(len) => rest = &candidate[len..],
            None => {
                out.push('<');
                rest = &candidate[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Délka řídicího tokenu na začátku `text` (včetně `<` a `>`), pokud tam
/// nějaký je. Za řídicí token se považuje `<…>` obsahující svislítko
/// (`<|im_start|>`, `<|channel>`, `<channel|>`) nebo známé jméno (`<eos>`).
fn control_token_len(text: &str) -> Option<usize> {
    let end = text
        .char_indices()
        .take(MAX_TOKEN_LEN)
        .find(|(_, c)| *c == '>')
        .map(|(i, _)| i + 1)?;
    let inner = &text[1..end - 1];
    if inner.contains('|') || NAMED_TOKENS.iter().any(|t| inner.eq_ignore_ascii_case(t)) {
        Some(end)
    } else {
        None
    }
}

/// Index rozdělaného řídicího tokenu na konci textu, od kterého se má
/// zbytek podržet. `None` = celý text jde odeslat.
fn pending_token_start(text: &str) -> Option<usize> {
    let start = text.rfind('<')?;
    let tail = &text[start..];
    // Uzavřený `<…>` už zpracoval remove_control_tokens, tohle je běžný text.
    if tail.contains('>') {
        return None;
    }
    // Dlouhý „otevřený" text není token, ale normální věta („5 < 10 a dál…").
    if tail.chars().count() > MAX_TOKEN_LEN {
        return None;
    }
    let looks_like_token = tail[1..]
        .chars()
        .all(|c| c == '|' || c == '_' || c == '/' || c.is_ascii_alphanumeric());
    looks_like_token.then_some(start)
}

/// Osekne smetí na začátku odpovědi: bílé znaky a řádky tvořené jen názvem
/// kanálu, rolí nebo osiřelou markdown mřížkou.
fn strip_prefix_noise(text: &str) -> &str {
    let mut rest = text.trim_start();
    loop {
        let line_end = rest.find('\n').unwrap_or(rest.len());
        let first_line = rest[..line_end].trim();
        if first_line.is_empty() && line_end < rest.len() {
            rest = rest[line_end + 1..].trim_start();
            continue;
        }
        if PREFIX_NOISE
            .iter()
            .any(|noise| first_line.eq_ignore_ascii_case(noise))
        {
            rest = if line_end < rest.len() {
                rest[line_end + 1..].trim_start()
            } else {
                ""
            };
            continue;
        }
        return rest;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Prožene text čističkou po jednom znaku — simuluje stream, kde token
    /// modelu přichází rozsekaný.
    fn clean_char_by_char(input: &str) -> String {
        let mut cleaner = ReplyCleaner::new();
        let mut out = String::new();
        for ch in input.chars() {
            out.push_str(&cleaner.push(&ch.to_string()));
        }
        out.push_str(&cleaner.finish());
        out
    }

    fn clean_at_once(input: &str) -> String {
        let mut cleaner = ReplyCleaner::new();
        let mut out = cleaner.push(input);
        out.push_str(&cleaner.finish());
        out
    }

    #[test]
    fn removes_gemma_channel_preamble_reported_by_user() {
        // Přesně to, co se objevovalo na začátku každé odpovědi.
        let raw = "<|channel><|channel>thought\n\n<|channel><channel|>###\n\nAhoj, jak se máš?";
        assert_eq!(clean_at_once(raw), "Ahoj, jak se máš?");
        assert_eq!(clean_char_by_char(raw), "Ahoj, jak se máš?");
    }

    #[test]
    fn removes_gemma_start_of_turn_role() {
        let raw = "<start_of_turn>model\nDobrý den.<end_of_turn>";
        assert_eq!(clean_at_once(raw), "Dobrý den.");
    }

    #[test]
    fn removes_chatml_tokens_anywhere_in_text() {
        let raw = "První věta.<|im_end|>\n<|im_start|>assistant\nDruhá věta.";
        let cleaned = clean_at_once(raw);
        assert!(cleaned.starts_with("První věta."));
        assert!(!cleaned.contains("<|"));
        assert!(cleaned.contains("Druhá věta."));
    }

    #[test]
    fn keeps_real_markdown_headings_and_content() {
        let raw = "### Nadpis\n\nText odpovědi s **tučným** písmem.";
        assert_eq!(clean_at_once(raw), raw);
        assert_eq!(clean_char_by_char(raw), raw);
    }

    #[test]
    fn keeps_generated_image_markdown_untouched() {
        // Obrázková větev posílá odkaz jedním kusem — nesmí se poškodit.
        let raw = "![obrázek](C:/weave/gallery/img_1.png)";
        assert_eq!(clean_at_once(raw), raw);
    }

    #[test]
    fn keeps_plain_text_with_angle_brackets() {
        let raw = "Platí, že 5 < 10 a zároveň 10 > 5.";
        assert_eq!(clean_at_once(raw), raw);
        assert_eq!(clean_char_by_char(raw), raw);
    }

    #[test]
    fn keeps_html_like_tags_without_pipes() {
        // `<div>` není řídicí token — nesmí zmizet z ukázky kódu.
        let raw = "Použij <div> a <span> v HTML.";
        assert_eq!(clean_at_once(raw), raw);
    }

    #[test]
    fn split_control_token_across_chunks_is_not_leaked() {
        let mut cleaner = ReplyCleaner::new();
        let mut out = cleaner.push("Ahoj<|cha");
        out.push_str(&cleaner.push("nnel|>světe"));
        out.push_str(&cleaner.finish());
        assert_eq!(out, "Ahoj světe".replace(' ', ""));
    }

    #[test]
    fn unfinished_token_at_stream_end_is_flushed_as_text() {
        // Když stream skončí uprostřed „<|", nesmí se text ztratit.
        let mut cleaner = ReplyCleaner::new();
        let mut out = cleaner.push("Konec textu<|");
        out.push_str(&cleaner.finish());
        assert_eq!(out, "Konec textu<|");
    }

    #[test]
    fn prefix_noise_is_stripped_only_at_the_beginning() {
        // „final" uprostřed odpovědi je normální slovo, ne název kanálu.
        let raw = "První řádek\nfinal\nDruhý řádek";
        assert_eq!(clean_at_once(raw), raw);
    }

    #[test]
    fn empty_and_whitespace_only_streams_stay_empty() {
        assert_eq!(clean_at_once(""), "");
        assert_eq!(clean_at_once("   \n\n  "), "");
        assert_eq!(clean_at_once("<|channel|>"), "");
    }

    #[test]
    fn finish_is_idempotent() {
        let mut cleaner = ReplyCleaner::new();
        let first = cleaner.push("Text odpovědi");
        let second = cleaner.finish();
        assert_eq!(format!("{first}{second}"), "Text odpovědi");
        assert_eq!(cleaner.finish(), "");
    }

    #[test]
    fn long_answer_streams_without_holding_text_back() {
        // Regrese: čekání na první řádek nesmí pozdržet zbytek odpovědi.
        let mut cleaner = ReplyCleaner::new();
        let first = cleaner.push("Tohle je dostatečně dlouhý začátek odpovědi, ");
        assert!(!first.is_empty(), "první chunk se měl rovnou odeslat");
        assert_eq!(cleaner.push("pokračování."), "pokračování.");
    }

    #[test]
    fn multibyte_characters_survive_chunk_boundaries() {
        let raw = "Příliš žluťoučký kůň úpěl ďábelské ódy.";
        assert_eq!(clean_char_by_char(raw), raw);
    }
}
