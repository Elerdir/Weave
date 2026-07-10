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
    /// Síla PuLID identity (ApplyPulid `weight`) při generování podle
    /// referenční fotky. Vyšší = věrnější podoba, ale méně prostoru pro
    /// prompt (přeučení, artefakty). `None` = výchozí 1.0.
    pub pulid_weight: Option<f32>,
    /// Doladit obličej/oči druhým průchodem FaceDetailer (ComfyUI Impact Pack).
    /// Vyžaduje doinstalování Impact Packu; `None`/`Some(false)` = vypnuto.
    pub face_detailer: Option<bool>,
    /// Per-chat override LLM runtime. `None` nebo `default` = globalni nastaveni.
    pub runtime_backend: Option<String>,
    /// Checkpoint pro generování obrázků (název souboru v ComfyUI
    /// `models/checkpoints`, např. stažený z CivitAI). `None`/prázdný =
    /// automatická volba podle stylu promptu (RealVisXL/Pony/SDXL).
    pub image_checkpoint: Option<String>,
    /// LoRA pro generování obrázků (název souboru v ComfyUI `models/loras`).
    /// `None`/prázdný = automatické vyhledání na CivitAI podle promptu.
    /// Trigger words si musí uživatel napsat do promptu sám.
    pub image_lora: Option<String>,
}

impl GenerationSettings {
    /// Efektivní teplota s výchozí hodnotou aplikace.
    pub fn temperature_or_default(&self) -> f32 {
        self.temperature.unwrap_or(0.7)
    }

    /// Efektivní síla PuLID (výchozí 1.0 = jako v ukázkových workflow PuLID).
    pub fn pulid_weight_or_default(&self) -> f32 {
        self.pulid_weight.unwrap_or(1.0)
    }

    /// Je zapnuté doladění obličeje FaceDetailerem?
    pub fn face_detailer_enabled(&self) -> bool {
        self.face_detailer.unwrap_or(false)
    }

    /// Vybraný checkpoint obrázků, pokud je nastavený a neprázdný.
    pub fn image_checkpoint(&self) -> Option<&str> {
        self.image_checkpoint
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
    }

    /// Vybraná LoRA obrázků, pokud je nastavená a neprázdná.
    pub fn image_lora(&self) -> Option<&str> {
        self.image_lora
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
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
        assert!(s.pulid_weight.is_none());
        assert!(s.face_detailer.is_none());
        assert!(s.runtime_backend.is_none());
        assert!(s.image_checkpoint.is_none());
        assert!(s.image_lora.is_none());
        assert!(s.image_lora().is_none());
        assert_eq!(s.temperature_or_default(), 0.7);
        assert_eq!(s.pulid_weight_or_default(), 1.0);
        assert!(!s.face_detailer_enabled());
        assert!(s.image_checkpoint().is_none());
    }

    #[test]
    fn image_checkpoint_ignores_blank_values() {
        let mut s = GenerationSettings {
            image_checkpoint: Some("  ".into()),
            ..Default::default()
        };
        assert!(s.image_checkpoint().is_none());
        s.image_checkpoint = Some(" custom.safetensors ".into());
        assert_eq!(s.image_checkpoint(), Some("custom.safetensors"));
    }

    #[test]
    fn explicit_temperature_wins_over_default() {
        let s = GenerationSettings {
            temperature: Some(1.4),
            ..Default::default()
        };
        assert_eq!(s.temperature_or_default(), 1.4);
    }

    #[test]
    fn explicit_image_fidelity_fields_win_over_defaults() {
        let s = GenerationSettings {
            pulid_weight: Some(0.75),
            face_detailer: Some(true),
            runtime_backend: Some("embedded".to_string()),
            ..Default::default()
        };
        assert_eq!(s.pulid_weight_or_default(), 0.75);
        assert!(s.face_detailer_enabled());
        assert_eq!(s.runtime_backend.as_deref(), Some("embedded"));
    }
}
