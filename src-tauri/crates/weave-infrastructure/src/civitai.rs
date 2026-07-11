//! CivitAI katalog LoRA modelů. Hledání jede bez přihlášení; stahování
//! některých modelů vyžaduje API token — když je v keychain, přidá se
//! k download URL jako `?token=…` (oficiálně podporovaný způsob).

use async_trait::async_trait;
use serde::Deserialize;
use weave_application::{
    error::{AppError, AppResult},
    ports::lora_catalog_port::{CatalogBrowseItem, ImageModelKind, LoraCatalogPort, LoraInfo},
};

pub struct CivitAiClient {
    http: reqwest::Client,
    base_url: String,
    token: Option<String>,
}

impl CivitAiClient {
    pub fn new(token: Option<String>) -> Self {
        Self::with_base_url("https://civitai.com", token)
    }

    pub fn with_base_url(base_url: impl Into<String>, token: Option<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            base_url: base_url.into(),
            token: token.filter(|t| !t.is_empty()),
        }
    }
}

#[derive(Deserialize)]
struct SearchResponse {
    items: Vec<ModelItem>,
}

#[derive(Deserialize)]
struct ModelItem {
    #[serde(default)]
    id: u64,
    name: String,
    #[serde(default)]
    nsfw: bool,
    #[serde(default)]
    creator: Option<Creator>,
    #[serde(default)]
    stats: Option<Stats>,
    #[serde(rename = "modelVersions", default)]
    model_versions: Vec<ModelVersion>,
}

#[derive(Deserialize, Default)]
struct Creator {
    #[serde(default)]
    username: String,
}

#[derive(Deserialize, Default)]
struct Stats {
    #[serde(rename = "downloadCount", default)]
    download_count: u64,
}

#[derive(Deserialize)]
struct ModelVersion {
    #[serde(rename = "baseModel", default)]
    base_model: String,
    #[serde(rename = "trainedWords", default)]
    trained_words: Vec<String>,
    #[serde(default)]
    images: Vec<VersionImage>,
    #[serde(default)]
    files: Vec<ModelFile>,
}

#[derive(Deserialize)]
struct VersionImage {
    #[serde(default)]
    url: String,
}

#[derive(Deserialize)]
struct ModelFile {
    name: String,
    #[serde(rename = "downloadUrl")]
    download_url: Option<String>,
    #[serde(rename = "sizeKB", default)]
    size_kb: f64,
}

/// SDXL rodina architektur — LoRA z těchto bází se do SDXL workflow načtou
/// (Pony/Illustrious/NoobAI jsou doladěné SDXL). SD 1.x/2.x/Flux LoRA by se
/// nenačetly správně, ty se přeskakují.
const SDXL_FAMILY: &[&str] = &[
    "SDXL 1.0",
    "SDXL 0.9",
    "Pony",
    "Illustrious",
    "NoobAI",
    "SDXL Turbo",
    "SDXL Lightning",
];

/// Vybere nejvhodnější LoRA. Bere jen SDXL rodinu (kompatibilní
/// architektura) a řadí podle: 1) shoda jména s dotazem (tag hledání vrací
/// i tangenciální modely — „ahsoka tano" štítek nese třeba Star Wars lokace),
/// 2) přesně požadovaná báze (`preferred_base`) před ostatními z rodiny,
/// 3) pořadí z katalogu (řazeno podle stažení). Čistá funkce kvůli testům.
fn pick_lora(items: &[ModelItem], query: &str, preferred_base: &str) -> Option<LoraInfo> {
    let query_tokens: Vec<String> = query
        .to_lowercase()
        .split_whitespace()
        .map(str::to_string)
        .collect();

    let mut best: Option<((usize, usize), LoraInfo)> = None;
    for item in items {
        let name = item.name.to_lowercase();
        let name_rank = usize::from(
            query_tokens.is_empty() || !query_tokens.iter().all(|t| name.contains(t.as_str())),
        );
        for version in &item.model_versions {
            if !SDXL_FAMILY.contains(&version.base_model.as_str()) {
                continue;
            }
            let Some(file) = version.files.iter().find(|f| {
                f.name.to_ascii_lowercase().ends_with(".safetensors") && f.download_url.is_some()
            }) else {
                continue;
            };
            let rank = (name_rank, usize::from(version.base_model != preferred_base));
            if best.as_ref().is_none_or(|(r, _)| rank < *r) {
                let info = LoraInfo {
                    name: item.name.clone(),
                    file_name: file.name.clone(),
                    download_url: file.download_url.clone().expect("filtrováno výše"),
                    trigger_words: version.trained_words.clone(),
                };
                let done = rank == (0, 0);
                best = Some((rank, info));
                if done {
                    return best.map(|(_, i)| i);
                }
            }
        }
    }
    best.map(|(_, i)| i)
}

