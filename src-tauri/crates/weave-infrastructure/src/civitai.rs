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

/// Vybere z odpovědi první model se stažitelným .safetensors souborem.
/// Čistá funkce kvůli testům.
fn pick_lora(response: SearchResponse) -> Option<LoraInfo> {
    for item in response.items {
        for version in &item.model_versions {
            let file = version.files.iter().find(|f| {
                f.name.to_ascii_lowercase().ends_with(".safetensors") && f.download_url.is_some()
            });
            if let Some(file) = file {
                return Some(LoraInfo {
                    name: item.name.clone(),
                    file_name: file.name.clone(),
                    download_url: file.download_url.clone().expect("filtrováno výše"),
                    trigger_words: version.trained_words.clone(),
                });
            }
        }
    }
    None
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

#[async_trait]
impl LoraCatalogPort for CivitAiClient {
    async fn find_lora(&self, query: &str, base_model: &str) -> AppResult<Option<LoraInfo>> {
        let url = format!("{}/api/v1/models", self.base_url);
        let resp = self
            .http
            .get(&url)
            .query(&[
                ("limit", "5"),
                ("types", "LORA"),
                ("sort", "Highest Rated"),
                ("query", query),
                ("baseModels", base_model),
            ])
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

        Ok(pick_lora(parsed).map(|mut lora| {
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
            .http
            .get(&url)
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
                    "name": "Bez souboru",
                    "modelVersions": [ { "trainedWords": [], "files": [] } ]
                },
                {
                    "name": "Nikol Style",
                    "modelVersions": [
                        {
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
            .and(query_param("baseModels", "Pony"))
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

    #[tokio::test]
    async fn propagates_http_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/models"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let client = CivitAiClient::with_base_url(server.uri(), None);
        assert!(client.find_lora("x", "Pony").await.is_err());
    }
}
