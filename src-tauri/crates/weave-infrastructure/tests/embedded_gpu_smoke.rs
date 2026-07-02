//! Manuální smoke test reálné GPU inference. Vyžaduje feature `llm-embedded`
//! (typicky `llm-cuda`) a proměnnou WEAVE_SMOKE_MODEL s cestou k .gguf modelu.
//! Nikdy neběží v CI (#[ignore] + feature gate) — spouští se ručně:
//!
//!   WEAVE_SMOKE_MODEL=C:/path/model.gguf cargo test --features llm-cuda \
//!     --test embedded_gpu_smoke -- --ignored --nocapture
//!
//! Testy spouštěj jednotlivě (--test-threads=1, nebo přes název testu), ne
//! všechny najednou — llama.cpp backend jde inicializovat jen jednou za
//! proces, druhý souběžný EmbeddedLlamaClient::new() spadne na
//! BackendAlreadyInitialized. V reálné appce se volá jen jednou, takže to
//! produkci neovlivňuje.

#![cfg(feature = "llm-embedded")]

use tokio::sync::mpsc;
use weave_application::ports::llm_port::{ChatRequest, LlmPort, StreamChunk};
use weave_domain::{conversation::ConversationId, message::Message};
use weave_infrastructure::llm::embedded::EmbeddedLlamaClient;

#[tokio::test]
#[ignore = "vyžaduje GPU + stažený .gguf model, spouštět ručně"]
async fn generates_real_tokens_on_gpu() {
    let model_path = std::env::var("WEAVE_SMOKE_MODEL")
        .expect("nastav WEAVE_SMOKE_MODEL na cestu k .gguf souboru");

    let client = EmbeddedLlamaClient::new(model_path.into(), 999, 2048);

    let request = ChatRequest {
        messages: vec![Message::user(
            ConversationId::new(),
            "Řekni jedno krátké české slovo.",
        )],
        model_id: "smoke-test".into(),
        max_tokens: Some(32),
        context_length: None,
        temperature: 0.7,
        stream: true,
    };

    let (tx, mut rx) = mpsc::channel(64);
    client
        .chat_stream(request, tx)
        .await
        .expect("chat_stream selhal");

    let mut output = String::new();
    let mut stats = None;
    while let Some(chunk) = rx.recv().await {
        match chunk {
            StreamChunk::Token(t) => output.push_str(&t),
            StreamChunk::Done(s) => stats = Some(s),
            StreamChunk::Error(e) => panic!("inference selhala: {e}"),
            StreamChunk::ImageStage(_) => {}
        }
    }

    println!("--- výstup modelu ---\n{output}\n---------------------");
    println!("stats: {stats:?}");

    assert!(!output.trim().is_empty(), "model nevygeneroval žádný text");
    let stats = stats.expect("chybí GenerationStats (Done nepřišlo)");
    assert!(stats.completion_tokens > 0);
    assert!(stats.tokens_per_second > 0.0);
}

