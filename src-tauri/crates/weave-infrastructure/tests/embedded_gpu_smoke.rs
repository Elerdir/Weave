//! Manuální smoke test reálné GPU inference. Vyžaduje feature `llm-embedded`
//! (typicky `llm-cuda`) a proměnnou WEAVE_SMOKE_MODEL s cestou k .gguf modelu.
//! Nikdy neběží v CI (#[ignore] + feature gate) — spouští se ručně:
//!
//!   WEAVE_SMOKE_MODEL=C:/path/model.gguf cargo test --features llm-cuda \
//!     --test embedded_gpu_smoke -- --ignored --nocapture

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
        max_tokens: 32,
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
        }
    }

    println!("--- výstup modelu ---\n{output}\n---------------------");
    println!("stats: {stats:?}");

    assert!(!output.trim().is_empty(), "model nevygeneroval žádný text");
    let stats = stats.expect("chybí GenerationStats (Done nepřišlo)");
    assert!(stats.completion_tokens > 0);
    assert!(stats.tokens_per_second > 0.0);
}
