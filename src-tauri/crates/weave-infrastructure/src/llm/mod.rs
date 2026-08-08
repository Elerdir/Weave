pub mod local_client;
pub mod unconfigured_client;

// Čtení GGUF hlavičky a plánování offloadu jsou čistý Rust bez llama.cpp,
// takže se kompilují vždy — potřebuje je i UI, které jen ukazuje, jak by se
// model rozložil, aniž by byl sestavený inference backend.
pub mod gguf_meta;
pub mod offload_plan;

// Výčet zařízení se ptá ggml, takže vyžaduje llama.cpp.
#[cfg(feature = "llm-embedded")]
pub mod device_catalog;

#[cfg(feature = "llm-embedded")]
pub mod embedded;

pub use gguf_meta::{read_gguf_info, GgufInfo};
pub use offload_plan::{plan_offload, MachineProfile, OffloadDecision, OffloadPlan};
