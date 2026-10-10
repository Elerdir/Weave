#!/usr/bin/env node
// Nastavi verzi Weave na vsech mistech, ktera musi byt v souladu:
//   package.json, src-tauri/tauri.conf.json, Cargo.toml [workspace.package], Cargo.lock (clenove workspace)
// Pouziti: node scripts/bump-version.mjs 0.3.0
// Bez zavislosti na PowerShellu — bezi stejne na Windows, macOS i Linux runneru.
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const version = process.argv[2];
if (!version || !/^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/.test(version)) {
  console.error("Pouziti: node scripts/bump-version.mjs <verze>   (napr. 0.3.0)");
  process.exit(1);
}

const read = (p) => readFileSync(join(root, p), "utf8");
const write = (p, s) => writeFileSync(join(root, p), s, "utf8");

/** Nahradi PRVNI vyskyt vzoru; kdyz nic nenajde, konci chybou (tichy nezdar by vydal spatnou verzi). */
function replaceFirst(file, pattern, replacement) {
  const text = read(file);
  if (!pattern.test(text)) throw new Error(`${file}: vzor ${pattern} nenalezen`);
  const next = text.replace(pattern, replacement);
  write(file, next);
  return text.match(pattern)[1];
}

const jsonVersion = /("version"\s*:\s*")([^"]+)(")/;
const oldPkg = read("package.json").match(jsonVersion)[2];
replaceFirst("package.json", jsonVersion, `$1${version}$3`);
replaceFirst("src-tauri/tauri.conf.json", jsonVersion, `$1${version}$3`);

const cargoVersion = /(\[workspace\.package\][^[]*?\nversion\s*=\s*")([^"]+)(")/;
const oldCargo = read("Cargo.toml").match(cargoVersion)[2];
replaceFirst("Cargo.toml", cargoVersion, `$1${version}$3`);

// Cargo.lock: radky `name = "weave-..."` + `version = "<stara>"` patri clenum workspace.
// Bez toho by `cargo build --locked` selhal na "lock file needs to be updated".
let lock = read("Cargo.lock");
let touched = 0;
lock = lock.replace(
  /(\[\[package\]\]\nname = "weave-[a-z-]+"\nversion = ")([^"]+)(")/g,
  (_, a, old, c) => {
    touched++;
    return `${a}${version}${c}`;
  },
);
if (touched === 0) throw new Error("Cargo.lock: zadny clen workspace (weave-*) nenalezen");
write("Cargo.lock", lock);

console.log(`package.json ${oldPkg} -> ${version}`);
console.log(`tauri.conf.json -> ${version}`);
console.log(`Cargo.toml ${oldCargo} -> ${version}`);
console.log(`Cargo.lock: ${touched} balicku weave-*`);
