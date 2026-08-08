use serde::{Deserialize, Serialize};

// Dřív tu bylo `recommend_gpu_layers`: buď všechny vrstvy na GPU, nebo (když
// se model nevešel do 80 % volné VRAM) celý model do RAM. To je u MoE modelů
// špatně v obou směrech — celý do RAM zahodí zrychlení, které jde dostat tím,
// že se na GPU nechá attention a v RAM jen experti. Rozhodování se přesunulo
// do `weave_infrastructure::llm::offload_plan`, kde je k dispozici i GGUF
// hlavička (počet expertů, vrstev, rozměry pro odhad KV cache).

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
            description: "Nejrychlejsi start — maly, ale schopny model. Rozumi cesky jen zakladne."
                .into(),
            size_bytes: 1_117_320_736,
            download_url: "https://huggingface.co/Qwen/Qwen2.5-1.5B-Instruct-GGUF/resolve/main/qwen2.5-1.5b-instruct-q4_k_m.gguf".into(),
            recommended_gpu_layers: 999,
        },
        RecommendedModel {
            id: "qwen2.5-3b-instruct".into(),
            name: "Qwen2.5 3B Instruct".into(),
            description: "Vyvazeny pomer rychlosti a kvality pro bezny chat."
                .into(),
            size_bytes: 2_104_932_768,
            download_url: "https://huggingface.co/Qwen/Qwen2.5-3B-Instruct-GGUF/resolve/main/qwen2.5-3b-instruct-q4_k_m.gguf".into(),
            recommended_gpu_layers: 999,
        },
        RecommendedModel {
            id: "mistral-7b-instruct-v0.3".into(),
            name: "Mistral 7B Instruct v0.3".into(),
            description: "Solidni vseobecny model, svizny i na slabsich sestavach."
                .into(),
            size_bytes: 4_372_812_000,
            download_url: "https://huggingface.co/bartowski/Mistral-7B-Instruct-v0.3-GGUF/resolve/main/Mistral-7B-Instruct-v0.3-Q4_K_M.gguf".into(),
            recommended_gpu_layers: 999,
        },
        RecommendedModel {
            id: "gemma-3-4b-it".into(),
            name: "Gemma 3 4B Instruct".into(),
            description: "Lehka Gemma — dobra cestina a dlouhy kontext za malo pameti."
                .into(),
            size_bytes: 2_560_000_000,
            download_url: "https://huggingface.co/unsloth/gemma-3-4b-it-GGUF/resolve/main/gemma-3-4b-it-Q4_K_M.gguf".into(),
            recommended_gpu_layers: 999,
        },
        RecommendedModel {
            id: "gemma-4-e4b-it".into(),
            name: "Gemma 4 E4B Instruct".into(),
            description: "Gemma 4 v male velikosti — velmi dobry pomer kvality a rychlosti."
                .into(),
            size_bytes: 4_980_000_000,
            download_url: "https://huggingface.co/unsloth/gemma-4-E4B-it-GGUF/resolve/main/gemma-4-E4B-it-Q4_K_M.gguf".into(),
            recommended_gpu_layers: 999,
        },
        RecommendedModel {
            id: "gemma-3-12b-it".into(),
            name: "Gemma 3 12B Instruct".into(),
            description: "Stredne velka Gemma 3 — kvalitnejsi psani a vicejazycnost nez 4B."
                .into(),
            size_bytes: 7_300_000_000,
            download_url: "https://huggingface.co/unsloth/gemma-3-12b-it-GGUF/resolve/main/gemma-3-12b-it-Q4_K_M.gguf".into(),
            recommended_gpu_layers: 999,
        },
        RecommendedModel {
            id: "tiger-gemma-12b-v3".into(),
            name: "Tiger Gemma 12B v3".into(),
            description: "Doladena Gemma 3 12B — silna vicejazycnost vcetne cestiny, otevrenejsi i k dospelym tematum."
                .into(),
            size_bytes: 7_867_145_696,
            download_url: "https://huggingface.co/TheDrummer/Tiger-Gemma-12B-v3-GGUF/resolve/main/Tiger-Gemma-12B-v3b-Q4_K_M.gguf".into(),
            recommended_gpu_layers: 999,
        },
        RecommendedModel {
            id: "magnum-v4-22b".into(),
            name: "Magnum v4 22B".into(),
            description: "Cili na kvalitu prozy srovnatelnou s velkymi cloudovymi modely — tvurci psani a delsi pribehy vcetne dospeleho obsahu."
                .into(),
            size_bytes: 13_341_241_824,
            download_url: "https://huggingface.co/anthracite-org/magnum-v4-22b-gguf/resolve/main/magnum-v4-22b-Q4_K_M.gguf".into(),
            recommended_gpu_layers: 999,
        },
        RecommendedModel {
            id: "cydonia-24b-v4.1".into(),
            name: "Cydonia 24B v4.1".into(),
            description: "Zalozeno na Mistral Small 24B — vyborna kvalita a vicejazycnost, ladene na roleplay a tvurci psani bez cenzurnich omezeni."
                .into(),
            size_bytes: 14_333_910_048,
            download_url: "https://huggingface.co/TheDrummer/Cydonia-24B-v4.1-GGUF/resolve/main/Cydonia-24B-v4j-Q4_K_M.gguf".into(),
            recommended_gpu_layers: 999,
        },
        RecommendedModel {
            id: "dolphin3.0-mistral-24b".into(),
            name: "Dolphin 3.0 Mistral 24B".into(),
            description: "Zalozeno na Mistral Small 24B — vsestranny model bez vestavenych odmitani. Silna vicejazycnost vcetne cestiny."
                .into(),
            size_bytes: 14_333_925_664,
            download_url: "https://huggingface.co/bartowski/cognitivecomputations_Dolphin3.0-Mistral-24B-GGUF/resolve/main/cognitivecomputations_Dolphin3.0-Mistral-24B-Q4_K_M.gguf".into(),
            recommended_gpu_layers: 999,
        },
        RecommendedModel {
            id: "gemma-2-27b-it".into(),
            name: "Gemma 2 27B Instruct".into(),
            description: "Googli model se spickovou vicejazycnosti (vyborna cestina) a kultivovanym stylem."
                .into(),
            size_bytes: 16_645_381_632,
            download_url: "https://huggingface.co/bartowski/gemma-2-27b-it-GGUF/resolve/main/gemma-2-27b-it-Q4_K_M.gguf".into(),
            recommended_gpu_layers: 999,
        },
        RecommendedModel {
            id: "gemma-3-27b-it".into(),
            name: "Gemma 3 27B Instruct".into(),
            description: "Gemma 3 v nejvetsi velikosti — 128K kontext, spickova cestina a silne uvazovani."
                .into(),
            size_bytes: 16_546_688_736,
            download_url: "https://huggingface.co/unsloth/gemma-3-27b-it-GGUF/resolve/main/gemma-3-27b-it-Q4_K_M.gguf".into(),
            recommended_gpu_layers: 999,
        },
        RecommendedModel {
            id: "gemma-4-26b-a4b-it".into(),
            name: "Gemma 4 26B-A4B Instruct".into(),
            description: "Gemma 4 MoE — na token pracuje jen zlomek parametru, takze bezi svizne i pres svoji velikost, ale ma kvalitu vetsi rodiny. Vyborna cestina."
                .into(),
            size_bytes: 16_947_541_728,
            download_url: "https://huggingface.co/unsloth/gemma-4-26B-A4B-it-GGUF/resolve/main/gemma-4-26B-A4B-it-UD-Q4_K_M.gguf".into(),
            recommended_gpu_layers: 999,
        },
        RecommendedModel {
            id: "gemma-4-26b-a4b-it-uncensored".into(),
            name: "Gemma 4 26B-A4B Instruct (necenzurovana)".into(),
            description: "Tataz Gemma 4 26B MoE, jen bez vestavenych odmitani. Abliterace nechala puvodni vahy vcetne cestiny prakticky nedotcene, takze kvalita i rychlost odpovidaji zakladni verzi."
                .into(),
            size_bytes: 16_796_011_072,
            download_url: "https://huggingface.co/TrevorJS/gemma-4-26B-A4B-it-uncensored-GGUF/resolve/main/gemma-4-26B-A4B-it-uncensored-Q4_K_M.gguf".into(),
            recommended_gpu_layers: 999,
        },
        RecommendedModel {
            id: "gemma-4-31b-it".into(),
            name: "Gemma 4 31B Instruct".into(),
            description: "Nejsilnejsi Gemma 4 — vyborny chat, psani i vicejazycnost. Husty model, takze narocnejsi na pamet nez 26B MoE."
                .into(),
            size_bytes: 18_300_000_000,
            download_url: "https://huggingface.co/unsloth/gemma-4-31B-it-GGUF/resolve/main/gemma-4-31B-it-Q4_K_M.gguf".into(),
            recommended_gpu_layers: 999,
        },
        RecommendedModel {
            id: "qwen2.5-32b-instruct".into(),
            name: "Qwen2.5 32B Instruct".into(),
            description: "Vlajkovy vseobecny model — skvely na znalosti, kod i vicejazycny chat vcetne cestiny."
                .into(),
            size_bytes: 19_851_336_576,
            download_url: "https://huggingface.co/bartowski/Qwen2.5-32B-Instruct-GGUF/resolve/main/Qwen2.5-32B-Instruct-Q4_K_M.gguf".into(),
            recommended_gpu_layers: 999,
        },
        RecommendedModel {
            id: "qwq-32b".into(),
            name: "QwQ 32B (reasoning)".into(),
            description: "Premyslivy model — pred odpovedi si nahlas rozmysli postup, takze exceluje v logice a matematice. Odpovida pomaleji."
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

    #[test]
    fn catalog_offers_gemma4_26b_in_both_variants() {
        // Uživatel chce mít po ruce obojí: základní i abliterovanou verzi
        // téhož modelu. Bez tohoto testu by jedna z nich mohla tiše vypadnout
        // při dalším přerovnání katalogu.
        let models = recommended_models();
        let ids: std::collections::HashSet<&str> = models.iter().map(|m| m.id.as_str()).collect();
        for expected in [
            "gemma-4-26b-a4b-it",
            "gemma-4-26b-a4b-it-uncensored",
            "gemma-4-31b-it",
            "gemma-3-4b-it",
        ] {
            assert!(
                ids.contains(expected),
                "missing recommended model {expected}"
            );
        }
    }

    #[test]
    fn descriptions_do_not_target_specific_cards() {
        // Popisy dřív radily podle konkrétní karty ("pro RTX 3090"), což je
        // matoucí kdekoli jinde — rozložení modelu si appka spočítá sama
        // podle skutečně volné VRAM (viz llm::offload_plan).
        for m in recommended_models() {
            for banned in ["3090", "4070", "RTX"] {
                assert!(
                    !m.description.contains(banned),
                    "popis {} zminuje konkretni kartu: {banned}",
                    m.id
                );
            }
        }
    }
}
