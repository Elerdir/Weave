use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use weave_application::{
    error::{AppError, AppResult},
    ports::image_gen_port::{ImageGenPort, ImageProgress, ImageRequest, StylePreset},
};

pub struct ComfyUiClient {
    http: Client,
    base_url: String,
    /// Kam ukládat hotové obrázky. MUSÍ ležet uvnitř assetProtocol scope
    /// (`$APPDATA/weave/**`), jinak frontend náhled nezobrazí.
    gallery_dir: std::path::PathBuf,
}

impl ComfyUiClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            http: Client::new(),
            base_url: base_url.into(),
            gallery_dir: dirs::data_dir()
                .unwrap_or_default()
                .join("weave")
                .join("gallery"),
        }
    }

    pub fn with_gallery_dir(mut self, dir: impl Into<std::path::PathBuf>) -> Self {
        self.gallery_dir = dir.into();
        self
    }
}

#[derive(Serialize)]
struct PromptRequest {
    prompt: serde_json::Value,
    client_id: String,
}

#[derive(Deserialize)]
struct PromptResponse {
    prompt_id: String,
}

#[derive(Deserialize)]
struct UploadImageResponse {
    name: String,
    subfolder: String,
}

impl ComfyUiClient {
    /// Nahraje referenční obrázek do ComfyUI přes `/upload/image` a vrátí název,
    /// pod kterým ho pak vidí `LoadImage` uzel. Standardní `LoadImage` neumí přímou
    /// filesystem cestu, jen soubory z vlastní `input/` složky — proto ten upload.
    async fn upload_reference_image(&self, path: &str) -> AppResult<String> {
        let bytes = tokio::fs::read(path)
            .await
            .map_err(|e| AppError::ComfyUi(format!("Čtení referenčního obrázku selhalo: {e}")))?;

        let filename = std::path::Path::new(path)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "reference.png".to_string());
        let ext = std::path::Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        let mime =
            crate::attachment_store::mime_for_extension(ext).unwrap_or("application/octet-stream");

        let part = reqwest::multipart::Part::bytes(bytes)
            .file_name(filename)
            .mime_str(mime)
            .map_err(|e| AppError::ComfyUi(e.to_string()))?;
        let form = reqwest::multipart::Form::new().part("image", part);

        let resp = self
            .http
            .post(format!("{}/upload/image", self.base_url))
            .multipart(form)
            .send()
            .await
            .map_err(|e| AppError::ComfyUi(e.to_string()))?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(AppError::ComfyUi(format!(
                "Upload referenčního obrázku do ComfyUI selhal: {body}"
            )));
        }

        let uploaded = resp
            .json::<UploadImageResponse>()
            .await
            .map_err(|e| AppError::ComfyUi(e.to_string()))?;

        Ok(if uploaded.subfolder.is_empty() {
            uploaded.name
        } else {
            format!("{}/{}", uploaded.subfolder, uploaded.name)
        })
    }
}

#[async_trait]
impl ImageGenPort for ComfyUiClient {
    async fn generate(
        &self,
        request: ImageRequest,
        tx: mpsc::Sender<ImageProgress>,
    ) -> AppResult<()> {
        // Referenční obrázky musí být nahrané do ComfyUI dřív, než na ně
        // může workflow (LoadImage uzly) odkázat jménem.
        let mut uploaded_references = Vec::with_capacity(request.reference_image_paths.len());
        for path in &request.reference_image_paths {
            uploaded_references.push(self.upload_reference_image(path).await?);
        }
        let workflow = build_basic_workflow(&request, &uploaded_references);
        let client_id = uuid::Uuid::new_v4().to_string();

        let prompt_req = PromptRequest {
            prompt: workflow,
            client_id: client_id.clone(),
        };

        let resp = self
            .http
            .post(format!("{}/prompt", self.base_url))
            .json(&prompt_req)
            .send()
            .await
            .map_err(|e| AppError::ComfyUi(e.to_string()))?;

        // ComfyUI vrací u chybného workflow (např. chybějící checkpoint/uzel)
        // 400 s JSON popisem chyby — bez této kontroly by se to ztratilo
        // v nesrozumitelné serde chybě z parsování jako PromptResponse.
        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(AppError::ComfyUi(format!(
                "ComfyUI odmítl workflow: {body}"
            )));
        }

        let resp = resp
            .json::<PromptResponse>()
            .await
            .map_err(|e| AppError::ComfyUi(e.to_string()))?;