/// Regresní test na GGML_ASSERT(n_tokens_all <= cparams.n_batch) — prompt
/// delší než n_batch (512) musí jít do decode() po částech, jinak llama.cpp
/// spadne tvrdým abortem celého procesu (ne zachytitelná Rust chyba).
#[tokio::test]
#[ignore = "vyžaduje GPU + stažený .gguf model, spouštět ručně"]
async fn handles_prompt_longer_than_n_batch() {
    let model_path = std::env::var("WEAVE_SMOKE_MODEL")
        .expect("nastav WEAVE_SMOKE_MODEL na cestu k .gguf souboru");

    let client = EmbeddedLlamaClient::new(model_path.into(), 999, 4096);

    // Historie s dostatkem obsahu, aby prompt po tokenizaci přesáhl 512 tokenů.
    let long_paragraph = "Toto je dlouhá věta, která se v historii konverzace \
        opakuje mnohokrát, aby prompt po tokenizaci spolehlivě přesáhl pět set \
        dvanáct tokenů a otestoval dekódování promptu po částech. "
        .repeat(40);
    let messages = vec![Message::user(ConversationId::new(), long_paragraph)];

    let request = ChatRequest {
        messages,
        model_id: "smoke-test".into(),
        max_tokens: Some(16),
        context_length: None,
        temperature: 0.7,
        stream: true,
    };

    let (tx, mut rx) = mpsc::channel(64);
    client
        .chat_stream(request, tx)
        .await
        .expect("chat_stream selhal");

    let mut output = String::new();
    let mut stats = None;
    while let Some(chunk) = rx.recv().await {
        match chunk {
            StreamChunk::Token(t) => output.push_str(&t),
            StreamChunk::Done(s) => stats = Some(s),
            StreamChunk::Error(e) => panic!("inference selhala: {e}"),
            StreamChunk::ImageStage(_) => {}
        }
    }

    println!("--- výstup modelu (dlouhý prompt) ---\n{output}\n---------------------");
    println!("stats: {stats:?}");

    let stats = stats.expect("chybí GenerationStats (Done nepřišlo) — proces pravděpodobně spadl");
    assert!(
        stats.prompt_tokens > 512,
        "test nesplnil předpoklad — prompt musí být > 512 tokenů"
    );
    assert!(stats.completion_tokens > 0);
}

/// Regresní test na „decode: failed to find a memory slot" — historie delší
/// než kontextové okno se musí oříznout (vypustit nejstarší zprávy), ne
/// shodit generování. Malé n_ctx (1024) + historie ~3000 tokenů.
#[tokio::test]
#[ignore = "vyžaduje GPU + stažený .gguf model, spouštět ručně"]
async fn trims_history_that_exceeds_context_window() {
    let model_path = std::env::var("WEAVE_SMOKE_MODEL")
        .expect("nastav WEAVE_SMOKE_MODEL na cestu k .gguf souboru");

    let client = EmbeddedLlamaClient::new(model_path.into(), 999, 1024);

    // Několik dlouhých výměn — dohromady výrazně přes 1024 tokenů.
    let conv = ConversationId::new();
    let filler = "Tohle je dlouhý odstavec historie konverzace, který se opakuje, \
        aby celkový prompt spolehlivě přerostl kontextové okno modelu. "
        .repeat(15);
    let mut messages = Vec::new();
    for _ in 0..4 {
        messages.push(Message::user(conv.clone(), filler.clone()));
        messages.push(Message::assistant(conv.clone(), filler.clone(), None));
    }
    messages.push(Message::user(
        conv.clone(),
        "Řekni jedno krátké české slovo.",
    ));

    let request = ChatRequest {
        messages,
        model_id: "smoke-test".into(),
        max_tokens: Some(16),
        context_length: None,
        temperature: 0.7,
        stream: true,
    };

    let (tx, mut rx) = mpsc::channel(64);
    client
        .chat_stream(request, tx)
        .await
        .expect("chat_stream selhal");

    let mut output = String::new();
    let mut stats = None;
    while let Some(chunk) = rx.recv().await {
        match chunk {
            StreamChunk::Token(t) => output.push_str(&t),
            StreamChunk::Done(s) => stats = Some(s),
            StreamChunk::Error(e) => panic!("inference selhala (ořez nezafungoval): {e}"),
            StreamChunk::ImageStage(_) => {}
        }
    }

    println!("--- výstup (oříznutá historie) ---\n{output}\n---------------------");
    println!("stats: {stats:?}");

    let stats = stats.expect("chybí GenerationStats — decode pravděpodobně selhal");
    // Prompt se musel oříznout pod n_ctx (1024) i s rezervou pro odpověď.
    assert!(
        stats.prompt_tokens <= 1024 - 256,
        "prompt nebyl oříznut: {} tokenů",
        stats.prompt_tokens
    );
    assert!(stats.completion_tokens > 0);
}
