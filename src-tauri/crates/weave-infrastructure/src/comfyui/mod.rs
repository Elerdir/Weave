use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use weave_application::{
    error::{AppError, AppResult},
    ports::image_gen_port::{ImageGenPort, ImageProgress, ImageRequest},
};

pub struct ComfyUiClient {
    http: Client,
    base_url: String,
}

impl ComfyUiClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            http: Client::new(),
            base_url: base_url.into(),
        }
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

#[async_trait]
impl ImageGenPort for ComfyUiClient {
    async fn generate(
        &self,
        request: ImageRequest,
        tx: mpsc::Sender<ImageProgress>,
    ) -> AppResult<()> {
        // Sestavíme základní txt2img workflow pro ComfyUI
        let workflow = build_basic_workflow(&request);
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
            .map_err(|e| AppError::ComfyUi(e.to_string()))?
            .json::<PromptResponse>()
            .await
            .map_err(|e| AppError::ComfyUi(e.to_string()))?;

        let prompt_id = resp.prompt_id;
        tracing::info!(%prompt_id, "ComfyUI prompt odeslán");

        // Poll na výsledek (zjednodušeno — produkčně WebSocket)
        let output_dir = dirs::data_dir()
            .unwrap_or_default()
            .join("weave")
            .join("gallery");
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

fn build_basic_workflow(req: &ImageRequest) -> serde_json::Value {
    serde_json::json!({
        "1": {
            "class_type": "CheckpointLoaderSimple",
            "inputs": { "ckpt_name": "flux1-dev.safetensors" }
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
    })
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
