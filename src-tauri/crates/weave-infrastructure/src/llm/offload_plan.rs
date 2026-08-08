//! Rozhodnutí, **co poslat na GPU a co nechat v RAM**, aby šel použít
//! model větší než VRAM a přitom se negeneroval rychlostí CPU.
//!
//! Naivní `-ngl 99` u modelu, který se do VRAM nevejde, končí buď OOM,
//! nebo (na Windows/WDDM) přetečením do RAM přes PCIe — a to je pomalejší
//! než čistý CPU. Naivní „offloadni N vrstev" je u MoE modelů taky špatně:
//! do VRAM se dostanou i experti, kteří se pro každý token stejně mění,
//! takže se jen jezdí po PCIe.
//!
//! Správné řešení pro MoE (ekvivalent `--cpu-moe` v llama.cpp):
//! **všechny vrstvy na GPU, ale tenzory expertů natvrdo do RAM.** Na GPU
//! zůstane attention + KV cache (malé, počítá se pro každý token), experti
//! se počítají na CPU, kde jsou na ně rychlé AVX kernely a plná propustnost
//! paměti. Aktivní je vždy jen zlomek expertů, takže CPU práce je malá.
//!
//! Naměřeno (Gemma 4 26B A4B, Q4_K, 16 GB soubor / 8 GB VRAM):
//!
//! | konfigurace                          | tok/s |
//! |--------------------------------------|-------|
//! | všechno na CPU, výchozí vlákna       |  9,7  |
//! | všechno na CPU, laděná vlákna        | 11,2  |
//! | naivní offload 12 vrstev             |  7,1  |
//! | hybrid (experti v RAM) + op_offload  | 17,8  |
//!
//! Dvě protiintuitivní věci, které z měření plynou a jsou tu zadrátované:
//! (1) víc vláken škodí — E-jádra drží bariéru zpátky, optimum jsou zhruba
//! dvě třetiny logických jader; (2) `op_offload = false` (nedávat jednotlivé
//! operace na GPU, když tam nejsou váhy) srazí čas prvního tokenu na
//! polovinu a decode nezhorší.

use super::gguf_meta::GgufInfo;

/// „Všechny vrstvy na GPU" — llama.cpp bere velké číslo jako všechno.
pub const ALL_GPU_LAYERS: u32 = 1_000_000;

/// Rezerva VRAM pro plochu, prohlížeč a ostatní procesy. Na notebooku
/// s připojeným displejem je desktop compositor běžně přes půl giga.
const VRAM_RESERVE_BYTES: u64 = 768 * 1024 * 1024;

/// Rezerva na compute buffery llama.cpp (mezivýsledky, logits).
const COMPUTE_BUFFER_BYTES: u64 = 512 * 1024 * 1024;

/// Odhad podílu vah, které u MoE modelu tvoří experti. Pro 128 expertů
/// s 8 aktivními je to přes 90 %; 85 % je konzervativní střed, aby plán
/// nepodstřelil VRAM u modelů s menším počtem expertů.
const MOE_EXPERT_WEIGHT_SHARE: f64 = 0.85;

/// Když model neuvádí rozměry attention, odhadneme KV cache paušálem
/// na 1024 tokenů (odpovídá ~48 vrstvám s 4 KV hlavami v F16).
const KV_FALLBACK_BYTES_PER_1K: u64 = 200 * 1024 * 1024;

/// Co je na stroji k dispozici. Odděleno od detekce, aby šla logika
/// testovat bez GPU.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MachineProfile {
    /// Volná paměť zvolené karty. `None` = žádná použitelná GPU.
    ///
    /// Záměrně **volná**, ne celková: na kartě, která zároveň kreslí plochu,
    /// je rozdíl klidně přes gigabajt a plánovat podle celkové by znamenalo
    /// slíbit VRAM, kterou nikdy nedostaneme.
    pub vram_bytes: Option<u64>,
    /// Index zvoleného zařízení pro `with_devices`. `None` = nechat volbu
    /// na llama.cpp (typicky když je zařízení jediné).
    pub device_index: Option<usize>,
    /// Počet fyzických jader CPU.
    pub cpu_cores: usize,
}

