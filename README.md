# Weave

Multiplatformní AI chat aplikace s inteligentním routováním modelů, generováním obrázků přes ComfyUI a automatickou správou modelů.

## Stack

- **Shell**: Tauri 2
- **Frontend**: Svelte 5 + TypeScript + Tailwind CSS 4
- **Backend**: Rust (clean architecture — domain / application / infrastructure / shell)
- **LLM**: Mistral API + lokální modely přes llama.cpp
- **Image gen**: ComfyUI (SDXL, Flux, PuLID)
- **Storage**: SQLite (sqlx) + OS Keychain (keyring)

## Vývoj

```bash
pnpm install
pnpm tauri dev
```

### Vestavěná GPU inference (volitelné)

Weave umí i vestavěnou inferenci přes llama.cpp (`llama-cpp-2`) s CUDA/Metal/Vulkan
akcelerací — model se pak nahraje přímo do procesu, bez externího serveru.
Vyžaduje CMake + odpovídající GPU toolchain a zkompiluje se jen s feature flagem:

```bash
# Windows + NVIDIA CUDA
pnpm tauri dev --features llm-cuda

# macOS (Apple Silicon / Metal)
pnpm tauri dev --features llm-metal

# Vulkan (AMD/Intel/cross-platform)
pnpm tauri dev --features llm-vulkan
```

Na Windows viz `run-dev.bat` — nastavuje `CMAKE_CUDA_ARCHITECTURES` (uprav podle
GPU: RTX 30xx=86, RTX 40xx=89, RTX 20xx=75) a vybírá funkční CUDA verzi (CUDA 13.x
pro novější MSVC/Visual Studio — CUDA 12.x starší VS odmítá).

Model (`.gguf`) se nastaví v aplikaci: **Nastavení → AI model → Vestavěná GPU
inference** → vyber doporučený model a klikni Stáhnout (appka po dokončení
automaticky nastaví backend i cestu — vlastní `.gguf` soubor jde přidat přes
„Pokročilé"). Bez feature flagu appka normálně staví a běží (fallback na
Mistral API / HTTP local server) — CI ho nikdy nesestavuje.

### ComfyUI — automatická instalace (volitelné)

Appka umí ComfyUI + PuLID (reference obrázky) nainstalovat sama, jedním
tlačítkem: **Nastavení → ComfyUI → Nainstalovat ComfyUI + PuLID**. Vyžaduje
Python 3 a Git na stroji; zbytek (git clone ComfyUI, venv, PyTorch — CUDA
build pokud je NVIDIA GPU, PuLID custom node + jeho závislosti) se stáhne a
nainstaluje automaticky. Trvá řádově minuty až desítky minut podle rychlosti
připojení. Ověřeno end-to-end (viz `tests/comfy_install_smoke.rs`, `#[ignore]`,
nikdy neběží v CI).

## Testování

```bash
# Rust unit + integrační testy
cargo test --all

# Coverage
cargo llvm-cov --all --html

# Frontend unit
pnpm test

# E2E + vizuální
pnpm playwright test
```

## Architektura

```
src-tauri/crates/
├── weave-domain/        # Entity, Value Objects — bez závislostí
├── weave-application/   # Use Cases, porty (traits)
├── weave-infrastructure/ # SQLite, keyring, HTTP adaptery
└── weave-shell/         # Tauri commands, entry point
```

## Branch model

- `main` — chráněná, pouze přes PR + passing CI
- `feature/*` — nové funkce
- `fix/*` — opravy
- `chore/*` — údržba, deps, CI
