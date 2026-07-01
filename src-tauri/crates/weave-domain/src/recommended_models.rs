use serde::{Deserialize, Serialize};

/// Doporučený model k jednoklikovému stažení pro vestavěnou GPU inferenci.
/// URL vede přímo na .gguf soubor na veřejně dostupném HuggingFace repu
/// (bez nutnosti přihlášení/tokenu).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecommendedModel {
    pub id: String,
    pub name: String,
    pub description: String,
    pub size_bytes: u64,
    pub download_url: String,
    /// Doporučený počet vrstev na GPU (999 = všechny).
    pub recommended_gpu_layers: u32,
}

pub fn recommended_models() -> Vec<RecommendedModel> {
    vec![
        RecommendedModel {
            id: "qwen2.5-1.5b-instruct".into(),
            name: "Qwen2.5 1.5B Instruct".into(),
            description: "Nejrychlejší start — malý, ale schopný model. Vhodný i na slabší GPU."
                .into(),
            size_bytes: 1_117_320_736,
            download_url: "https://huggingface.co/Qwen/Qwen2.5-1.5B-Instruct-GGUF/resolve/main/qwen2.5-1.5b-instruct-q4_k_m.gguf".into(),
            recommended_gpu_layers: 999,
        },
        RecommendedModel {
            id: "qwen2.5-3b-instruct".into(),
            name: "Qwen2.5 3B Instruct".into(),
            description: "Vyvážený poměr rychlosti a kvality pro běžný chat.".into(),
            size_bytes: 2_104_932_768,
            download_url: "https://huggingface.co/Qwen/Qwen2.5-3B-Instruct-GGUF/resolve/main/qwen2.5-3b-instruct-q4_k_m.gguf".into(),
            recommended_gpu_layers: 999,
        },
        RecommendedModel {
            id: "mistral-7b-instruct-v0.3".into(),
            name: "Mistral 7B Instruct v0.3".into(),
            description: "Nejkvalitnější odpovědi z nabídky — potřebuje víc VRAM (~6 GB)."
                .into(),
            size_bytes: 4_372_812_000,
            download_url: "https://huggingface.co/bartowski/Mistral-7B-Instruct-v0.3-GGUF/resolve/main/Mistral-7B-Instruct-v0.3-Q4_K_M.gguf".into(),
            recommended_gpu_layers: 999,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_recommended_models_have_valid_data() {
        let models = recommended_models();
        assert!(!models.is_empty());
        for m in &models {
            assert!(!m.id.is_empty());
            assert!(!m.name.is_empty());
            assert!(m.download_url.starts_with("https://"));
            assert!(m.download_url.ends_with(".gguf"));
            assert!(m.size_bytes > 0);
        }
    }

    #[test]
    fn ids_are_unique() {
        let models = recommended_models();
        let mut ids: Vec<&str> = models.iter().map(|m| m.id.as_str()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), models.len());
    }
}
