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

```bash
# NVIDIA přes CUDA — rychlejší tam, kde se model vejde do VRAM
pnpm tauri dev --features llm-cuda
```

**Vulkan je výchozí cesta**, protože běží na NVIDII, AMD i Intelu a runtime má
každý stroj v ovladači grafiky. CUDA je navíc pro NVIDII: u modelu, který se
vejde celý do VRAM, znatelně zrychlí zpracování promptu. U modelu většího než
VRAM na backendu nezáleží — tam rozhoduje rozložení modelu (viz níž), takže se
CUDA nevyplatí ani stavět. Backend je v llama.cpp zakompilovaný napevno, takže
jedna binárka mezi nimi za běhu přepnout neumí; instalátor to řeší tím, že nese
obě (viz [Instalátor pro Windows](#instalátor-pro-windows)).

Na Windows jsou na to připravené dávky v kořeni repozitáře — obě si samy
přepnou do svého adresáře, takže je můžeš spustit odkudkoli (dvojklikem
i z terminálu):

| skript | backend | co potřebuje navíc |
| --- | --- | --- |
| `run-dev.bat` | Vulkan (NVIDIA/AMD/Intel) | Vulkan SDK |
| `run-dev-cpu.bat` | jen CPU | nic (stačí CMake + MSVC) |
| `instalator.bat` | CUDA i Vulkan (instalátor) | Vulkan SDK + CUDA Toolkit |

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

**CUDA větev se nedělí.** Na NVIDII je karta typicky dost velká na celý model,
takže se místo dělení zkrátí kontext na to, co se do VRAM vejde vedle vah — a
když se nevejdou ani ty samotné, jde všechno na CPU. Rozdíl je vidět u modelů
s velkými hlavami: Gemma 4 má K/V hlavu 512 dimenzí, tedy 480 kB KV cache na
token, takže na 24GB kartě vyjde celý model na GPU do ~13 tisíc tokenů (Qwen3.8
s poloviční hlavou do ~25 tisíc). Zkrácení se zapíše do logu. Dělení vrstev
i hybridní MoE režim zůstávají Vulkanu, kde bývá karta menší a kompromis
dává smysl.

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

Vyrobí **jeden instalátor, který si GPU backend vybere sám při instalaci**
(`target/release/bundle/nsis/*-setup.exe`):

| co je ve stroji | co se nainstaluje |
| --- | --- |
| NVIDIA s 20 GB VRAM a víc | CUDA (3090, 4090, 5090 — model se vejde do VRAM) |
| NVIDIA s 12–19 GB VRAM | instalátor se zeptá |
| cokoli jiného | Vulkan (AMD, Intel, slabší NVIDIA) |

Rozhoduje `src-tauri/nsis/gpu-backend.nsh`. Kartu pozná přes `nvidia-smi`
(je součástí ovladače NVIDIE, takže jeho úspěšné spuštění je zároveň důkaz, že
NVIDIA ve stroji je). Volba se ukládá do registru, protože updater instaluje
tiše — bez toho by první aktualizace CUDA verzi potichu přepsala Vulkanem.

Cenou je velikost: backend je v llama.cpp zakompilovaný napevno, takže
instalátor nese **obě** binárky a k tomu cuBLAS + cudart (485 MB, jsou součástí
CUDA Toolkitu, ne ovladače, takže na cizím stroji nejsou). Nepotřebná větev se
při instalaci smaže, na disku tedy zůstane jen jedna. Build trvá přes hodinu —
llama.cpp se kompiluje dvakrát, pro CUDA navíc s kernely pro tři architektury
(`86;89;120` = RTX 30xx/40xx/50xx; slabší karty mají pod 12 GB VRAM, takže jim
instalátor CUDA větev stejně nenabídne).

Stroj, který instalátor **staví**, potřebuje Vulkan SDK i CUDA Toolkit. Stroj,
který ho spouští, nepotřebuje nic.

MSI tuhle detekci neumí (WiX nemá jednoduchý ekvivalent instalačních hooků),
takže se staví zvlášť a jen s Vulkanem:

```bat
pnpm tauri build --bundles msi --features llm-vulkan
```

Oficiální podepsané instalátory staví `release.yml` při tagu `v*` — ale zatím
**jen s Vulkanem**, protože CUDA Toolkit se na GitHub runner musí doinstalovat
a llama.cpp by se v jednom jobu kompilovala dvakrát. Kdo chce CUDA větev, musí
si instalátor postavit lokálně přes `instalator.bat`; aktualizace přes updater
takovou instalaci vrátí zpět na Vulkan (appka běží dál, jen bez CUDY).

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
