#!/usr/bin/env bash
# Vypise "co je noveho" pro dany tag z CHANGELOG.md (bez nadpisu sekce) - jde do UpdateHubu,
# k aktualizaci v aplikaci a na www.niderle.cz. Text je pro uzivatele, ne predmety commitu.
#
# Commit s verzi zije jen pod tagem (main je chranena), takze sekce `## [X.Y.Z]` casto jeste
# neexistuje. Poradi: sekce verze -> [Unreleased] -> jedna radka s nazvem verze.
#
# Pouziti: scripts/release-notes.sh v0.3.0 > release-notes.md
set -euo pipefail

tag="${1:?usage: release-notes.sh <tag>}"
version="${tag#v}"
changelog="$(dirname "$0")/../CHANGELOG.md"

section() {
  # Doslovne porovnani (index), ne regex - hranate zavorky a tecky ve verzi by se musely escapovat.
  awk -v hdr="## $1" '
    index($0, hdr) == 1 { on = 1; next }
    on && (/^## / || /^---/) { exit }
    on { print }
  ' "$changelog" | sed -e '/./,$!d' | sed -e ':a' -e '/^\n*$/{$d;N;ba' -e '}'
}

notes=""
[ -f "$changelog" ] && notes="$(section "[$version]")"
[ -z "$notes" ] && [ -f "$changelog" ] && notes="$(section "[Unreleased]")"
[ -z "$notes" ] && notes="Weave $version"
printf '%s\n' "$notes"
