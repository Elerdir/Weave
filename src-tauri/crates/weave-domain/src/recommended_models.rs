use serde::{Deserialize, Serialize};

/// Kolik z volné VRAM smí model maximálně zabrat — zbytek je rezerva pro KV
/// cache, aktivace a další appky (prohlížeč, ComfyUI...).
pub const GPU_VRAM_BUDGET_FRACTION: f64 = 0.8;

/// Doporučí `gpu_layers` podle toho, jestli se celý model (odhadem podle
/// velikosti .gguf souboru — v praxi dobrá aproximace VRAM nároku při plném
/// GPU offloadu) vejde do rozpočtu volné VRAM. Když ne, vrátí 0 (celý model
/// do RAM) — částečný offload, který stejně skončí OOM nebo je nepředvídatelně
/// pomalý, je horší než čisté CPU/RAM řešení.
///
/// `free_vram_mb == 0` znamená neznámou VRAM (např. detekce selhala nebo jde
/// o non-NVIDIA GPU) — chování zůstává beze změny (999 = všechny vrstvy).
pub fn recommend_gpu_layers(model_size_bytes: u64, free_vram_mb: u64) -> u32 {
    if free_vram_mb == 0 {
        return 999;
    }
    let budget_bytes = (free_vram_mb as f64 * 1024.0 * 1024.0 * GPU_VRAM_BUDGET_FRACTION) as u64;
    if model_size_bytes <= budget_bytes {
        999
    } else {
        0
    }
}

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
        RecommendedModel {
            id: "tiger-gemma-12b-v3".into(),
            name: "Tiger Gemma 12B v3".into(),
            description: "Doladěný Gemma 3 12B — silná vícejazyčnost (i čeština), otevřenější \
                i k citlivějším a dospělým tématům. Potřebuje ~9 GB VRAM."
                .into(),
            size_bytes: 7_867_145_696,
            download_url: "https://huggingface.co/TheDrummer/Tiger-Gemma-12B-v3-GGUF/resolve/main/Tiger-Gemma-12B-v3b-Q4_K_M.gguf".into(),
            recommended_gpu_layers: 999,
        },
        RecommendedModel {
            id: "magnum-v4-22b".into(),
            name: "Magnum v4 22B".into(),
            description: "Cílí na kvalitu prózy srovnatelnou s velkými cloudovými modely — \
                tvůrčí psaní a delší příběhy včetně dospělého obsahu. Potřebuje ~14 GB VRAM."
                .into(),
            size_bytes: 13_341_241_824,
            download_url: "https://huggingface.co/anthracite-org/magnum-v4-22b-gguf/resolve/main/magnum-v4-22b-Q4_K_M.gguf".into(),
            recommended_gpu_layers: 999,
        },
        RecommendedModel {
            id: "cydonia-24b-v4.1".into(),
            name: "Cydonia 24B v4.1".into(),
            description: "Založeno na Mistral Small 24B — nejlepší kvalita a vícejazyčnost \
                (i čeština) z nabídky, laděné na roleplay a tvůrčí psaní bez cenzurních \
                omezení. Potřebuje ~16 GB VRAM."
                .into(),
            size_bytes: 14_333_910_048,
            download_url: "https://huggingface.co/TheDrummer/Cydonia-24B-v4.1-GGUF/resolve/main/Cydonia-24B-v4j-Q4_K_M.gguf".into(),
            recommended_gpu_layers: 999,
        },
        RecommendedModel {
            id: "dolphin3.0-mistral-24b".into(),
            name: "Dolphin 3.0 Mistral 24B".into(),
            description: "Založeno na Mistral Small 24B — všestranný model bez vestavěných \
                odmítání, vhodný i pro dospělý obsah. Silná vícejazyčnost (i čeština). \
                Potřebuje ~16 GB VRAM."
                .into(),
            size_bytes: 14_333_925_664,
            download_url: "https://huggingface.co/bartowski/cognitivecomputations_Dolphin3.0-Mistral-24B-GGUF/resolve/main/cognitivecomputations_Dolphin3.0-Mistral-24B-Q4_K_M.gguf".into(),
            recommended_gpu_layers: 999,
        },
        RecommendedModel {
            id: "gemma-2-27b-it".into(),
            name: "Gemma 2 27B Instruct".into(),
            description: "Googlí model se špičkovou vícejazyčností (výborná čeština) a \
                kultivovaným stylem. Vhodný na chat i psaní. Potřebuje ~18 GB VRAM."
                .into(),
            size_bytes: 16_645_381_632,
            download_url: "https://huggingface.co/bartowski/gemma-2-27b-it-GGUF/resolve/main/gemma-2-27b-it-Q4_K_M.gguf".into(),
            recommended_gpu_layers: 999,
        },
        RecommendedModel {
            id: "gemma-3-27b-it".into(),
            name: "Gemma 3 27B Instruct".into(),
            description: "Nejnovější Gemma od Googlu — 128K kontext, špičková vícejazyčnost \
                (výborná čeština) a silné uvažování. Potřebuje ~18 GB VRAM."
                .into(),
            size_bytes: 16_546_688_736,
            download_url: "https://huggingface.co/unsloth/gemma-3-27b-it-GGUF/resolve/main/gemma-3-27b-it-Q4_K_M.gguf".into(),
            recommended_gpu_layers: 999,
        },
        RecommendedModel {
            id: "qwen2.5-32b-instruct".into(),
            name: "Qwen2.5 32B Instruct".into(),
            description: "Vlajkový všestranný model — nejchytřejší z nabídky, skvělý na \
                znalosti, kód i vícejazyčný chat (i čeština). Potřebuje ~22 GB VRAM \
                (sedne na 24GB kartu)."
                .into(),
            size_bytes: 19_851_336_576,
            download_url: "https://huggingface.co/bartowski/Qwen2.5-32B-Instruct-GGUF/resolve/main/Qwen2.5-32B-Instruct-Q4_K_M.gguf".into(),
            recommended_gpu_layers: 999,
        },
        RecommendedModel {
            id: "qwq-32b".into(),
            name: "QwQ 32B (reasoning)".into(),
            description: "Přemýšlivý model — před odpovědí si nahlas rozmyslí postup, takže \
                exceluje v logice, matematice a složitějších úlohách. Odpovídá pomaleji. \
                Potřebuje ~22 GB VRAM."
                .into(),
            size_bytes: 19_851_336_512,
            download_url: "https://huggingface.co/bartowski/Qwen_QwQ-32B-GGUF/resolve/main/Qwen_QwQ-32B-Q4_K_M.gguf".into(),
            recommended_gpu_layers: 999,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recommend_gpu_layers_uses_all_gpu_when_model_fits_budget() {
        // 10 GB volné VRAM * 0.8 = 8 GB rozpočet; 7 GB model se vejde.
        let free_vram_mb = 10 * 1024;
        let model_size = 7 * 1024 * 1024 * 1024u64;
        assert_eq!(recommend_gpu_layers(model_size, free_vram_mb), 999);
    }

    #[test]
    fn recommend_gpu_layers_falls_back_to_ram_when_over_budget() {
        // 10 GB volné VRAM * 0.8 = 8 GB rozpočet; 9 GB model se nevejde.
        let free_vram_mb = 10 * 1024;
        let model_size = 9 * 1024 * 1024 * 1024u64;
        assert_eq!(recommend_gpu_layers(model_size, free_vram_mb), 0);
    }

    #[test]
    fn recommend_gpu_layers_defaults_to_all_when_vram_unknown() {
        assert_eq!(recommend_gpu_layers(50 * 1024 * 1024 * 1024, 0), 999);
    }

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
