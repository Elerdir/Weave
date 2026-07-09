//! HuggingFace Hub katalog — vyhledávání GGUF modelů a výpis kvantizací.
//!
//! Read-only klient nad veřejným HF API (`/api/models`). Funguje bez tokenu;
//! s HF tokenem (keychain) navíc vidí gated repa, ke kterým má uživatel
//! schválený přístup, a má vyšší rate limity.

use async_trait::async_trait;
use weave_application::{
    error::{AppError, AppResult},
    ports::model_catalog_port::{CatalogFile, CatalogModel, ModelCatalogPort},
};

pub struct HuggingFaceCatalog {
    http: reqwest::Client,
    base_url: String,
}

impl Default for HuggingFaceCatalog {
    fn default() -> Self {
        Self::new("https://huggingface.co")
    }
}

impl HuggingFaceCatalog {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            base_url: base_url.into(),
        }
    }

    fn get(&self, url: &str, token: Option<&str>) -> reqwest::RequestBuilder {
        let mut req = self.http.get(url);
        if let Some(token) = token {
            req = req.bearer_auth(token);
        }
        req
    }
}

#[async_trait]
impl ModelCatalogPort for HuggingFaceCatalog {
    async fn search(
        &self,
        query: &str,
        token: Option<&str>,
        limit: u32,
    ) -> AppResult<Vec<CatalogModel>> {
        let url = format!("{}/api/models", self.base_url);
        let resp = self
            .get(&url, token)
            .query(&[
                ("search", query.trim()),
                ("filter", "gguf"),
                ("sort", "downloads"),
                ("direction", "-1"),
                ("limit", &limit.clamp(1, 50).to_string()),
            ])
            .send()
            .await
            .map_err(|e| AppError::Repository(format!("HF hledání selhalo: {e}")))?;
        if !resp.status().is_success() {
            return Err(AppError::Repository(format!(
                "HF hledání selhalo: server vrátil {}",
                resp.status()
            )));
        }
        let items: Vec<serde_json::Value> = resp
            .json()
            .await
            .map_err(|e| AppError::Repository(format!("HF odpověď nejde parsovat: {e}")))?;

        Ok(items.iter().filter_map(parse_search_item).collect())
    }

    async fn list_gguf_files(
        &self,
        repo_id: &str,
        token: Option<&str>,
    ) -> AppResult<Vec<CatalogFile>> {
        // Bez `recursive` vrací jen kořen repa — GGUF kvantizace tam leží
        // prakticky vždy; rekurze by u velkých rep tahala tisíce záznamů.
        let url = format!("{}/api/models/{}/tree/main", self.base_url, repo_id);
        let resp = self
            .get(&url, token)
            .send()
            .await
            .map_err(|e| AppError::Repository(format!("Výpis souborů repa selhal: {e}")))?;
        if !resp.status().is_success() {
            return Err(AppError::Repository(format!(
                "Výpis souborů repa '{repo_id}' selhal: server vrátil {} (gated repo vyžaduje \
                 HF token se schváleným přístupem)",
                resp.status()
            )));
        }
        let entries: Vec<serde_json::Value> = resp
            .json()
            .await
            .map_err(|e| AppError::Repository(format!("HF odpověď nejde parsovat: {e}")))?;

        let mut files: Vec<CatalogFile> = entries
            .iter()
            .filter_map(|entry| {
                if entry.get("type")?.as_str()? != "file" {
                    return None;
                }
                let path = entry.get("path")?.as_str()?;
                if !path.to_ascii_lowercase().ends_with(".gguf") {
                    return None;
                }
                // U LFS souborů (všechny GGUF) je skutečná velikost v `lfs.size`;
                // top-level `size` bývá jen velikost pointer souboru.
                let lfs = entry.get("lfs");
                let size_bytes = lfs
                    .and_then(|l| l.get("size"))
                    .and_then(|s| s.as_u64())
                    .or_else(|| entry.get("size").and_then(|s| s.as_u64()))
                    .unwrap_or(0);
                // `lfs.oid` je SHA256 souboru (někdy s prefixem "sha256:").
                let sha256 = lfs
                    .and_then(|l| l.get("oid"))
                    .and_then(|o| o.as_str())
                    .map(|o| o.trim_start_matches("sha256:").to_ascii_lowercase())
                    .filter(|o| !o.is_empty());
                Some(CatalogFile {
                    file_name: path.to_string(),
                    size_bytes,
                    quant: parse_quant(path),
                    download_url: format!("{}/{}/resolve/main/{}", self.base_url, repo_id, path),
                    sha256,
                })
            })
            .collect();
        files.sort_by_key(|f| f.size_bytes);
        Ok(files)
    }
}

fn parse_search_item(item: &serde_json::Value) -> Option<CatalogModel> {
    let repo_id = item
        .get("id")
        .or_else(|| item.get("modelId"))?
        .as_str()?
        .to_string();
    let (author, name) = repo_id
        .split_once('/')
        .map(|(a, n)| (a.to_string(), n.to_string()))
        .unwrap_or_else(|| (String::new(), repo_id.clone()));
    Some(CatalogModel {
        author,
        name,
        downloads: item.get("downloads").and_then(|v| v.as_u64()).unwrap_or(0),
        likes: item.get("likes").and_then(|v| v.as_u64()).unwrap_or(0),
        // `gated` bývá false | "auto" | "manual" — cokoliv mimo false znamená
        // nutnost schváleného přístupu (a tedy HF token).
        gated: match item.get("gated") {
            Some(serde_json::Value::Bool(b)) => *b,
            Some(serde_json::Value::String(_)) => true,
            _ => false,
        },
        repo_id,
    })
}

