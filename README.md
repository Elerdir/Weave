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

Weave umí i vestavěnou inferenci přes llama.cpp (`llama-cpp-2`) — model se
nahraje přímo do procesu, bez externího serveru. Vyžaduje CMake + GPU SDK
a zkompiluje se jen s feature flagem:

```bash
# GPU přes Vulkan (NVIDIA, AMD i Intel) — hlavní cesta
pnpm tauri dev --features llm-vulkan

# macOS (Apple Silicon / Metal)
pnpm tauri dev --features llm-metal

# Jen CPU, bez GPU toolchainu
pnpm tauri dev --features llm-embedded
```

**CUDA se pro text nestaví.** Vulkan pokrývá NVIDII stejně jako AMD a Intel,
SDK je o řád menší a u modelů větších než VRAM stejně nerozhoduje backend, ale
rozložení modelu (viz níž). CUDA v projektu zůstává jen pro ComfyUI, které si
ji instaluje samo do vlastního Python prostředí.

Na Windows jsou na to připravené dávky v kořeni repozitáře — obě si samy
přepnou do svého adresáře, takže je můžeš spustit odkudkoli (dvojklikem
i z terminálu):

| skript | backend | co potřebuje navíc |
| --- | --- | --- |
| `run-dev.bat` | Vulkan (NVIDIA/AMD/Intel) | Vulkan SDK |
| `run-dev-cpu.bat` | jen CPU | nic (stačí CMake + MSVC) |

Název `run-dev-local.bat` je vyhrazený pro tvůj vlastní launcher na míru stroji —
je v `.gitignore`, takže ho commit nesebere.

#### Jak se model rozloží mezi GPU a RAM

Počet vrstev na GPU se nenastavuje ručně. Při načtení modelu spočítá
`weave_infrastructure::llm::offload_plan` plán z velikosti souboru, GGUF
hlavičky (počet expertů a vrstev, rozměry pro odhad KV cache) a **volné** VRAM
zjištěné přes ggml (vidí i AMD a Intel, ne jen NVIDII):

| plán | kdy | co se stane |
| --- | --- | --- |
| `FullGpu` | model se vejde do VRAM | všechny vrstvy na GPU |
| `HybridMoe` | MoE větší než VRAM | všechny vrstvy na GPU, **tenzory expertů v RAM** |
| `PartialLayers` | hustý model větší než VRAM | na GPU jde tolik vrstev, kolik se vejde |
| `Cpu` | není použitelná GPU | vše na CPU |

Naivní `-ngl 99` u modelu, který se nevejde, končí OOM nebo (na Windows/WDDM)
přetečením do RAM přes PCIe — a to je pomalejší než čistý CPU. Naivní „offloadni
N vrstev" je u MoE špatně taky: do VRAM se dostanou i experti, kteří se pro
každý token mění. Naměřeno na Gemma 4 26B A4B (16GB soubor, 8GB VRAM):

| konfigurace | tok/s |
| --- | --- |
| všechno na CPU, laděná vlákna | 11,2 |
| naivní offload 12 vrstev | 7,1 |
| hybrid (experti v RAM) | **17,8** |

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
jen aplikaci — modely i ComfyUI se stahují až z aplikace podle toho, co uživatel
zapne.

Staví se **s `--features llm-vulkan`**, a to nutně: vestavěná inference je jediný
backend, který appka má, takže build bez ní vyrobí aplikaci, která se nainstaluje
a spustí, ale na první zprávu odpoví, že není nastavený žádný AI model. Vulkan SDK
přitom potřebuje jen stroj, který instalátor staví — uživatel ne, runtime
`vulkan-1.dll` je součástí ovladače grafiky. Totéž platí pro `release.yml`
(Windows staví s Vulkanem, macOS s Metalem).

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