/// Zvolená strategie. Slouží hlavně k logování a testům — konkrétní
/// parametry pro llama.cpp nese `OffloadDecision`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OffloadPlan {
    /// Bez GPU (vypnuto uživatelem nebo karta není).
    Cpu,
    /// Model se do VRAM vejde celý.
    FullGpu,
    /// MoE: všechny vrstvy na GPU, tenzory expertů v RAM.
    HybridMoe,
    /// Hustý model větší než VRAM — na GPU jde jen tolik vrstev, kolik
    /// se vejde. Pomalejší než hybrid, ale u hustých modelů není zbytí.
    PartialLayers,
}

impl OffloadPlan {
    pub fn label(self) -> &'static str {
        match self {
            OffloadPlan::Cpu => "CPU",
            OffloadPlan::FullGpu => "celý model na GPU",
            OffloadPlan::HybridMoe => "hybrid MoE (experti v RAM, attention na GPU)",
            OffloadPlan::PartialLayers => "částečný offload vrstev",
        }
    }
}

/// Konkrétní parametry, které si vezme `LlamaModelParams` / `LlamaContextParams`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OffloadDecision {
    pub plan: OffloadPlan,
    pub gpu_layers: u32,
    /// Přesunout tenzory expertů do RAM (`--cpu-moe`).
    pub cpu_moe: bool,
    /// `false` = neposílat jednotlivé operace na GPU, když tam nejsou váhy.
    pub op_offload: bool,
    pub threads: i32,
    /// Kvantovat KV cache na Q8_0 — u paměťově napjatých plánů ušetří
    /// polovinu VRAM za KV při zanedbatelné ztrátě kvality.
    pub quantized_kv: bool,
    /// Na kterém zařízení běžet (`None` = nechat volbu na llama.cpp).
    pub device_index: Option<usize>,
    /// Vysvětlení pro log a UI.
    pub reason: String,
}

/// Kolik vláken dát llama.cpp: **fyzická jádra**, ne logická vlákna.
///
/// V hybridním profilu na počtu vláken skoro nezáleží — decode je omezený
/// propustností systémové paměti, ne výpočtem, protože experti se streamují
/// z RAM. Naměřený rozptyl dvou běhů téže konfigurace (16 vláken: 16,94 a
/// 18,43 tok/s) byl větší než rozdíl mezi 6 a 22 vlákny. Fyzická jádra jsou
/// zvolená hlavně proto, že jsou předvídatelná: nepřekročí stroj, nespoléhají
/// na SMT sourozence (ti u paměťově omezené úlohy nepřidají) a odpovídají
/// tomu, co dělá llama.cpp samo.
///
/// Strop 32 je tam, kde končí měření — u strojů s víc jádry netvrdíme nic.
pub fn tune_threads(physical_cores: usize) -> i32 {
    physical_cores.clamp(1, 32) as i32
}

/// Největší kontext, pro který se KV cache ještě vejde do `kv_budget`.
///
/// Slouží k tomu, aby plán uměl říct **co s tím**, ne jen „nevejde se".
/// U modelů s velkými hlavami (Gemma 4 má 512 dimenzí) sežere KV cache celou
/// VRAM dřív než váhy, takže rozdíl mezi 64k a 16k kontextem rozhoduje o tom,
/// jestli model poběží hybridně na GPU, nebo celý na CPU.
fn max_context_for_kv_budget(info: &GgufInfo, kv_budget: u64) -> Option<u32> {
    // Jen když známe skutečnou geometrii. Paušální fallback by na jeden token
    // vyšel na 100 MB a doporučená hodnota by byla nesmysl.
    let per_token = kv_bytes_per_token(info)?;
    let tokens = kv_budget / per_token;
    // Pod 512 tokenů je to k ničemu — to už není rada, ale výsměch.
    (tokens >= 512).then(|| u32::try_from(tokens).unwrap_or(u32::MAX))
}