impl CivitAiClient {
    /// Token do download URL — hledání ho nepotřebuje, stažení často ano
    /// (oficiálně podporovaný `?token=` způsob).
    fn with_token(&self, url: String) -> String {
        match &self.token {
            Some(token) => {
                let sep = if url.contains('?') { '&' } else { '?' };
                format!("{url}{sep}token={token}")
            }
            None => url,
        }
    }
}

/// Převede položku vyhledávání na kartu prohlížeče: bere první verzi se
/// stažitelným .safetensors souborem. Čistá funkce kvůli testům.
fn to_browse_item(item: &ModelItem, kind: ImageModelKind) -> Option<CatalogBrowseItem> {
    for version in &item.model_versions {
        let file = version.files.iter().find(|f| {
            f.name.to_ascii_lowercase().ends_with(".safetensors") && f.download_url.is_some()
        });
        let Some(file) = file else { continue };
        return Some(CatalogBrowseItem {
            name: item.name.clone(),
            creator: item
                .creator
                .as_ref()
                .map(|c| c.username.clone())
                .unwrap_or_default(),
            kind,
            base_model: version.base_model.clone(),
            preview_image_url: version
                .images
                .iter()
                .map(|i| i.url.clone())
                .find(|u| !u.is_empty()),
            downloads: item.stats.as_ref().map(|s| s.download_count).unwrap_or(0),
            nsfw: item.nsfw,
            file_name: file.name.clone(),
            download_url: file.download_url.clone().expect("filtrováno výše"),
            size_bytes: (file.size_kb * 1024.0) as u64,
            trigger_words: version.trained_words.clone(),
        });
    }
    None
}

/// Kolikrát zkusit hledání znovu při přechodné chybě (503/429). CivitAI je
/// notoricky nespolehlivé a rate-limituje — jeden 503 nesmí shodit celé
/// generování.
const SEARCH_ATTEMPTS: u32 = 3;

impl CivitAiClient {
    /// Přidá `Authorization: Bearer <token>`, pokud ho máme. Autentizované
    /// dotazy mají u CivitAI vyšší rate limity a spolehlivější přístup
    /// k NSFW obsahu — právě tam bez tokenu chodí 503.
    fn authed(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.token {
            Some(token) => req.bearer_auth(token),
            None => req,
        }
    }

    /// Jeden dotaz na `/api/v1/models` s daným vyhledávacím parametrem
    /// (`query` = fulltext, `tag` = přesný štítek). `nsfw=true` je nutné —
    /// API jinak NSFW modely tiše skrývá (u LoRA postav velmi časté).
    /// Retry na 503/429 s narůstajícím backoffem.
    async fn search_loras(&self, param: (&str, &str)) -> AppResult<Vec<ModelItem>> {
        let url = format!("{}/api/v1/models", self.base_url);
        let mut last_err = String::new();
        for attempt in 1..=SEARCH_ATTEMPTS {
            let resp = self
                .authed(self.http.get(&url))
                .query(&[
                    ("limit", "15"),
                    ("types", "LORA"),
                    ("sort", "Most Downloaded"),
                    ("nsfw", "true"),
                    param,
                ])
                .send()
                .await;

            match resp {
                Ok(resp) if resp.status().is_success() => {
                    let parsed: SearchResponse = resp.json().await.map_err(|e| {
                        AppError::ComfyUi(format!("CivitAI odpověď nejde přečíst: {e}"))
                    })?;
                    return Ok(parsed.items);
                }
                // 503/429 (a 5xx obecně) = přechodné → zkusit znovu.
                Ok(resp) if resp.status().is_server_error() || resp.status().as_u16() == 429 => {
                    last_err = format!("HTTP {}", resp.status());
                }
                // Ostatní HTTP chyby (4xx mimo 429) nemá smysl opakovat.
                Ok(resp) => {
                    return Err(AppError::ComfyUi(format!(
                        "CivitAI hledání selhalo: HTTP {}",
                        resp.status()
                    )));
                }
                Err(e) => last_err = e.to_string(),
            }

            if attempt < SEARCH_ATTEMPTS {
                tracing::warn!(attempt, error = %last_err, "CivitAI hledání — přechodná chyba, zkouším znovu");
                tokio::time::sleep(std::time::Duration::from_millis(800 * u64::from(attempt)))
                    .await;
            }
        }
        Err(AppError::ComfyUi(format!(
            "CivitAI hledání selhalo po {SEARCH_ATTEMPTS} pokusech: {last_err}"
        )))
    }
}

