#!/usr/bin/env bash
# Spustí Weave ve vývojovém režimu s vestavěnou Metal GPU inferencí (llama.cpp)
# na macOS s Apple Silicon (M1/M2/M3/M4…).
#
# Předpoklady:
#   - Xcode Command Line Tools:  xcode-select --install
#   - Homebrew balíčky:          brew install cmake node pnpm rustup
#   - Rust stable:               rustup default stable
#
# Model (.gguf) a počet GPU vrstev se nastavují v aplikaci:
#   Nastavení -> AI model -> Vestavěná GPU inference
# Na Apple Silicon nech "všechny vrstvy" — GPU sdílí unified memory s CPU.

set -euo pipefail
cd "$(dirname "$0")"

# sqlx používá commitnutou offline cache, DB není při buildu potřeba
export SQLX_OFFLINE=true

if ! command -v pnpm >/dev/null 2>&1; then
    echo "CHYBA: 'pnpm' nebyl nalezen v PATH (brew install pnpm)." >&2
    exit 1
fi

echo
echo "=== Weave dev (Metal build) ==="
echo

exec pnpm tauri dev --features llm-metal
