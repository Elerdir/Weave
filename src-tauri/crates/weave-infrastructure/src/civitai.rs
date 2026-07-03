//! CivitAI katalog LoRA modelů. Hledání jede bez přihlášení; stahování
//! některých modelů vyžaduje API token — když je v keychain, přidá se
//! k download URL jako `?token=…` (oficiálně podporovaný způsob).

use async_trait::async_trait;
use serde::Deserialize;
use weave_application::{
    error::{AppError, AppResult},
    ports::lora_catalog_port::{LoraCatalogPort, LoraInfo},
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
    #[serde(rename = "modelVersions", default)]
    model_versions: Vec<ModelVersion>,
}

#[derive(Deserialize)]
struct ModelVersion {
    #[serde(rename = "trainedWords", default)]
    trained_words: Vec<String>,
    #[serde(default)]
    files: Vec<ModelFile>,
}

#[derive(Deserialize)]
struct ModelFile {
    name: String,
    #[serde(rename = "downloadUrl")]
    download_url: Option<String>,
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
            // Token do download URL — hledání ho nepotřebuje, stažení často ano.
            if let Some(token) = &self.token {
                let sep = if lora.download_url.contains('?') {
                    '&'
                } else {
                    '?'
                };
                lora.download_url = format!("{}{}token={}", lora.download_url, sep, token);
            }
            lora
        }))
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