#[async_trait]
impl LoraCatalogPort for CivitAiClient {
    async fn find_lora(&self, query: &str, base_model: &str) -> AppResult<Option<LoraInfo>> {
        // Fulltext `query` je na CivitAI překvapivě slabý (např. „ahsoka"
        // nenajde nic) — `tag` hledání bývá výrazně bohatší. Kombinují se
        // oba zdroje; kompatibilitu architektury (SDXL rodina) a preferenci
        // base modelu řeší až pick_lora, serverový `baseModels` filtr by
        // zahodil použitelné Pony/Illustrious LoRA.
        // Obě hledání jsou tolerantní — CivitAI běžně na jednom endpointu
        // vrátí 503. Použije se, co projde; chyba se hlásí jen když selžou
        // OBĚ (jinak by přechodný 503 na `query` zbytečně shodil celé hledání,
        // i kdyby `tag` vrátilo výsledky).
        let lower = query.to_lowercase();
        let (query_result, tag_result) = tokio::join!(
            self.search_loras(("query", query)),
            self.search_loras(("tag", &lower)),
        );

        let mut items: Vec<ModelItem> = Vec::new();
        let mut push_unique = |source: Vec<ModelItem>| {
            for item in source {
                if !items.iter().any(|i| i.id == item.id) {
                    items.push(item);
                }
            }
        };
        match (query_result, tag_result) {
            (Ok(q), Ok(t)) => {
                push_unique(q);
                push_unique(t);
            }
            (Ok(q), Err(e)) => {
                tracing::warn!("CivitAI tag hledání selhalo ({e}) — pokračuji jen s query");
                push_unique(q);
            }
            (Err(e), Ok(t)) => {
                tracing::warn!("CivitAI query hledání selhalo ({e}) — pokračuji jen s tag");
                push_unique(t);
            }
            (Err(e), Err(_)) => return Err(e),
        }

        Ok(pick_lora(&items, query, base_model).map(|mut lora| {
            lora.download_url = self.with_token(lora.download_url);
            lora
        }))
    }

    async fn browse(
        &self,
        query: &str,
        kind: ImageModelKind,
        base_model: Option<&str>,
    ) -> AppResult<Vec<CatalogBrowseItem>> {
        let url = format!("{}/api/v1/models", self.base_url);
        let mut params: Vec<(&str, String)> = vec![
            ("limit", "20".into()),
            ("types", kind.api_type().into()),
            ("sort", "Most Downloaded".into()),
            // Uživatel generuje i 18+ obsah — NSFW modely se nefiltrují,
            // v UI se jen označí štítkem.
            ("nsfw", "true".into()),
            ("query", query.trim().into()),
        ];
        if let Some(base) = base_model {
            params.push(("baseModels", base.into()));
        }
        let resp = self
            .authed(self.http.get(&url))
            .query(&params)
            .send()
            .await
            .map_err(|e| AppError::ComfyUi(format!("CivitAI hledání selhalo: {e}")))?;

        if !resp.status().is_success() {
            return Err(AppError::ComfyUi(format!(
                "CivitAI hledání selhalo: HTTP {}",
                resp.status()
            )));
        }

        let parsed: SearchResponse = resp
            .json()
            .await
            .map_err(|e| AppError::ComfyUi(format!("CivitAI odpověď nejde přečíst: {e}")))?;

        Ok(parsed
            .items
            .iter()
            .filter_map(|item| to_browse_item(item, kind))
            .map(|mut item| {
                item.download_url = self.with_token(item.download_url);
                item
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn sample_body() -> serde_json::Value {
        serde_json::json!({
            "items": [
                {
                    "id": 1,
                    "name": "Bez souboru",
                    "modelVersions": [ { "baseModel": "Pony", "trainedWords": [], "files": [] } ]
                },
                {
                    "id": 2,
                    "name": "Nikol Style",
                    "modelVersions": [
                        {
                            "baseModel": "Pony",
                            "trainedWords": ["nikol woman"],
                            "files": [
                                { "name": "readme.txt", "downloadUrl": "https://cdn/x.txt" },
                                { "name": "nikol_v1.safetensors", "downloadUrl": "https://cdn/nikol_v1.safetensors" }
                            ]
                        }
                    ]
                }
            ]
        })
    }

    #[tokio::test]
    async fn finds_first_lora_with_safetensors_file() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/models"))
            .and(query_param("types", "LORA"))
            .and(query_param("nsfw", "true"))
            .respond_with(ResponseTemplate::new(200).set_body_json(sample_body()))
            .mount(&server)
            .await;

        let client = CivitAiClient::with_base_url(server.uri(), None);
        let lora = client.find_lora("nikol", "Pony").await.unwrap().unwrap();

        assert_eq!(lora.name, "Nikol Style");
        assert_eq!(lora.file_name, "nikol_v1.safetensors");
        assert_eq!(lora.download_url, "https://cdn/nikol_v1.safetensors");
        assert_eq!(lora.trigger_words, vec!["nikol woman"]);
    }

    #[tokio::test]
    async fn appends_token_to_download_url() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_json(sample_body()))
            .mount(&server)
            .await;

        let client = CivitAiClient::with_base_url(server.uri(), Some("tajny-token".into()));
        let lora = client.find_lora("nikol", "Pony").await.unwrap().unwrap();
        assert_eq!(
            lora.download_url,
            "https://cdn/nikol_v1.safetensors?token=tajny-token"
        );
    }

    #[tokio::test]
    async fn returns_none_when_nothing_downloadable() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/models"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({ "items": [] })),
            )
            .mount(&server)
            .await;