        let prompt_id = resp.prompt_id;
        tracing::info!(%prompt_id, "ComfyUI prompt odeslán");

        // Poll na výsledek (zjednodušeno — produkčně WebSocket)
        let output_dir = self.gallery_dir.clone();
        std::fs::create_dir_all(&output_dir).ok();

        loop {
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

            let history: serde_json::Value = self
                .http
                .get(format!("{}/history/{}", self.base_url, prompt_id))
                .send()
                .await
                .map_err(|e| AppError::ComfyUi(e.to_string()))?
                .json()
                .await
                .map_err(|e| AppError::ComfyUi(e.to_string()))?;

            if let Some(entry) = history.get(&prompt_id) {
                if let Some(outputs) = entry.get("outputs") {
                    if let Some(filename) = extract_output_filename(outputs) {
                        // Stáhni obrázek z ComfyUI
                        let img_url =
                            format!("{}/view?filename={}&type=output", self.base_url, filename);
                        let img_bytes = self
                            .http
                            .get(&img_url)
                            .send()
                            .await
                            .map_err(|e| AppError::ComfyUi(e.to_string()))?
                            .bytes()
                            .await
                            .map_err(|e| AppError::ComfyUi(e.to_string()))?;

                        let out_path = output_dir.join(&filename);
                        std::fs::write(&out_path, &img_bytes)
                            .map_err(|e| AppError::ComfyUi(e.to_string()))?;

                        // Označení AI původu (metadata + neviditelný vodoznak).
                        // Selhání generování neshodí — obrázek už na disku je.
                        if let Err(e) = crate::image_stamp::stamp_ai_image(&out_path) {
                            tracing::warn!("Označení AI obrázku selhalo: {e:#}");
                        }

                        let _ = tx
                            .send(ImageProgress::Done {
                                output_path: out_path.to_string_lossy().into_owned(),
                            })
                            .await;
                        return Ok(());
                    }
                }
            }
        }
    }

    async fn is_available(&self) -> bool {
        self.http
            .get(format!("{}/system_stats", self.base_url))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }
}

/// Sestaví workflow JSON pro ComfyUI `/prompt`. Čistá funkce (žádné I/O) —
/// `uploaded_reference` je název souboru, který referenční obrázek dostal
/// PO uploadu na ComfyUI server (viz `ComfyUiClient::upload_reference_image`),
/// ne lokální filesystem cesta.
///
/// Checkpoint se volí podle stylu promptu (`req.style_preset`) — anime má
/// vlastní doladěný SDXL checkpoint, zbytek jede na SDXL base. Obě varianty
/// jsou SDXL architektura, takže fungují i s PuLID větví.
fn build_basic_workflow(req: &ImageRequest, uploaded_references: &[String]) -> serde_json::Value {
    if uploaded_references.is_empty() {
        build_txt2img_workflow(req)
    } else {
        build_pulid_workflow(req, uploaded_references)
    }
}

/// Pony V6 checkpoint vyžaduje score tagy v promptu (bez nich generuje
/// znatelně hůř). Vrací (prefix pozitivního, prefix negativního promptu).
fn style_prompt_prefixes(style: StylePreset) -> (&'static str, &'static str) {
    match style {
        StylePreset::SemiRealistic => (
            "score_9, score_8_up, score_7_up, realistic, ",
            "score_6, score_5, score_4, ",
        ),
        StylePreset::Anime => (
            "score_9, score_8_up, score_7_up, source_anime, ",
            "score_6, score_5, score_4, ",
        ),
        _ => ("", ""),
    }
}

/// Pony V6 se trénoval s clip skip 2 — bez CLIPSetLastLayer(-2) vychází
/// obrázky rozbité.
fn uses_clip_skip(style: StylePreset) -> bool {
    matches!(style, StylePreset::SemiRealistic | StylePreset::Anime)
}

