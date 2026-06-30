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
        let keywords = [
            "nakresli",
            "vygeneruj obrázek",
            "vytvoř obrázek",
            "generate image",
            "draw",
            "paint",
            "ilustruj",
            "foto",
            "fotografie",
            "image of",
            "picture of",
            "render",
            "visualize",
        ];
        keywords.iter().any(|kw| text.contains(kw))
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
}
