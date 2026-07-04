use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Intent {
    TextChat,
    StoryWriting,
    ImageGeneration,
    FileAnalysis,
    CodeAssistance,
    Reasoning,
}

/// Jednoduchý klasifikátor záměru uživatele z textu.
/// Používá heuristiky (regex / klíčová slova) — žádný API call.
pub struct IntentClassifier;

impl IntentClassifier {
    pub fn classify(text: &str) -> Intent {
        let lower = text.to_lowercase();

        if Self::matches_image(&lower) {
            return Intent::ImageGeneration;
        }
        if Self::matches_code(&lower) {
            return Intent::CodeAssistance;
        }
        if Self::matches_reasoning(&lower) {
            return Intent::Reasoning;
        }
        if Self::matches_story(&lower) {
            return Intent::StoryWriting;
        }
        Intent::TextChat
    }

    fn matches_image(text: &str) -> bool {
        // Explicitní žádost o obrázek (verb/podstatné jméno, cz+en). „obrázek",
        // „vygeneruj", „fotka" apod. samostatně — v této appce prakticky vždy
        // znamenají generování obrázku, ne text.
        const TRIGGERS: &[&str] = &[
            "nakresli",
            "namaluj",
            "vygeneruj",
            "vygenerovat",
            "generuj",
            "vytvoř obráz",
            "vytvoř mi obráz",
            "vyfoť",
            "vyobraz",
            "obrázek",
            "obrázku",
            "obrázok",
            "fotka",
            "fotku",
            "fotografi",
            "portrét",
            "generate image",
            "generate an image",
            "create image",
            "make an image",
            "draw",
            "paint",
            "ilustruj",
            "foto",
            "image of",
            "picture of",
            "photo of",
            "render",
            "visualize",
            "portrait",
        ];
        if TRIGGERS.iter().any(|kw| text.contains(kw)) {
            return true;
        }

        // Rozpoznání „hotového" Stable Diffusion promptu — komma-separovaný
        // výčet vizuálních deskriptorů (uživatel často vloží přímo anglický
        // prompt bez slovesa „vygeneruj"). Dva a víc signálů = obrázek.
        const SD_HINTS: &[&str] = &[
            "photorealistic",
            "full body",
            "full length",
            "highly detailed",
            "sharp focus",
            "cinematic",
            "bokeh",
            "8k",
            "dslr",
            "studio lighting",
            "natural lighting",
            "photography",
            "realistic skin",
            "head to toe",
        ];
        SD_HINTS.iter().filter(|kw| text.contains(**kw)).count() >= 2
    }

    fn matches_code(text: &str) -> bool {
        let keywords = [
            "napiš kód",
            "write code",
            "implement",
            "debug",
            "fix bug",
            "funkce",
            "function",
            "třída",
            "class",
            "algorithm",
            "algoritmus",
            "script",
            "program",
        ];
        keywords.iter().any(|kw| text.contains(kw))
    }

    fn matches_reasoning(text: &str) -> bool {
        let keywords = [
            "vyřeš",
            "solve",
            "dokař",
            "prove",
            "matematik",
            "math",
            "logick",
            "reasoning",
            "analyz",
            "analyze",
            "porovnej",
            "compare",
        ];
        keywords.iter().any(|kw| text.contains(kw))
    }

    fn matches_story(text: &str) -> bool {
        let keywords = [
            "napiš příběh",
            "write story",
            "pokračuj v příběhu",
            "continue story",
            "povídka",
            "román",
            "fiction",
            "fantasy",
            "scifi",
            "sci-fi",
            "postava",
            "character",
            "dialóg",
            "dialogue",
        ];
        keywords.iter().any(|kw| text.contains(kw))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub provider: ModelProvider,
    pub context_length: u32,
    pub supports_vision: bool,
    pub supports_tools: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelProvider {
    MistralApi,
    Local,
    ComfyUi,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_image_request() {
        assert_eq!(
            IntentClassifier::classify("nakresli mi kočku na měsíci"),
            Intent::ImageGeneration
        );
        assert_eq!(
            IntentClassifier::classify("generate image of a sunset"),
            Intent::ImageGeneration
        );
    }

    #[test]
    fn classifies_code_request() {
        assert_eq!(
            IntentClassifier::classify("napiš kód pro quicksort v Rustu"),
            Intent::CodeAssistance
        );
    }

    #[test]
    fn classifies_story_request() {
        assert_eq!(
            IntentClassifier::classify("napiš příběh o rytíři"),
            Intent::StoryWriting
        );
    }

    #[test]
    fn defaults_to_text_chat() {
        assert_eq!(IntentClassifier::classify("jak se máš?"), Intent::TextChat);
    }

    #[test]
    fn classifies_loose_and_pasted_image_requests() {
        // Volnější česká formulace (dřív spadla do textu → cenzurované odmítnutí)
        assert_eq!(
            IntentClassifier::classify("vygeneruj mi obrázek nahé ženy v lese"),
            Intent::ImageGeneration
        );
        // Rovnou vložený anglický SD prompt bez slovesa „vygeneruj"
        assert_eq!(
            IntentClassifier::classify(
                "a young woman, full body, photorealistic, natural lighting, highly detailed"
            ),
            Intent::ImageGeneration
        );
    }
}