/// Doplní style-specifika do hotového workflow: prefixy promptů a případný
/// CLIPSetLastLayer uzel (id 13), přes který pak jdou oba text encody.
fn apply_style_tuning(workflow: &mut serde_json::Value, req: &ImageRequest) {
    let (pos_prefix, neg_prefix) = style_prompt_prefixes(req.style_preset);
    let map = workflow.as_object_mut().expect("workflow je JSON objekt");

    if !pos_prefix.is_empty() {
        map["2"]["inputs"]["text"] = serde_json::json!(format!("{pos_prefix}{}", req.prompt));
        map["3"]["inputs"]["text"] = serde_json::json!(format!(
            "{neg_prefix}{}",
            req.negative_prompt.as_deref().unwrap_or("")
        ));
    }

    if uses_clip_skip(req.style_preset) {
        map.insert(
            "13".into(),
            serde_json::json!({
                "class_type": "CLIPSetLastLayer",
                "inputs": { "clip": ["1", 1], "stop_at_clip_layer": -2 }
            }),
        );
        map["2"]["inputs"]["clip"] = serde_json::json!(["13", 0]);
        map["3"]["inputs"]["clip"] = serde_json::json!(["13", 0]);
    }
}

fn build_txt2img_workflow(req: &ImageRequest) -> serde_json::Value {
    let mut workflow = serde_json::json!({
        "1": {
            "class_type": "CheckpointLoaderSimple",
            "inputs": {
                "ckpt_name": crate::comfy_installer::checkpoint_filename_for_style(req.style_preset)
            }
        },
        "2": {
            "class_type": "CLIPTextEncode",
            "inputs": { "text": req.prompt, "clip": ["1", 1] }
        },
        "3": {
            "class_type": "CLIPTextEncode",
            "inputs": {
                "text": req.negative_prompt.as_deref().unwrap_or(""),
                "clip": ["1", 1]
            }
        },
        "4": {
            "class_type": "EmptyLatentImage",
            "inputs": { "width": req.width, "height": req.height, "batch_size": 1 }
        },
        "5": {
            "class_type": "KSampler",
            "inputs": {
                "model": ["1", 0],
                "positive": ["2", 0],
                "negative": ["3", 0],
                "latent_image": ["4", 0],
                "steps": req.steps,
                "cfg": req.cfg_scale,
                "sampler_name": "euler",
                "scheduler": "normal",
                "denoise": 1.0,
                "seed": req.seed.unwrap_or(42)
            }
        },
        "6": {
            "class_type": "VAEDecode",
            "inputs": { "samples": ["5", 0], "vae": ["1", 2] }
        },
        "7": {
            "class_type": "SaveImage",
            "inputs": { "images": ["6", 0], "filename_prefix": "weave" }
        }
    });
    apply_style_tuning(&mut workflow, req);
    workflow
}

/// PuLID workflow s podporou VÍCE referenčních obrázků: každý má vlastní
/// LoadImage uzel (id 20, 21, …) a dohromady se řetězí core uzly `ImageBatch`
/// (id 30, 31, …) do jedné dávky — PuLID pak identity embeddingy z dávky
/// průměruje, takže víc fotek = věrnější podoba. `ImageBatch` si rozdílné
/// rozměry srovná sám (druhý obrázek přeškáluje na rozměr prvního).
fn build_pulid_workflow(req: &ImageRequest, uploaded_images: &[String]) -> serde_json::Value {
    let mut workflow = serde_json::json!({
        "1": {
            "class_type": "CheckpointLoaderSimple",
            "inputs": {
                "ckpt_name": crate::comfy_installer::checkpoint_filename_for_style(req.style_preset)
            }
        },
        "2": {
            "class_type": "CLIPTextEncode",
            "inputs": { "text": req.prompt, "clip": ["1", 1] }
        },
        "3": {
            "class_type": "CLIPTextEncode",
            "inputs": {
                "text": req.negative_prompt.as_deref().unwrap_or(""),
                "clip": ["1", 1]
            }
        },
        "4": {
            "class_type": "EmptyLatentImage",
            "inputs": { "width": req.width, "height": req.height, "batch_size": 1 }
        },
        "5": {
            "class_type": "KSampler",
            "inputs": {
                "model": ["12", 0],
                "positive": ["2", 0],
                "negative": ["3", 0],
                "latent_image": ["4", 0],
                "steps": req.steps,
                "cfg": req.cfg_scale,
                "sampler_name": "euler",
                "scheduler": "normal",
                "denoise": 1.0,
                "seed": req.seed.unwrap_or(42)
            }
        },
        "6": {
            "class_type": "VAEDecode",
            "inputs": { "samples": ["5", 0], "vae": ["1", 2] }
        },
        "7": {
            "class_type": "SaveImage",
            "inputs": { "images": ["6", 0], "filename_prefix": "weave" }
        },
        "9": {
            "class_type": "PulidModelLoader",
            "inputs": { "pulid_file": crate::comfy_installer::PULID_WEIGHTS_FILENAME }
        },
        "10": {
            "class_type": "PulidInsightFaceLoader",
            "inputs": { "provider": "CPU" }
        },
        "11": {
            "class_type": "PulidEvaClipLoader",
            "inputs": {}
        },
        // fidelity/1.0/celý rozsah start_at..end_at — stejné hodnoty jako
        // v ukázkových workflow z cubiq/PuLID_ComfyUI (nejbližší podobnost referenci).
        "12": {
            "class_type": "ApplyPulid",
            "inputs": {
                "model": ["1", 0],
                "pulid": ["9", 0],
                "eva_clip": ["11", 0],
                "face_analysis": ["10", 0],
                "image": ["8", 0],
                "method": "fidelity",
                "weight": 1.0,
                "start_at": 0.0,
                "end_at": 1.0
            }
        }
    });

    // LoadImage uzel pro každou referenci (id 20, 21, …)
    let map = workflow.as_object_mut().expect("workflow je JSON objekt");
    for (i, image) in uploaded_images.iter().enumerate() {
        map.insert(
            format!("{}", 20 + i),
            serde_json::json!({
                "class_type": "LoadImage",
                "inputs": { "image": image }
            }),
        );
    }

    // Řetěz ImageBatch uzlů: (20+21)→30, (30+22)→31, … Poslední článek
    // (nebo přímo LoadImage u jediné fotky) jde do ApplyPulid jako uzel "8".
    let mut last_ref: serde_json::Value = serde_json::json!(["20", 0]);
    for i in 1..uploaded_images.len() {
        let batch_id = format!("{}", 30 + i - 1);
        map.insert(
            batch_id.clone(),
            serde_json::json!({
                "class_type": "ImageBatch",
                "inputs": {
                    "image1": last_ref,
                    "image2": [format!("{}", 20 + i), 0]
                }
            }),
        );
        last_ref = serde_json::json!([batch_id, 0]);
    }
    map["12"]["inputs"]["image"] = last_ref;

    apply_style_tuning(&mut workflow, req);
    workflow
}

