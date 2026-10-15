# Vydávání aktualizací (release + auto-update)

Weave se aktualizuje přes **[UpdateHub](https://github.com/Elerdir/updatehub)**
(self-hosted update server na `https://updatehub.niderle.cz`) a vestavěný
Tauri updater. Hlavní okno se po startu (a pak každých 6 hodin) potichu
zeptá UpdateHubu na novou verzi; když nějaká je, ukáže v pravém dolním rohu
lištu **Je dostupná nová verze → Stáhnout a nainstalovat → Restartovat**.
Ručně jde kontrolu spustit v Nastavení → Aktualizace. Klient ověřuje Ed25519
podpis, takže instalátor bez platného podpisu nenainstaluje.

Automatická kontrola neběží v dev režimu (`pnpm tauri dev`) a její chyby
(offline, výpadek UpdateHubu) se jen logují — uživatele neobtěžují.

## Jednorázové nastavení

### 1. Endpoint a veřejný klíč (hotovo)

`src-tauri/tauri.conf.json` už míří na UpdateHub:

```json
"endpoints": [
  "https://updatehub.niderle.cz/api/apps/weave/tauri/latest.json?channel=stable"
]
```

> **Instalace postavené před touto změnou se samy neaktualizují** — mají
> zapečený starý placeholder endpoint i starý veřejný klíč. Na nové verzi je
> potřeba je jednou přeinstalovat ručně; od té doby už jedou přes updater.

### 2. Podpisový klíč updateru

Klíč leží mimo repozitář:

| soubor | obsah |
|---|---|
| `%USERPROFILE%\.tauri\weave-updater.key` | privátní klíč (šifrovaný heslem) |
| `%USERPROFILE%\.tauri\weave-updater.key.pub` | veřejný klíč = `pubkey` v `tauri.conf.json` |
| `%USERPROFILE%\.tauri\weave-updater-password.clixml` | heslo přes DPAPI (`Export-Clixml`) — přečte ho jen tenhle uživatel na tomhle stroji |

Heslo se dá vypsat (třeba pro GitHub Secret) takhle:

```powershell
$s = Import-Clixml "$env:USERPROFILE\.tauri\weave-updater-password.clixml"
[Runtime.InteropServices.Marshal]::PtrToStringAuto([Runtime.InteropServices.Marshal]::SecureStringToBSTR($s))
```

**Klíč si zálohuj** (privátní klíč + heslo, třeba do správce hesel). Když se
ztratí, nejde podepsat další verzi a všechny nainstalované kopie se musí
přeinstalovat ručně — přesně to se stalo s původním klíčem, ke kterému se
privátní část nedochovala.

Toto je **updater podpis** (Ed25519), ne code-signing certifikát
(Authenticode). Instalátor tedy není podepsaný certifikátem a Windows
SmartScreen může varovat.

### 3. Zaregistrovat aplikaci v UpdateHubu

Admin UI → Applications → **+ New Application** → slug `weave`, název `Weave`.
Pak na detailu appky získej **CI token**.

### 4. GitHub Secrets (repo Elerdir/Weave) — pro release z CI

| Secret | Hodnota |
|---|---|
| `TAURI_SIGNING_PRIVATE_KEY` | obsah `weave-updater.key` |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | heslo (viz výš) |
| `UPDATEHUB_URL` | `https://updatehub.niderle.cz` |
| `UPDATEHUB_CI_TOKEN` | CI token z UpdateHubu |

### 5. Osobní web

Na www.niderle.cz v adminu přidej aplikaci Weave a do pole UpdateHub slug
vyplň `weave`. Web si pak verzi i instalátory bere z
`/api/apps/weave/latest` sám.

## Vydání nové verze

1. Zvedni `version` v `src-tauri/tauri.conf.json` (a `package.json`).
2. Commitni, mergni do `main`.
3. Postav instalátor — podle toho, kterou cestou (viz níž).
4. V admin UI UpdateHubu release zkontroluj a klikni **Publish**.
5. Běžící klienti nabídnou aktualizaci do 6 hodin, nově spuštění do pár
   sekund po startu.

### Windows: CUDA + Vulkan lokálně (`instalator.bat`)

`instalator.bat` staví kombinovaný instalátor, který si backend vybere podle
grafiky (viz README). Když najde `weave-updater.key`, podepíše ho a vedle
`*-setup.exe` vznikne i `*-setup.exe.sig`. Obojí nahraje do UpdateHubu
skript (release vznikne jako Draft):

```powershell
.\scripts\publish-updatehub.ps1 -SaveToken          # poprvé: uloží CI token (DPAPI)
.\scripts\publish-updatehub.ps1 -Notes "Co je nového…"
```

Token se ukládá do `%USERPROFILE%\.updatehub\weave-token.clixml`. Ručně to
jde i přes admin UI (platforma `windows`, arch `x64`, podpis = obsah `.sig`).

Aby CUDA instalace zůstaly na CUDA, musí aktualizační balíček nést CUDA větev:
updater spustí instalátor potichu, hook přečte dřívější volbu z registru a
CUDA větev nechá — ale jen když ji balíček obsahuje. Stejný kombinovaný
instalátor teď staví i CI (viz níž), takže lokální build je jen záložní cesta.

### Přes Dispatch (https://dispatch.niderle.cz) — Windows (CUDA + Vulkan) — doporučeno

Repo žije na Forgeju (`https://git.niderle.cz/elerdir/Weave`), GitHub je jen zrcadlo. Dispatch spustí
`.forgejo/workflows/dispatch.yml` (Forgejo Actions) s verzí a platformami. Ten v jednom běhu:

1. ověří vstupy a že tag ještě neexistuje,
2. spustí `scripts/bump-version.mjs` (package.json, tauri.conf.json, Cargo.toml, Cargo.lock),
3. commit s novou verzí vytvoří **jen pod tagem `v<verze>`** (odpojený od větve) a tag pushne účet
   `release-bot` (secret organizace `RELEASE_TOKEN`). `main` je chráněná; soubory v ní se dorovnají v PR,
4. job `windows` na Windows runneru (VM na NUC) tag postaví složenou akcí
   `.forgejo/actions/windows-installer`, podepíše a nahraje do UpdateHubu jako **Draft**.

Poznámky k vydání se skládají z git logu od předchozího tagu (`scripts/release-notes.sh`):
bez merge commitů a bez `chore:`/`ci:`/`docs:`/`test:`, prefixy `feat:`/`fix:` a čísla PR se odstraní.
Piš proto předměty commitů srozumitelně pro uživatele.

Windows staví **tentýž kombinovaný instalátor jako lokální `instalator.bat`** (`instalator.bat /nopause`).
CUDA Toolkit, Vulkan SDK, Ninja, CMake a LLVM jsou ve VM nainstalované natrvalo (instalace při každém
buildu by trvala déle než build); akce je na začátku ověří. Instalátor si při instalaci vybere GPU backend
(NVIDIA s ≥ 20 GB VRAM → CUDA, 12–19 GB → zeptá se, jinak Vulkan), takže aktualizace nepřepne CUDA
instalace na Vulkan. Počítej s **1–2 hodinami buildu** (llama.cpp se kompiluje dvakrát, VM má 4 vCPU)
a ~1 GB instalátorem, který se nahrává do UpdateHubu s opakováním. CUDA musí umět architektury 86, 89
a 120 (RTX 30xx/40xx/50xx); `NVCC_APPEND_FLAGS=-allow-unsupported-compiler` je pojistka pro novější MSVC.

**macOS** (Apple Silicon: `.app.tar.gz` pro updater + `.dmg`) se vrátí s Mac runnerem.

### Tag bez Dispatche / opakování

`git tag v0.3.0 && git push origin v0.3.0` (nebo Actions → release → Spustit se vstupem `tag`) spustí
`.forgejo/workflows/release.yml`: Windows instalátor do UpdateHubu jako **Draft**. Verze v `package.json`,
`tauri.conf.json` a `Cargo.toml` se musí shodovat s tagem. Tagy od `release-bot` (z Dispatche) release.yml
přeskakuje — ty už staví dispatch.yml.

## Poznámky

- Instalátor je **NSIS** v režimu `currentUser` (bez admin práv → funguje i
  auto-update). MSI do UpdateHubu nepatří.
- `createUpdaterArtifacts` zajistí, že se k instalátoru vygeneruje `.sig`;
  `instalator.bat` ho zapíná jen tehdy, když má klíč.