/// Kolik bajtů KV cache (Q8_0) zabere jeden token. `None` = model neuvádí
/// rozměry, ze kterých to jde spočítat.
fn kv_bytes_per_token(info: &GgufInfo) -> Option<u64> {
    let blocks = u64::from(info.block_count?);
    let head_dim = kv_head_dim(info)?;
    let kv_heads = u64::from(info.head_count_kv?);
    let bytes = blocks * kv_heads * head_dim * 2;
    (bytes > 0).then_some(bytes)
}

/// Rozměr K/V hlavy: buď explicitně z metadat (Gemma ho uvádí a NENÍ to
/// `embedding_length / head_count`), nebo dopočítaný.
fn kv_head_dim(info: &GgufInfo) -> Option<u64> {
    info.key_length
        .map(u64::from)
        .or_else(|| match (info.embedding_length, info.head_count) {
            (Some(emb), Some(heads)) if heads > 0 => Some(u64::from(emb / heads)),
            _ => None,
        })
        .filter(|dim| *dim > 0)
}

/// Doporučení pro hlášku: „sniž kontext na N". Prázdné, když by to nepomohlo.
fn context_advice(info: &GgufInfo, kv_budget: u64) -> String {
    match max_context_for_kv_budget(info, kv_budget) {
        // Zaokrouhlíme dolů na tisíce, ať to vypadá jako nastavitelná hodnota.
        Some(tokens) => format!(
            " Snížením kontextu na ~{} tisíc tokenů by model běžel na GPU.",
            tokens / 1000
        ),
        None => String::new(),
    }
}

/// Odhad velikosti KV cache pro dané okno kontextu.
fn estimate_kv_bytes(info: &GgufInfo, context_tokens: u32, quantized: bool) -> u64 {
    let bytes_per_element: u64 = if quantized { 1 } else { 2 };
    let blocks = info.block_count.unwrap_or(0) as u64;

    // Rozměr K/V hlavy: buď explicitně z metadat (Gemma), nebo
    // embedding / počet hlav.
    let head_dim =
        info.key_length
            .map(u64::from)
            .or_else(|| match (info.embedding_length, info.head_count) {
                (Some(emb), Some(heads)) if heads > 0 => Some((emb / heads) as u64),
                _ => None,
            });
    let kv_heads = info.head_count_kv.map(u64::from);

    match (blocks, head_dim, kv_heads) {
        (b, Some(dim), Some(heads)) if b > 0 && dim > 0 && heads > 0 => {
            // K i V, tedy ×2.
            context_tokens as u64 * b * heads * dim * 2 * bytes_per_element
        }
        _ => {
            let raw = (context_tokens as u64).div_ceil(1024) * KV_FALLBACK_BYTES_PER_1K;
            if quantized {
                raw / 2
            } else {
                raw
            }
        }
    }
}

