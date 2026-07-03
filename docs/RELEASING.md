# Vydávání aktualizací (release + auto-update)

Weave se aktualizuje přes **[UpdateHub](https://github.com/Elerdir/updatehub)**
(self-hosted update server) a vestavěný Tauri updater. Klient stáhne manifest
z UpdateHubu, ověří Ed25519 podpis a nabídne aktualizaci v Nastavení →
Aktualizace.

## Jednorázové nastavení

### 1. Nasadit UpdateHub na veřejnou HTTPS adresu

Tauri updater **vyžaduje HTTPS** — přes `http://` (i localhost) se manifest
načíst nedá a aktualizace neproběhne. Nasaď UpdateHub (viz jeho
`docs/deployment/`) a nastav `UPDATEHUB_BASE_URL` na svou doménu, např.
`https://updates.tvujserver.cz`.

### 2. Přepsat endpoint v `src-tauri/tauri.conf.json`

Placeholder `https://updates.weave.app/...` swapni za svou doménu:

```json
"endpoints": [
  "https://updates.TVUJSERVER/api/apps/weave/tauri/latest.json?channel=stable"
]
```

### 3. Zaregistrovat aplikaci v UpdateHubu

Admin UI → Applications → **+ New Application** → slug `weave`, název `Weave`.
Pak na detailu appky (nebo v Settings) získej **CI token**.

### 4. Nastavit GitHub Secrets (repo Elerdir/Weave)

| Secret | Hodnota |
|---|---|
| `TAURI_SIGNING_PRIVATE_KEY` | Obsah privátního klíče updateru (`weave.key`) |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | Heslo ke klíči (prázdné, pokud bez hesla) |
| `UPDATEHUB_URL` | `https://updates.TVUJSERVER` |
| `UPDATEHUB_CI_TOKEN` | CI token z UpdateHubu |

> **Podpisový klíč updateru** (`weave.key` + `weave.key.pub`) byl vygenerován
> přes `pnpm tauri signer generate`. Veřejný klíč (`pubkey`) je už v
> `tauri.conf.json`. Privátní klíč **nikdy necommituj** — patří jen do
> GitHub Secrets. Bez něj/hesla nepůjde podepsat další aktualizace.
>
> Toto je **updater podpis** (Ed25519), ne code-signing certifikát
> (Authenticode). Instalátor tedy zatím není podepsaný certifikátem a
> Windows SmartScreen může varovat — to řešíme později.

## Vydání nové verze

1. Zvedni `version` v `src-tauri/tauri.conf.json` (a `package.json`).
2. Commitni, mergni do `main`.
3. Otaguj: `git tag v0.2.0 && git push origin v0.2.0`.
4. Workflow **release** (`.github/workflows/release.yml`) sestaví Windows
   instalátor, podepíše ho a nahraje do UpdateHubu jako **Draft**.
5. V admin UI UpdateHubu release zkontroluj a klikni **Publish**.
6. Klienti dostanou aktualizaci při další kontrole (Nastavení → Aktualizace,
   tlačítko „Zkontrolovat aktualizace").

Alternativně jde workflow spustit ručně (Actions → release → Run workflow)
se zadanou verzí — užitečné pro test bez tagu.

## Poznámky

- Instalátor je **NSIS** v režimu `currentUser` (bez admin práv → funguje i
  auto-update). Vzniká i MSI (per-machine). Do UpdateHubu se nahrává NSIS.
- `createUpdaterArtifacts: true` v `tauri.conf.json` zajistí, že se k
  instalátoru vygeneruje `.sig` (podpis pro updater).
