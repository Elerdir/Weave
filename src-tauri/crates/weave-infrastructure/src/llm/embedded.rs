//! Vestavěná inference přes llama.cpp (feature `llm-embedded` / `llm-cuda` …).
//!
//! Model není `Send`, proto ho vlastní dedikované vlákno a komunikuje se přes
//! kanál — `EmbeddedLlamaClient` drží jen `Sender` (je `Send + Sync`).

use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender};

use async_trait::async_trait;
use llama_cpp_2::{
    context::params::LlamaContextParams,
    llama_backend::LlamaBackend,
    llama_batch::LlamaBatch,
    model::{params::LlamaModelParams, AddBos, LlamaModel},
    sampling::LlamaSampler,
};
use tokio::sync::mpsc;
use weave_application::{
    error::{AppError, AppResult},
    ports::llm_port::{ChatRequest, LlmPort, StreamChunk},
};
use weave_domain::message::{GenerationStats, Message, ModelBackend, Role};

#[cfg(feature = "llm-cuda")]
const ACTIVE_BACKEND: ModelBackend = ModelBackend::LocalCuda;
#[cfg(all(feature = "llm-metal", not(feature = "llm-cuda")))]
const ACTIVE_BACKEND: ModelBackend = ModelBackend::LocalMetal;
#[cfg(all(
    feature = "llm-vulkan",
    not(any(feature = "llm-cuda", feature = "llm-metal"))
))]
const ACTIVE_BACKEND: ModelBackend = ModelBackend::LocalVulkan;
#[cfg(not(any(feature = "llm-cuda", feature = "llm-metal", feature = "llm-vulkan")))]
const ACTIVE_BACKEND: ModelBackend = ModelBackend::LocalCpu;

struct WorkerRequest {
    request: ChatRequest,
    tx: mpsc::Sender<StreamChunk>,
}

pub struct EmbeddedLlamaClient {
    tx_req: Sender<WorkerRequest>,
}

impl EmbeddedLlamaClient {
    /// Spustí worker vlákno, které načte model a obsluhuje požadavky.
    /// `n_gpu_layers` = kolik vrstev nahrát na GPU (velké číslo = všechny).
    pub fn new(model_path: PathBuf, n_gpu_layers: u32, n_ctx: u32) -> Self {
        let (tx_req, rx_req) = std::sync::mpsc::channel::<WorkerRequest>();

        std::thread::Builder::new()
            .name("llama-worker".into())
            .spawn(move || worker_loop(model_path, n_gpu_layers, n_ctx, rx_req))
            .expect("nepodařilo se spustit llama vlákno");

        Self { tx_req }
    }
}

#[async_trait]
impl LlmPort for EmbeddedLlamaClient {
    async fn chat_stream(
        &self,
        request: ChatRequest,
        tx: mpsc::Sender<StreamChunk>,
    ) -> AppResult<()> {
        self.tx_req
            .send(WorkerRequest { request, tx })
            .map_err(|_| AppError::Llm("Llama worker není dostupný".into()))
    }

    async fn list_available_models(&self) -> AppResult<Vec<String>> {
        Ok(vec![])
    }
}

/// Sestaví jednoduchý chat prompt z historie (obecný ChatML-like formát).
fn build_prompt(messages: &[&Message]) -> String {
    let mut out = String::new();
    for msg in messages {
        let role = match msg.role {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
        };
        out.push_str(&format!(
            "<|im_start|>{role}\n{}<|im_end|>\n",
            msg.content.trim()
        ));
    }
    out.push_str("<|im_start|>assistant\n");
    out
}

/// Vypustí nejstarší zprávu, kterou lze obětovat: přeskakuje system zprávy
/// (persona, kontext souborů) a nikdy nesahá na poslední zprávu (aktuální
/// dotaz). Vrací false, když už není co vypustit.
fn drop_oldest_droppable(messages: &mut Vec<&Message>) -> bool {
    if messages.len() <= 1 {
        return false;
    }
    let last = messages.len() - 1;
    if let Some(idx) = messages[..last].iter().position(|m| m.role != Role::System) {
        messages.remove(idx);
        true
    } else {
        false
    }
}