fn extract_output_filename(outputs: &serde_json::Value) -> Option<String> {
    outputs.as_object()?.values().find_map(|node| {
        node.get("images")?
            .as_array()?
            .first()?
            .get("filename")?
            .as_str()
            .map(|s| s.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use weave_application::ports::image_gen_port::StylePreset;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn sample_request() -> ImageRequest {
        ImageRequest {
            prompt: "kosmická loď nad planetou".into(),
            negative_prompt: Some("rozmazané".into()),
            width: 1024,
            height: 768,
            steps: 25,
            cfg_scale: 6.5,
            seed: Some(1234),
            style_preset: StylePreset::Realistic,
            reference_image_paths: vec![],
        }
    }

    #[test]
    fn txt2img_workflow_used_without_reference_image() {
        let req = sample_request();
        let workflow = build_basic_workflow(&req, &[]);

        // Realistický styl → RealVisXL (nejlepší fotorealismus)
        assert_eq!(
            workflow["1"]["inputs"]["ckpt_name"],
            crate::comfy_installer::REALVIS_CHECKPOINT_FILENAME
        );
        assert_eq!(
            workflow["5"]["inputs"]["model"],
            serde_json::json!(["1", 0])
        );
        assert_eq!(workflow.as_object().unwrap().len(), 7);

        // Žádná PuLID větev nesmí být přítomná, dokud není referenční obrázek
        let json_text = workflow.to_string();
        assert!(!json_text.contains("Pulid"));
    }

    #[test]
    fn txt2img_anime_style_switches_checkpoint() {
        let mut req = sample_request();
        req.style_preset = StylePreset::Anime;

        let workflow = build_basic_workflow(&req, &[]);
        assert_eq!(
            workflow["1"]["inputs"]["ckpt_name"],
            crate::comfy_installer::PONY_CHECKPOINT_FILENAME
        );

        // Pony checkpoint je SDXL architektura → platí i pro PuLID větev
        let pulid = build_basic_workflow(&req, &["ref.png".into()]);
        assert_eq!(
            pulid["1"]["inputs"]["ckpt_name"],
            crate::comfy_installer::PONY_CHECKPOINT_FILENAME
        );
    }

    #[test]
    fn pony_styles_get_score_tags_and_clip_skip() {
        let mut req = sample_request();
        req.style_preset = StylePreset::Anime;

        let workflow = build_basic_workflow(&req, &[]);
        let positive = workflow["2"]["inputs"]["text"].as_str().unwrap();
        let negative = workflow["3"]["inputs"]["text"].as_str().unwrap();
        assert!(positive.starts_with("score_9, score_8_up, score_7_up, source_anime, "));
        assert!(positive.ends_with("kosmická loď nad planetou"));
        assert!(negative.starts_with("score_6, score_5, score_4, "));

        // Clip skip -2 přes CLIPSetLastLayer, text encody vedou přes něj
        assert_eq!(workflow["13"]["class_type"], "CLIPSetLastLayer");
        assert_eq!(workflow["13"]["inputs"]["stop_at_clip_layer"], -2);
        assert_eq!(
            workflow["2"]["inputs"]["clip"],
            serde_json::json!(["13", 0])
        );

        // Semi-real dostane realistic tag místo source_anime
        req.style_preset = StylePreset::SemiRealistic;
        let semi = build_basic_workflow(&req, &[]);
        let semi_positive = semi["2"]["inputs"]["text"].as_str().unwrap();
        assert!(semi_positive.contains("realistic, "));
        assert!(!semi_positive.contains("source_anime"));
    }

    #[test]
    fn realistic_style_has_no_score_tags_nor_clip_skip() {
        let req = sample_request();
        let workflow = build_basic_workflow(&req, &[]);

        assert_eq!(workflow["2"]["inputs"]["text"], "kosmická loď nad planetou");
        assert!(workflow.get("13").is_none());
        assert_eq!(workflow["2"]["inputs"]["clip"], serde_json::json!(["1", 1]));
    }

    #[test]
    fn flux_workflow_carries_over_request_fields() {
        let req = sample_request();
        let workflow = build_basic_workflow(&req, &[]);

        assert_eq!(workflow["2"]["inputs"]["text"], "kosmická loď nad planetou");
        assert_eq!(workflow["3"]["inputs"]["text"], "rozmazané");
        assert_eq!(workflow["4"]["inputs"]["width"], 1024);
        assert_eq!(workflow["4"]["inputs"]["height"], 768);
        assert_eq!(workflow["5"]["inputs"]["steps"], 25);
        assert_eq!(workflow["5"]["inputs"]["cfg"], 6.5);
        assert_eq!(workflow["5"]["inputs"]["seed"], 1234);
    }

    #[test]
    fn flux_workflow_defaults_missing_negative_prompt_and_seed() {
        let mut req = sample_request();
        req.negative_prompt = None;
        req.seed = None;

        let workflow = build_basic_workflow(&req, &[]);

        assert_eq!(workflow["3"]["inputs"]["text"], "");
        assert_eq!(workflow["5"]["inputs"]["seed"], 42);
    }

    #[test]
    fn pulid_workflow_used_when_reference_image_present() {
        let req = sample_request();
        let workflow = build_basic_workflow(&req, &["uploaded-ref.png".into()]);

        assert_eq!(
            workflow["1"]["inputs"]["ckpt_name"],
            crate::comfy_installer::REALVIS_CHECKPOINT_FILENAME
        );
        assert_eq!(workflow["20"]["class_type"], "LoadImage");
        assert_eq!(workflow["20"]["inputs"]["image"], "uploaded-ref.png");
        // Jediná fotka → žádný ImageBatch, LoadImage jde do ApplyPulid přímo
        assert_eq!(
            workflow["12"]["inputs"]["image"],
            serde_json::json!(["20", 0])
        );
        assert!(workflow.get("30").is_none());
    }

    #[test]
    fn pulid_workflow_batches_multiple_references() {
        let req = sample_request();
        let refs: Vec<String> = vec!["a.png".into(), "b.png".into(), "c.png".into()];
        let workflow = build_basic_workflow(&req, &refs);

        // Tři LoadImage uzly
        assert_eq!(workflow["20"]["inputs"]["image"], "a.png");
        assert_eq!(workflow["21"]["inputs"]["image"], "b.png");
        assert_eq!(workflow["22"]["inputs"]["image"], "c.png");

        // Řetěz: 30 = batch(20, 21), 31 = batch(30, 22)
        assert_eq!(workflow["30"]["class_type"], "ImageBatch");
        assert_eq!(
            workflow["30"]["inputs"]["image1"],
            serde_json::json!(["20", 0])
        );
        assert_eq!(
            workflow["30"]["inputs"]["image2"],
            serde_json::json!(["21", 0])
        );
        assert_eq!(
            workflow["31"]["inputs"]["image1"],
            serde_json::json!(["30", 0])
        );
        assert_eq!(
            workflow["31"]["inputs"]["image2"],
            serde_json::json!(["22", 0])
        );

        // Konec řetězu jde do ApplyPulid
        assert_eq!(
            workflow["12"]["inputs"]["image"],
            serde_json::json!(["31", 0])
        );
    }

    #[test]
    fn pulid_nodes_are_wired_together_correctly() {
        let req = sample_request();
        let workflow = build_basic_workflow(&req, &["uploaded-ref.png".into()]);

        assert_eq!(workflow["9"]["class_type"], "PulidModelLoader");
        assert_eq!(
            workflow["9"]["inputs"]["pulid_file"],
            crate::comfy_installer::PULID_WEIGHTS_FILENAME
        );
        assert_eq!(workflow["10"]["class_type"], "PulidInsightFaceLoader");
        assert_eq!(workflow["11"]["class_type"], "PulidEvaClipLoader");

        let apply_pulid = &workflow["12"];
        assert_eq!(apply_pulid["class_type"], "ApplyPulid");
        assert_eq!(apply_pulid["inputs"]["model"], serde_json::json!(["1", 0]));
        assert_eq!(apply_pulid["inputs"]["pulid"], serde_json::json!(["9", 0]));
        assert_eq!(
            apply_pulid["inputs"]["eva_clip"],
            serde_json::json!(["11", 0])
        );
        assert_eq!(
            apply_pulid["inputs"]["face_analysis"],
            serde_json::json!(["10", 0])
        );
        assert_eq!(apply_pulid["inputs"]["image"], serde_json::json!(["20", 0]));

        // KSampler musí čerpat z PuLID-patchnutého modelu (uzel 12), ne přímo
        // z checkpointu (uzel 1) — jinak by referenční obrázek nemělo na co vliv.
        assert_eq!(
            workflow["5"]["inputs"]["model"],
            serde_json::json!(["12", 0])
        );
    }

    #[test]
    fn pulid_workflow_carries_over_request_fields() {
        let req = sample_request();
        let workflow = build_basic_workflow(&req, &["uploaded-ref.png".into()]);

        assert_eq!(workflow["2"]["inputs"]["text"], "kosmická loď nad planetou");
        assert_eq!(workflow["3"]["inputs"]["text"], "rozmazané");
        assert_eq!(workflow["4"]["inputs"]["width"], 1024);
        assert_eq!(workflow["4"]["inputs"]["height"], 768);
        assert_eq!(workflow["5"]["inputs"]["steps"], 25);
        assert_eq!(workflow["5"]["inputs"]["cfg"], 6.5);
        assert_eq!(workflow["5"]["inputs"]["seed"], 1234);
    }

    async fn write_temp_file(name: &str, content: &[u8]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("weave_comfyui_test_{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let path = dir.join(name);
        tokio::fs::write(&path, content).await.unwrap();
        path
    }

    #[tokio::test]
    async fn upload_reference_image_returns_plain_filename() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/upload/image"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "name": "abc123.png",
                "subfolder": "",
                "type": "input"
            })))
            .mount(&server)
            .await;

        let client = ComfyUiClient::new(server.uri());
        let source = write_temp_file("photo.png", b"fake-png-bytes").await;

        let uploaded = client
            .upload_reference_image(source.to_str().unwrap())
            .await
            .unwrap();

        assert_eq!(uploaded, "abc123.png");
    }

    #[tokio::test]
    async fn upload_reference_image_joins_nonempty_subfolder() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/upload/image"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "name": "abc123.png",
                "subfolder": "clipspace",
                "type": "input"
            })))
            .mount(&server)
            .await;

        let client = ComfyUiClient::new(server.uri());
        let source = write_temp_file("photo.png", b"fake-png-bytes").await;

        let uploaded = client
            .upload_reference_image(source.to_str().unwrap())
            .await
            .unwrap();

        assert_eq!(uploaded, "clipspace/abc123.png");
    }

    #[tokio::test]
    async fn upload_reference_image_propagates_server_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/upload/image"))
            .respond_with(ResponseTemplate::new(500).set_body_string("boom"))
            .mount(&server)
            .await;

        let client = ComfyUiClient::new(server.uri());
        let source = write_temp_file("photo.png", b"fake-png-bytes").await;

        let result = client
            .upload_reference_image(source.to_str().unwrap())
            .await;

        assert!(result.is_err());
    }
}
