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
