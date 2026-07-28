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

# Jen CPU, bez GPU toolchainu
pnpm tauri dev --features llm-embedded
```

Na Windows jsou na to připravené dávky v kořeni repozitáře — všechny si samy
přepnou do svého adresáře, takže je můžeš spustit odkudkoli (dvojklikem
i z terminálu):

| skript | backend | co potřebuje navíc |
| --- | --- | --- |
| `run-dev.bat` | CUDA (NVIDIA) | CUDA Toolkit |
| `run-dev-vulkan.bat` | Vulkan (AMD/Intel) | Vulkan SDK |
| `run-dev-cpu.bat` | jen CPU | nic (stačí CMake + MSVC) |

Název `run-dev-local.bat` je vyhrazený pro tvůj vlastní launcher na míru stroji —
je v `.gitignore`, takže ho commit nesebere.

`run-dev.bat` si sám najde nejnovější nainstalovaný CUDA Toolkit a zjistí
compute capability karty přes `nvidia-smi`; obojí jde přebít proměnnými
`CUDA_PATH` a `CMAKE_CUDA_ARCHITECTURES`. Pozn.: CUDA 12.x odmítá novější
Visual Studio, proto se vyplatí mít CUDA 13.x.

Na macOS (Apple Silicon) viz `run-dev-mac.sh` — Metal nepotřebuje žádný extra
toolchain kromě Xcode Command Line Tools + CMake (`brew install cmake`). GPU
sdílí unified memory, takže se v aplikaci nechávají offloadnuté všechny vrstvy.
Release build pro macOS (`.dmg`, aarch64 + Metal) vzniká automaticky v release
workflow vedle Windows instalátoru. Aplikace není podepsaná Apple Developer ID —
při prvním spuštění je potřeba pravý klik → Otevřít (Gatekeeper).

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

## Instalátor pro Windows

```bat
instalator.bat
```

Sestaví release build a z něj MSI balíčky do `target/release/bundle/msi/`
— jeden pro češtinu, jeden pro angličtinu (`bundle.windows.wix.language`). Skript
vypíná updater artefakty přes `--config` override, protože ty vyžadují podpisový
klíč z GitHub Secrets; oficiální podepsané instalátory (NSIS + MSI) staví release
workflow při tagu `v*`.

MSI se instaluje pro celý počítač (vyžaduje práva správce). Instalátor obsahuje
jen aplikaci — modely, ComfyUI ani OpenVINO runtime se stahují až z aplikace
podle toho, co uživatel zapne. Build je bez GPU featur (`llm-cuda`/`llm-metal`/
`llm-vulkan`), aby aplikace běžela i na stroji bez CUDA runtime; vestavěnou
inferenci si sestav lokálně přes `run-dev.bat`.

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