/// Vyčte kvantizaci z názvu GGUF souboru: `model.Q4_K_M.gguf` → `Q4_K_M`,
/// `model-IQ2_XS.gguf` → `IQ2_XS`, `model-f16.gguf` → `F16`. Segmenty se
/// dělí tečkou a pomlčkou (podtržítka jsou součást názvu kvantizace).
fn parse_quant(file_name: &str) -> Option<String> {
    for segment in file_name.rsplit(['.', '-']) {
        let upper = segment.to_ascii_uppercase();
        if matches!(upper.as_str(), "F16" | "BF16" | "F32" | "FP16" | "FP32") {
            return Some(upper);
        }
        let rest = upper.strip_prefix("IQ").or_else(|| upper.strip_prefix("Q"));
        if let Some(rest) = rest {
            // Za prefixem musí být číslice (Q4…, IQ2…), jinak je to slovo jako "Quant"
            if rest.starts_with(|c: char| c.is_ascii_digit())
                && upper.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            {
                return Some(upper);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn parse_quant_recognizes_common_patterns() {
        assert_eq!(
            parse_quant("Meta-Llama-3.1-8B-Instruct-Q4_K_M.gguf").as_deref(),
            Some("Q4_K_M")
        );
        assert_eq!(parse_quant("model.Q8_0.gguf").as_deref(), Some("Q8_0"));
        assert_eq!(parse_quant("model-IQ2_XS.gguf").as_deref(), Some("IQ2_XS"));
        assert_eq!(parse_quant("model-f16.gguf").as_deref(), Some("F16"));
        // Bez kvantizace v názvu → None (a "Quant" nesmí projít jako Q...)
        assert_eq!(parse_quant("some-quant-model.gguf"), None);
    }

    #[test]
    fn parse_search_item_handles_gated_variants() {
        let plain = serde_json::json!({ "id": "org/model-GGUF", "downloads": 5, "likes": 2, "gated": false });
        let auto = serde_json::json!({ "id": "meta/llama-GGUF", "gated": "auto" });

        let plain = parse_search_item(&plain).unwrap();
        assert_eq!(plain.author, "org");
        assert_eq!(plain.name, "model-GGUF");
        assert!(!plain.gated);
        assert_eq!(plain.downloads, 5);

        assert!(parse_search_item(&auto).unwrap().gated);
    }

    #[tokio::test]
    async fn search_builds_query_and_parses_results() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/models"))
            .and(query_param("search", "llama 8b"))
            .and(query_param("filter", "gguf"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                { "id": "bartowski/Llama-8B-GGUF", "downloads": 1000, "likes": 50, "gated": false },
                { "id": "meta-llama/Llama-8B", "downloads": 99, "gated": "manual" }
            ])))
            .mount(&server)
            .await;

        let catalog = HuggingFaceCatalog::new(server.uri());
        let results = catalog.search("llama 8b", None, 20).await.unwrap();

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].repo_id, "bartowski/Llama-8B-GGUF");
        assert_eq!(results[0].downloads, 1000);
        assert!(results[1].gated);
    }

    #[tokio::test]
    async fn list_gguf_files_filters_sorts_and_builds_urls() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/models/org/repo-GGUF/tree/main"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                { "type": "file", "path": "README.md", "size": 100 },
                { "type": "file", "path": "big.Q8_0.gguf", "size": 134,
                  "lfs": { "size": 8_000_000_000u64, "oid": "sha256:ABCDEF123456" } },
                { "type": "file", "path": "small.Q4_K_M.gguf", "size": 134,
                  "lfs": { "size": 4_000_000_000u64, "oid": "deadbeef" } },
                { "type": "directory", "path": "images" }
            ])))
            .mount(&server)
            .await;

        let catalog = HuggingFaceCatalog::new(server.uri());
        let files = catalog
            .list_gguf_files("org/repo-GGUF", None)
            .await
            .unwrap();

        // Jen .gguf, seřazené od nejmenšího, velikost z lfs.size
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].file_name, "small.Q4_K_M.gguf");
        assert_eq!(files[0].size_bytes, 4_000_000_000);
        assert_eq!(files[0].quant.as_deref(), Some("Q4_K_M"));
        assert!(files[0]
            .download_url
            .ends_with("/org/repo-GGUF/resolve/main/small.Q4_K_M.gguf"));
        assert_eq!(files[0].sha256.as_deref(), Some("deadbeef"));
        assert_eq!(files[1].file_name, "big.Q8_0.gguf");
        // Prefix "sha256:" se odstraňuje a hex normalizuje na lowercase
        assert_eq!(files[1].sha256.as_deref(), Some("abcdef123456"));
    }

    #[tokio::test]
    async fn list_gguf_files_error_mentions_gated_hint() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;

        let catalog = HuggingFaceCatalog::new(server.uri());
        let err = catalog
            .list_gguf_files("meta/llama", None)
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("gated"), "chyba má poradit s tokenem: {err}");
    }
}