fn worker_loop(model_path: PathBuf, n_gpu_layers: u32, n_ctx: u32, rx: Receiver<WorkerRequest>) {
    let backend = match LlamaBackend::init() {
        Ok(b) => b,
        Err(e) => {
            drain_with_error(rx, &format!("Llama backend selhal: {e}"));
            return;
        }
    };

    let model_params = LlamaModelParams::default().with_n_gpu_layers(n_gpu_layers);
    let model = match LlamaModel::load_from_file(&backend, &model_path, &model_params) {
        Ok(m) => m,
        Err(e) => {
            drain_with_error(rx, &format!("Načtení modelu selhalo: {e}"));
            return;
        }
    };
    tracing::info!(?model_path, n_gpu_layers, "Vestavěný model načten (GPU)");

    for req in rx {
        if let Err(e) = run_inference(&backend, &model, n_ctx, &req) {
            let _ = req.tx.blocking_send(StreamChunk::Error(e.to_string()));
        }
    }
}

/// Když se worker nepodaří nastartovat, každému požadavku vrátíme chybu.
fn drain_with_error(rx: Receiver<WorkerRequest>, msg: &str) {
    for req in rx {
        let _ = req.tx.blocking_send(StreamChunk::Error(msg.to_string()));
    }
}

const N_BATCH: usize = 512;

fn run_inference(
    backend: &LlamaBackend,
    model: &LlamaModel,
    n_ctx: u32,
    req: &WorkerRequest,
) -> AppResult<()> {
    // Kontext držíme v mezích toho, na co byl model trénovaný.
    let n_ctx_eff = n_ctx.max(512).min(model.n_ctx_train());

    let ctx_params = LlamaContextParams::default()
        .with_n_ctx(NonZeroU32::new(n_ctx_eff))
        .with_n_batch(N_BATCH as u32);
    let mut ctx = model
        .new_context(backend, ctx_params)
        .map_err(|e| AppError::Llm(format!("Kontext: {e}")))?;

    // Historie musí nechat v okně rezervu pro odpověď — jinak decode spadne
    // na „failed to find a memory slot" a generování nejde vůbec spustit.
    // Vypouštíme nejstarší zprávy, dokud se prompt nevejde.
    let reserve = (n_ctx_eff / 4).max(256);
    let mut messages: Vec<&Message> = req.request.messages.iter().collect();
    let tokens = loop {
        let prompt = build_prompt(&messages);
        let tokens = model
            .str_to_token(&prompt, AddBos::Always)
            .map_err(|e| AppError::Llm(format!("Tokenizace: {e}")))?;
        if (tokens.len() as u32).saturating_add(reserve) <= n_ctx_eff {
            break tokens;
        }
        if !drop_oldest_droppable(&mut messages) {
            return Err(AppError::Llm(format!(
                "Zpráva se nevejde do kontextového okna modelu ({n_ctx_eff} tokenů). \
                 Zkrať ji, nebo zvětši kontextové okno v Nastavení → AI model."
            )));
        }
    };

    // Prompt musí jít do decode() po částech ≤ N_BATCH — llama.cpp jinak
    // spadne na GGML_ASSERT(n_tokens_all <= cparams.n_batch). U delší
    // historie konverzace (teď navíc bez umělého stropu na délku odpovědi)
    // prompt běžně přesáhne 512 tokenů.
    let mut batch = LlamaBatch::new(N_BATCH, 1);
    let last = tokens.len() - 1;
    for chunk_start in (0..tokens.len()).step_by(N_BATCH) {
        let chunk_end = (chunk_start + N_BATCH).min(tokens.len());
        batch.clear();
        for (i, token) in tokens.iter().enumerate().take(chunk_end).skip(chunk_start) {
            batch
                .add(*token, i as i32, &[0], i == last)
                .map_err(|e| AppError::Llm(format!("Batch: {e}")))?;
        }
        ctx.decode(&mut batch)
            .map_err(|e| AppError::Llm(format!("Decode: {e}")))?;
    }

    let mut sampler = LlamaSampler::chain_simple([
        LlamaSampler::top_k(40),
        LlamaSampler::top_p(0.95, 1),
        LlamaSampler::temp(req.request.temperature.max(0.1)),
        LlamaSampler::dist(1234),
    ]);

    // UTF-8 decoder napříč tokeny — jeden token může být jen část
    // vícebajtového znaku (diakritika, emoji apod.).
    let mut utf8_decoder = encoding_rs::UTF_8.new_decoder();

    let start = std::time::Instant::now();
    // Ne batch.n_tokens() — po dekódování po částech odráží jen poslední kus.
    let mut n_cur = tokens.len() as i32;
    let mut completion_tokens = 0u32;
    let max_tokens = req.request.max_tokens;
    let n_ctx_limit = n_ctx_eff as i32;

    loop {
        // Zastavíme se jen na přirozeném konci (EOG token, viz níže), na
        // volitelném stropu z požadavku, nebo na skutečné technické hranici
        // — zaplněném kontextovém okně. Bez umělého omezení navíc.
        if should_stop_generating(completion_tokens, max_tokens, n_cur, n_ctx_limit) {
            break;
        }

        let token = sampler.sample(&ctx, batch.n_tokens() - 1);
        sampler.accept(token);

        if model.is_eog_token(token) {
            break;
        }

        let piece = model
            .token_to_piece(token, &mut utf8_decoder, true, None)
            .unwrap_or_default();
        if req.tx.blocking_send(StreamChunk::Token(piece)).is_err() {
            return Ok(()); // příjemce zmizel
        }
        completion_tokens += 1;

        batch.clear();
        batch
            .add(token, n_cur, &[0], true)
            .map_err(|e| AppError::Llm(format!("Batch: {e}")))?;
        n_cur += 1;
        ctx.decode(&mut batch)
            .map_err(|e| AppError::Llm(format!("Decode: {e}")))?;
    }

    let elapsed = start.elapsed().as_secs_f64();
    let tps = if elapsed > 0.0 {
        completion_tokens as f64 / elapsed
    } else {
        0.0
    };
    let _ = req.tx.blocking_send(StreamChunk::Done(GenerationStats {
        tokens_per_second: tps,
        prompt_tokens: tokens.len() as u32,
        completion_tokens,
        model_id: req.request.model_id.clone(),
        backend: ACTIVE_BACKEND,
    }));
    Ok(())
}

