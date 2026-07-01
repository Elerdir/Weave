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
use weave_domain::message::{GenerationStats, ModelBackend, Role};

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
fn build_prompt(request: &ChatRequest) -> String {
    let mut out = String::new();
    for msg in &request.messages {
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

fn run_inference(
    backend: &LlamaBackend,
    model: &LlamaModel,
    n_ctx: u32,
    req: &WorkerRequest,
) -> AppResult<()> {
    let ctx_params = LlamaContextParams::default()
        .with_n_ctx(NonZeroU32::new(n_ctx.max(512)))
        .with_n_batch(512);
    let mut ctx = model
        .new_context(backend, ctx_params)
        .map_err(|e| AppError::Llm(format!("Kontext: {e}")))?;

    let prompt = build_prompt(&req.request);
    let tokens = model
        .str_to_token(&prompt, AddBos::Always)
        .map_err(|e| AppError::Llm(format!("Tokenizace: {e}")))?;

    let mut batch = LlamaBatch::new(tokens.len().max(512), 1);
    let last = tokens.len() - 1;
    for (i, token) in tokens.iter().enumerate() {
        batch
            .add(*token, i as i32, &[0], i == last)
            .map_err(|e| AppError::Llm(format!("Batch: {e}")))?;
    }
    ctx.decode(&mut batch)
        .map_err(|e| AppError::Llm(format!("Decode: {e}")))?;

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
    let mut n_cur = batch.n_tokens();
    let mut completion_tokens = 0u32;
    let max_tokens = req.request.max_tokens as i32;

    while completion_tokens < max_tokens as u32 {
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