/// Naplánuje offload pro konkrétní model na konkrétním stroji.
///
/// `model_bytes` je velikost GGUF souboru — u kvantovaného modelu odpovídá
/// velikosti vah v paměti dost přesně na to, aby se podle ní rozhodovalo.
pub fn plan_offload(
    model_bytes: u64,
    info: &GgufInfo,
    machine: &MachineProfile,
    context_tokens: u32,
    use_gpu: bool,
) -> OffloadDecision {
    let threads = tune_threads(machine.cpu_cores);

    let cpu_only = |reason: String| OffloadDecision {
        plan: OffloadPlan::Cpu,
        gpu_layers: 0,
        cpu_moe: false,
        op_offload: true,
        threads,
        quantized_kv: false,
        device_index: None,
        reason,
    };

    if !use_gpu {
        return cpu_only("GPU vypnuta v nastavení příběhu.".into());
    }
    let Some(vram) = machine.vram_bytes.filter(|v| *v > 0) else {
        return cpu_only("Nenalezena použitelná GPU — počítám na CPU.".into());
    };

    let budget = vram.saturating_sub(VRAM_RESERVE_BYTES);
    if budget <= COMPUTE_BUFFER_BYTES {
        return cpu_only(format!(
            "VRAM ({} MB) je po rezervě na plochu příliš malá.",
            vram / (1024 * 1024)
        ));
    }
    let weights_budget = budget - COMPUTE_BUFFER_BYTES;

    // 1) Vejde se celý model i s KV cache? Pak žádná kouzla nepotřebujeme.
    let kv_f16 = estimate_kv_bytes(info, context_tokens, false);
    if model_bytes + kv_f16 <= weights_budget {
        return OffloadDecision {
            plan: OffloadPlan::FullGpu,
            gpu_layers: ALL_GPU_LAYERS,
            cpu_moe: false,
            op_offload: true,
            threads,
            quantized_kv: false,
            device_index: machine.device_index,
            reason: format!(
                "Model ({} MB) se vejde do VRAM ({} MB) — všechny vrstvy na GPU.",
                model_bytes / (1024 * 1024),
                vram / (1024 * 1024)
            ),
        };
    }

    // Od téhle chvíle je model větší než VRAM. KV cache kvantujeme —
    // ušetřená VRAM je přesně to, co rozhoduje o tom, kolik se toho
    // na kartu vejde.
    let kv_q8 = estimate_kv_bytes(info, context_tokens, true);
    let after_kv = weights_budget.saturating_sub(kv_q8);

    // 2) MoE: experti do RAM, zbytek (attention, embeddings, normy) na GPU.
    if info.is_moe() {
        let resident = (model_bytes as f64 * (1.0 - MOE_EXPERT_WEIGHT_SHARE)) as u64;
        if resident <= after_kv {
            return OffloadDecision {
                plan: OffloadPlan::HybridMoe,
                gpu_layers: ALL_GPU_LAYERS,
                cpu_moe: true,
                // Klíčové: bez tohohle jde první token ~2× pomaleji,
                // protože llama.cpp tahá operace nad CPU tenzory na GPU.
                op_offload: false,
                threads,
                quantized_kv: true,
                device_index: machine.device_index,
                reason: format!(
                    "MoE model ({} MB) je větší než VRAM ({} MB) — experti zůstávají \
                     v RAM, attention a KV cache jedou na GPU.",
                    model_bytes / (1024 * 1024),
                    vram / (1024 * 1024)
                ),
            };
        }
    }

    // 3) Hustý model (nebo MoE, kde se ani attention nevejde) — offloadneme
    //    tolik vrstev, kolik se vejde.
    let blocks = info.block_count.unwrap_or(0);
    if blocks == 0 {
        return cpu_only(
            "Model je větší než VRAM a hlavička neuvádí počet vrstev — počítám na CPU.".into(),
        );
    }
    let bytes_per_layer = (model_bytes / blocks as u64).max(1);
    let fits = (after_kv / bytes_per_layer) as u32;
    if fits == 0 {
        // Skoro vždy za to může KV cache, ne váhy: u modelu s velkými hlavami
        // ji kontext nafoukne přes celý rozpočet dřív, než dojde na vrstvy.
        // Řekneme rovnou, kam kontext stáhnout, ať uživatel nehádá.
        let resident = if info.is_moe() {
            (model_bytes as f64 * (1.0 - MOE_EXPERT_WEIGHT_SHARE)) as u64
        } else {
            bytes_per_layer
        };
        let advice = context_advice(info, weights_budget.saturating_sub(resident));
        return cpu_only(format!(
            "Kontext {context_tokens} tokenů si žádá {} MB KV cache, takže do VRAM \
             ({} MB) nezbude místo ani na jednu vrstvu — počítám na CPU.{advice}",
            kv_q8 / (1024 * 1024),
            vram / (1024 * 1024)
        ));
    }
    let gpu_layers = fits.min(blocks);
    OffloadDecision {
        plan: OffloadPlan::PartialLayers,
        gpu_layers,
        cpu_moe: info.is_moe(),
        op_offload: false,
        threads,
        quantized_kv: true,
        device_index: machine.device_index,
        reason: format!(
            "Hustý model ({} MB) je větší než VRAM ({} MB) — na GPU jde {gpu_layers} z \
             {blocks} vrstev, zbytek počítá CPU.",
            model_bytes / (1024 * 1024),
            vram / (1024 * 1024)
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GB: u64 = 1024 * 1024 * 1024;

    fn moe_info() -> GgufInfo {
        // Gemma 4 26B A4B — 128 expertů, 48 vrstev, 4 KV hlavy à 256.
        GgufInfo {
            architecture: "gemma4".into(),
            expert_count: Some(128),
            block_count: Some(48),
            embedding_length: Some(2560),
            head_count: Some(8),
            head_count_kv: Some(4),
            key_length: Some(256),
            value_length: Some(256),
        }
    }

    /// Skutečná geometrie z `D:\models\gemma-4-26B-A4B-it-UD-Q4_K_XL.gguf`,
    /// jak ji hlásí llama.cpp: 30 vrstev, hlavy 512 dimenzí, `head_count_kv`
    /// je pole `[8, …, 2, …]` (bereme maximum 8).
    fn gemma4_26b_info() -> GgufInfo {
        GgufInfo {
            architecture: "gemma4".into(),
            expert_count: Some(128),
            block_count: Some(30),
            embedding_length: Some(2560),
            head_count: Some(8),
            head_count_kv: Some(8),
            key_length: Some(512),
            value_length: Some(512),
        }
    }

    #[test]
    fn huge_context_falls_back_to_cpu_but_says_what_to_do() {
        // Reálný případ z tohohle stroje: 16,2GB model, RTX 4070 Laptop se
        // 7180 MB volné VRAM, kontext 64k. KV cache u hlav o 512 dimenzích
        // sežere víc než celý rozpočet, takže hybrid nevyjde. Hláška ale musí
        // říct, kam kontext stáhnout — bez toho uživatel jen vidí, že je to
        // pomalé, a netuší proč.
        let info = gemma4_26b_info();
        let machine = MachineProfile {
            vram_bytes: Some(7180 * 1024 * 1024),
            device_index: Some(0),
            cpu_cores: 16,
        };
        let decision = plan_offload(16_200_000_000, &info, &machine, 64_000, true);
        assert_eq!(decision.plan, OffloadPlan::Cpu);
        assert!(
            decision.reason.contains("Snížením kontextu"),
            "hláška má poradit menší kontext, ne jen konstatovat pád: {}",
            decision.reason
        );

        // A při tom menším kontextu už hybrid vyjít musí, jinak je rada lež.
        let smaller = plan_offload(16_200_000_000, &info, &machine, 8_000, true);
        assert_eq!(smaller.plan, OffloadPlan::HybridMoe);
        assert!(smaller.cpu_moe);
    }

    fn dense_info() -> GgufInfo {
        GgufInfo {
            architecture: "llama".into(),
            expert_count: None,
            block_count: Some(32),
            embedding_length: Some(4096),
            head_count: Some(32),
            head_count_kv: Some(8),
            key_length: None,
            value_length: None,
        }
    }

    fn machine(vram_gb: u64) -> MachineProfile {
        MachineProfile {
            vram_bytes: Some(vram_gb * GB),
            device_index: Some(1),
            cpu_cores: 16,
        }
    }

    #[test]
    fn big_moe_on_small_card_keeps_experts_in_ram() {
        // Přesně uživatelův případ: 16 GB model, 8 GB karta.
        let d = plan_offload(16 * GB, &moe_info(), &machine(8), 8192, true);
        assert_eq!(d.plan, OffloadPlan::HybridMoe);
        assert_eq!(d.gpu_layers, ALL_GPU_LAYERS);
        assert!(d.cpu_moe);
        assert!(!d.op_offload, "op_offload musí být vypnutý (TTFT)");
        assert!(d.quantized_kv);
    }

    #[test]
    fn model_that_fits_goes_fully_to_gpu() {
        let d = plan_offload(6 * GB, &dense_info(), &machine(24), 4096, true);
        assert_eq!(d.plan, OffloadPlan::FullGpu);
        assert_eq!(d.gpu_layers, ALL_GPU_LAYERS);
        assert!(!d.cpu_moe);
        assert!(d.op_offload);
    }

    #[test]
    fn big_dense_model_offloads_only_what_fits() {
        let d = plan_offload(40 * GB, &dense_info(), &machine(8), 4096, true);
        assert_eq!(d.plan, OffloadPlan::PartialLayers);
        assert!(d.gpu_layers > 0 && d.gpu_layers < 32, "{}", d.gpu_layers);
        assert!(!d.cpu_moe);
    }

    #[test]
    fn dense_model_without_layer_count_falls_back_to_cpu() {
        let info = GgufInfo {
            block_count: None,
            ..dense_info()
        };
        let d = plan_offload(40 * GB, &info, &machine(8), 4096, true);
        assert_eq!(d.plan, OffloadPlan::Cpu);
    }

    #[test]
    fn gpu_disabled_means_cpu() {
        let d = plan_offload(16 * GB, &moe_info(), &machine(24), 4096, false);
        assert_eq!(d.plan, OffloadPlan::Cpu);
        assert_eq!(d.gpu_layers, 0);
    }

    #[test]
    fn no_gpu_means_cpu() {
        let machine = MachineProfile {
            vram_bytes: None,
            device_index: None,
            cpu_cores: 8,
        };
        let d = plan_offload(16 * GB, &moe_info(), &machine, 4096, true);
        assert_eq!(d.plan, OffloadPlan::Cpu);
    }

    #[test]
    fn tiny_card_is_not_worth_it() {
        // 1 GB iGPU: po rezervě na plochu nezbude ani na compute buffery.
        let d = plan_offload(16 * GB, &moe_info(), &machine(1), 4096, true);
        assert_eq!(d.plan, OffloadPlan::Cpu);
    }

    #[test]
    fn threads_follow_physical_cores() {
        assert_eq!(tune_threads(16), 16); // Core Ultra 9 185H: 6 P + 8 E + 2 LP-E
        assert_eq!(tune_threads(8), 8);
        assert_eq!(tune_threads(64), 32); // strop, kde končí měření
        assert_eq!(tune_threads(1), 1);
        assert_eq!(tune_threads(0), 1);
    }

    #[test]
    fn chosen_device_is_carried_into_every_gpu_plan() {
        // Kdyby se index ztratil, llama.cpp by sáhlo po nultém zařízení —
        // na hybridním notebooku po integrované grafice.
        let moe = plan_offload(16 * GB, &moe_info(), &machine(8), 8192, true);
        let full = plan_offload(2 * GB, &dense_info(), &machine(24), 4096, true);
        let partial = plan_offload(40 * GB, &dense_info(), &machine(8), 4096, true);

        assert_eq!(moe.device_index, Some(1));
        assert_eq!(full.device_index, Some(1));
        assert_eq!(partial.device_index, Some(1));
    }

    #[test]
    fn cpu_plan_has_no_device() {
        let d = plan_offload(16 * GB, &moe_info(), &machine(8), 4096, false);
        assert_eq!(d.device_index, None);
    }

    #[test]
    fn kv_estimate_scales_with_context_and_halves_when_quantized() {
        let info = moe_info();
        let small = estimate_kv_bytes(&info, 4096, false);
        let big = estimate_kv_bytes(&info, 8192, false);
        assert_eq!(big, small * 2);
        assert_eq!(estimate_kv_bytes(&info, 4096, true), small / 2);
    }

    #[test]
    fn kv_estimate_has_fallback_without_metadata() {
        let info = GgufInfo {
            architecture: "mystery".into(),
            ..Default::default()
        };
        assert!(estimate_kv_bytes(&info, 4096, false) > 0);
    }

    #[test]
    fn larger_context_can_flip_full_gpu_to_hybrid() {
        // Model se do VRAM vejde, ale KV cache pro velké okno už ne.
        let info = moe_info();
        assert_eq!(
            plan_offload(4 * GB, &info, &machine(8), 4096, true).plan,
            OffloadPlan::FullGpu
        );
        assert_eq!(
            plan_offload(4 * GB, &info, &machine(8), 32_768, true).plan,
            OffloadPlan::HybridMoe
        );
    }
}
