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

### Windows: CUDA + Vulkan (`instalator.bat`) — doporučeno

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

Tohle je jediná cesta, po které CUDA instalace zůstanou na CUDA: updater
spustí instalátor potichu, hook přečte dřívější volbu z registru a nechá
CUDA větev — ale jen když ji balíček nese.

### Windows: jen Vulkan (CI)

`git tag v0.2.0 && git push origin v0.2.0` spustí `.github/workflows/release.yml`:
Vulkan instalátor + macOS, podepíše a nahraje do UpdateHubu jako **Draft**.
Pozor: CUDA instalace by se touhle verzí přepnuly na Vulkan (appka běží dál,
jen pomaleji). Když vydáváš Windows přes `instalator.bat`, Windows build z CI
do UpdateHubu nepublikuj.

Workflow jde spustit i ručně (Actions → release → Run workflow) se zadanou
verzí — užitečné pro test bez tagu.

## Poznámky

- Instalátor je **NSIS** v režimu `currentUser` (bez admin práv → funguje i
  auto-update). MSI do UpdateHubu nepatří.
- `createUpdaterArtifacts` zajistí, že se k instalátoru vygeneruje `.sig`;
  `instalator.bat` ho zapíná jen tehdy, když má klíč.
