use serde::{Deserialize, Serialize};

/// Parametry generování specifické pro jednu konverzaci.
/// `None` = použij globální výchozí hodnotu (nastavení appky / chování API).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GenerationSettings {
    /// Kontextové okno v tokenech (jen vestavěná inference).
    pub context_length: Option<u32>,
    /// Teplota vzorkování (0.0 = deterministické, ~2.0 = velmi kreativní).
    pub temperature: Option<f32>,
    /// Strop délky odpovědi v tokenech. `None` = bez umělého omezení.
    pub max_tokens: Option<u32>,
}

impl GenerationSettings {
    /// Efektivní teplota s výchozí hodnotou aplikace.
    pub fn temperature_or_default(&self) -> f32 {
        self.temperature.unwrap_or(0.7)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_all_unset() {
        let s = GenerationSettings::default();
        assert!(s.context_length.is_none());
        assert!(s.temperature.is_none());
        assert!(s.max_tokens.is_none());
        assert_eq!(s.temperature_or_default(), 0.7);
    }

    #[test]
    fn explicit_temperature_wins_over_default() {
        let s = GenerationSettings {
            temperature: Some(1.4),
            ..Default::default()
        };
        assert_eq!(s.temperature_or_default(), 1.4);
    }
}
