#!/usr/bin/env bash
# Poznamky k vydani pro UpdateHub z git logu: predmety commitu od predchoziho tagu
# (bez merge commitu a bez chore/ci/docs/test/build/style), prefixy feat:/fix: se
# odstrani. Weave nema CHANGELOG.md, a repo je verejne, ale poznamky maji byt cisty text.
# Bez predchoziho tagu se vezme poslednich 20 commitu.
#
# Pouziti: scripts/release-notes.sh v0.3.0 > release-notes.md   (tag musi existovat)
set -euo pipefail

tag="${1:?usage: release-notes.sh <tag>}"
version="${tag#v}"

prev=$(git tag --list 'v*' --sort=-v:refname | grep -vx "$tag" | head -n1 || true)
if [ -n "$prev" ]; then
  log=$(git log --no-merges --pretty='%s' "$prev..$tag")
else
  log=$(git log --no-merges --pretty='%s' -n 20 "$tag")
fi

notes=$(printf '%s\n' "$log" \
  | grep -viE '^(chore|ci|docs|test|tests|build|style|refactor)(\([^)]*\))?!?:' \
  | sed -E 's/^(feat|fix|perf)(\([^)]*\))?!?:[[:space:]]*//I' \
  | sed -E 's/[[:space:]]*\(#[0-9]+\)$//' \
  | sed -E '/^[[:space:]]*$/d' \
  | awk '!seen[$0]++' \
  | sed -E 's/^/- /' || true)

if [ -z "$notes" ]; then
  notes="Weave $version"
fi
printf '%s\n' "$notes"