/// Rozhoduje, kdy generační smyčka skončí — na volitelném stropu z požadavku
/// (pokud je nastaven), nebo na skutečné technické hranici (zaplněné
/// kontextové okno). Bez zadaného stropu pokračuje, dokud model sám
/// nenarazí na EOG token nebo dokud se nezaplní kontext.
fn should_stop_generating(
    completion_tokens: u32,
    max_tokens: Option<u32>,
    n_cur: i32,
    n_ctx_limit: i32,
) -> bool {
    max_tokens.is_some_and(|max| completion_tokens >= max) || n_cur >= n_ctx_limit
}

#[cfg(test)]
mod tests {
    use super::*;
    use weave_domain::conversation::ConversationId;

    fn history() -> Vec<Message> {
        let conv = ConversationId::new();
        vec![
            Message::system(conv.clone(), "persona prompt"),
            Message::user(conv.clone(), "první otázka"),
            Message::assistant(conv.clone(), "první odpověď", None),
            Message::user(conv.clone(), "druhá otázka"),
        ]
    }

    #[test]
    fn drop_oldest_skips_system_and_keeps_last() {
        let owned = history();
        let mut msgs: Vec<&Message> = owned.iter().collect();

        // 1. vypuštění: nejstarší ne-system → "první otázka"
        assert!(drop_oldest_droppable(&mut msgs));
        let contents: Vec<&str> = msgs.iter().map(|m| m.content.as_str()).collect();
        assert_eq!(
            contents,
            vec!["persona prompt", "první odpověď", "druhá otázka"]
        );

        // 2. vypuštění: "první odpověď"
        assert!(drop_oldest_droppable(&mut msgs));
        let contents: Vec<&str> = msgs.iter().map(|m| m.content.as_str()).collect();
        assert_eq!(contents, vec!["persona prompt", "druhá otázka"]);

        // Zbývá jen system + poslední zpráva → není co vypustit
        assert!(!drop_oldest_droppable(&mut msgs));
        assert_eq!(msgs.len(), 2);
    }

    #[test]
    fn drop_oldest_never_touches_single_message() {
        let conv = ConversationId::new();
        let owned = [Message::user(conv, "jediná zpráva")];
        let mut msgs: Vec<&Message> = owned.iter().collect();
        assert!(!drop_oldest_droppable(&mut msgs));
        assert_eq!(msgs.len(), 1);
    }

    #[test]
    fn continues_without_cap_until_context_window_fills() {
        assert!(!should_stop_generating(10_000, None, 100, 4096));
        assert!(should_stop_generating(10_000, None, 4096, 4096));
    }

    #[test]
    fn stops_at_explicit_cap_before_context_limit() {
        assert!(!should_stop_generating(31, Some(32), 100, 4096));
        assert!(should_stop_generating(32, Some(32), 100, 4096));
    }

    #[test]
    fn stops_at_context_limit_even_under_explicit_cap() {
        assert!(should_stop_generating(5, Some(1000), 4096, 4096));
    }
}