        let client = CivitAiClient::with_base_url(server.uri(), None);
        assert!(client.find_lora("nic", "Pony").await.unwrap().is_none());
    }

    fn browse_body() -> serde_json::Value {
        serde_json::json!({
            "items": [
                {
                    "name": "RealVis Ultra",
                    "nsfw": true,
                    "creator": { "username": "sg161222" },
                    "stats": { "downloadCount": 12345 },
                    "modelVersions": [
                        {
                            "baseModel": "SDXL 1.0",
                            "trainedWords": [],
                            "images": [ { "url": "https://img.civitai/preview.jpg" } ],
                            "files": [
                                { "name": "realvis_ultra.safetensors",
                                  "downloadUrl": "https://cdn/realvis_ultra.safetensors",
                                  "sizeKB": 2048.0 }
                            ]
                        }
                    ]
                },
                {
                    "name": "Bez souboru",
                    "modelVersions": [ { "baseModel": "SDXL 1.0", "files": [] } ]
                }
            ]
        })
    }

    #[tokio::test]
    async fn browse_maps_items_and_filters_undownloadable() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/models"))
            .and(query_param("types", "Checkpoint"))
            .and(query_param("nsfw", "true"))
            .respond_with(ResponseTemplate::new(200).set_body_json(browse_body()))
            .mount(&server)
            .await;

        let client = CivitAiClient::with_base_url(server.uri(), None);
        let items = client
            .browse("realvis", ImageModelKind::Checkpoint, None)
            .await
            .unwrap();

        // Položka bez stažitelného souboru se vynechá
        assert_eq!(items.len(), 1);
        let item = &items[0];
        assert_eq!(item.name, "RealVis Ultra");
        assert_eq!(item.creator, "sg161222");
        assert_eq!(item.base_model, "SDXL 1.0");
        assert_eq!(
            item.preview_image_url.as_deref(),
            Some("https://img.civitai/preview.jpg")
        );
        assert_eq!(item.downloads, 12345);
        assert!(item.nsfw);
        assert_eq!(item.file_name, "realvis_ultra.safetensors");
        assert_eq!(item.size_bytes, 2048 * 1024);
    }

    #[tokio::test]
    async fn browse_appends_token_and_base_model_filter() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/models"))
            .and(query_param("types", "LORA"))
            .and(query_param("baseModels", "Pony"))
            .respond_with(ResponseTemplate::new(200).set_body_json(browse_body()))
            .mount(&server)
            .await;

        let client = CivitAiClient::with_base_url(server.uri(), Some("tajny".into()));
        let items = client
            .browse("x", ImageModelKind::Lora, Some("Pony"))
            .await
            .unwrap();

        assert_eq!(
            items[0].download_url,
            "https://cdn/realvis_ultra.safetensors?token=tajny"
        );
        assert_eq!(items[0].kind, ImageModelKind::Lora);
    }

    #[test]
    fn pick_lora_prefers_exact_base_and_skips_incompatible() {
        let items: Vec<ModelItem> = serde_json::from_value(serde_json::json!([
            {
                "id": 1,
                "name": "SD15 only",
                "modelVersions": [
                    { "baseModel": "SD 1.5", "trainedWords": [],
                      "files": [ { "name": "old.safetensors", "downloadUrl": "https://cdn/old" } ] }
                ]
            },
            {
                "id": 2,
                "name": "Illustrious verze",
                "modelVersions": [
                    { "baseModel": "Illustrious", "trainedWords": ["ahsoka"],
                      "files": [ { "name": "ill.safetensors", "downloadUrl": "https://cdn/ill" } ] }
                ]
            },
            {
                "id": 3,
                "name": "Presna baze",
                "modelVersions": [
                    { "baseModel": "SDXL 1.0", "trainedWords": ["ahsoka tano"],
                      "files": [ { "name": "sdxl.safetensors", "downloadUrl": "https://cdn/sdxl" } ] }
                ]
            }
        ]))
        .unwrap();

        // Bez jmenné shody rozhoduje báze: přesná vyhrává nad Illustrious
        let picked = pick_lora(&items, "zzz", "SDXL 1.0").unwrap();
        assert_eq!(picked.file_name, "sdxl.safetensors");

        // Bez přesné shody se vezme první z SDXL rodiny (SD 1.5 se přeskočí)
        let picked = pick_lora(&items, "zzz", "Pony").unwrap();
        assert_eq!(picked.file_name, "ill.safetensors");

        // Jmenná shoda s dotazem přebíjí přesnou bázi — tag hledání vrací
        // i tangenciální modely (Star Wars lokace u štítku „ahsoka tano"),
        // postava v názvu je silnější signál než báze.
        let picked = pick_lora(&items, "Illustrious VERZE", "SDXL 1.0").unwrap();
        assert_eq!(picked.file_name, "ill.safetensors");

        // Jen nekompatibilní báze → nic
        let sd15only = &items[..1];
        assert!(pick_lora(sd15only, "zzz", "SDXL 1.0").is_none());
    }

    #[tokio::test]
    async fn find_lora_falls_back_to_tag_search() {
        // Fulltext `query` nic nenajde (reálná slabina CivitAI API),
        // `tag` hledání ano — výsledek se musí použít.
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/models"))
            .and(query_param("tag", "ahsoka tano"))
            .respond_with(ResponseTemplate::new(200).set_body_json(sample_body()))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/v1/models"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({ "items": [] })),
            )
            .mount(&server)
            .await;

        let client = CivitAiClient::with_base_url(server.uri(), None);
        let lora = client
            .find_lora("Ahsoka Tano", "Pony")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(lora.file_name, "nikol_v1.safetensors");
    }

    #[tokio::test]
    async fn find_lora_tolerates_503_on_one_endpoint() {
        // `query` endpoint trvale 503 (přechodná chyba CivitAI), `tag` projde
        // → LoRA se přesto najde, celé hledání se kvůli jednomu 503 neshodí.
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/models"))
            .and(query_param("tag", "ahsoka tano"))
            .respond_with(ResponseTemplate::new(200).set_body_json(sample_body()))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/v1/models"))
            .and(query_param("query", "Ahsoka Tano"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;

        let client = CivitAiClient::with_base_url(server.uri(), None);
        let lora = client
            .find_lora("Ahsoka Tano", "Pony")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(lora.file_name, "nikol_v1.safetensors");
    }

    #[tokio::test]
    async fn search_retries_transient_503_then_succeeds() {
        // První pokus 503, druhý 200 — retry to zvytáhne.
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/models"))
            .respond_with(ResponseTemplate::new(503))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_json(sample_body()))
            .mount(&server)
            .await;

        let client = CivitAiClient::with_base_url(server.uri(), None);
        let lora = client.find_lora("nikol", "Pony").await.unwrap().unwrap();
        assert_eq!(lora.file_name, "nikol_v1.safetensors");
    }

    #[tokio::test]
    async fn find_lora_errors_only_when_both_searches_fail() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/models"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;

        let client = CivitAiClient::with_base_url(server.uri(), None);
        assert!(client.find_lora("x", "Pony").await.is_err());
    }
}
