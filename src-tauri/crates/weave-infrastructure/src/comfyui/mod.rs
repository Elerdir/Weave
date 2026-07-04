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

    /// Ověří, že běžící ComfyUI zná daný checkpoint (dotaz na `/object_info`).
    /// Když ne, vrátí srozumitelnou chybu — typicky na portu poslouchá jiná
    /// ComfyUI instance (např. z AI Studia) než ta, do které Weave stahuje
    /// modely. Bez tohohle by /prompt vrátil jen kryptické „value_not_in_list".
    /// Nedostupné `/object_info` = kontrolu přeskočíme (chybu ukáže až /prompt).
    async fn verify_checkpoint_available(&self, ckpt_name: &str) -> AppResult<()> {
        let url = format!("{}/object_info/CheckpointLoaderSimple", self.base_url);
        let Ok(resp) = self.http.get(&url).send().await else {
            return Ok(());
        };
        if !resp.status().is_success() {
            return Ok(());
        }
        let Ok(info) = resp.json::<serde_json::Value>().await else {
            return Ok(());
        };

        let list = info
            .get("CheckpointLoaderSimple")
            .and_then(|c| c.get("input"))
            .and_then(|i| i.get("required"))
            .and_then(|r| r.get("ckpt_name"))
            .and_then(|c| c.get(0))
            .and_then(|l| l.as_array());

        let Some(list) = list else {
            return Ok(());
        };
        if list.iter().any(|v| v.as_str() == Some(ckpt_name)) {
            return Ok(());
        }

        let available: Vec<&str> = list.iter().filter_map(|v| v.as_str()).collect();
        let available = if available.is_empty() {
            "žádné".to_string()
        } else {
            available.join(", ")
        };
        Err(AppError::ComfyUi(format!(
            "Běžící ComfyUI nezná model '{ckpt_name}'. Na adrese {} nejspíš poslouchá jiná \
             ComfyUI instance (např. z AI Studia) než ta, kterou spravuje Weave - zavři ji \
             a zkus generování znovu. Dostupné modely: {available}.",
            self.base_url
        )))
    }

    /// Zná běžící ComfyUI daný typ uzlu (`/object_info/<class>`)? Slouží k
    /// „graceful degradation" volitelných uzlů (FaceDetailer z Impact Packu) —
    /// když chybí nebo je `/object_info` nedostupné, vrátíme `false` a workflow
    /// se poskládá bez něj, místo aby /prompt spadl na neznámém uzlu.
    async fn verify_node_available(&self, class_type: &str) -> bool {
        let url = format!("{}/object_info/{class_type}", self.base_url);
        let Ok(resp) = self.http.get(&url).send().await else {
            return false;
        };
        if !resp.status().is_success() {
            return false;
        }
        match resp.json::<serde_json::Value>().await {
            Ok(info) => info.get(class_type).is_some(),
            Err(_) => false,
        }
    }
}

#[async_trait]
impl ImageGenPort for ComfyUiClient {
    async fn generate(
        &self,
        mut request: ImageRequest,
        tx: mpsc::Sender<ImageProgress>,
    ) -> AppResult<()> {
        // FaceDetailer (Impact Pack) je volitelný uzel — když ho běžící ComfyUI
        // nezná (instalace ho ještě nemá), radši ho ze zadání shodíme, aby
        // /prompt nespadl na neznámém uzlu. Ostatní kroky pipeline pokračují.
        if request.face_detailer && !self.verify_node_available("FaceDetailer").await {
            tracing::warn!(
                "FaceDetailer uzel není v běžícím ComfyUI dostupný — generuji bez doladění obličeje"
            );
            request.face_detailer = false;
        }

        // Referenční obrázky musí být nahrané do ComfyUI dřív, než na ně
        // může workflow (LoadImage uzly) odkázat jménem.
        let mut uploaded_references = Vec::with_capacity(request.reference_image_paths.len());
        for path in &request.reference_image_paths {
            uploaded_references.push(self.upload_reference_image(path).await?);
        }
        let uploaded_init = match &request.init_image_path {
            Some(path) => Some(self.upload_reference_image(path).await?),
            None => None,
        };
        let workflow =
            build_basic_workflow(&request, &uploaded_references, uploaded_init.as_deref());

        // Preflight: zná běžící ComfyUI požadovaný checkpoint? Když ne, dej
        // srozumitelnou chybu (nejspíš cizí instance na portu) místo kryptické
        // validace z /prompt.
        if let Some(ckpt) = workflow
            .get("1")
            .and_then(|n| n.get("inputs"))
            .and_then(|i| i.get("ckpt_name"))
            .and_then(|v| v.as_str())
        {
            self.verify_checkpoint_available(ckpt).await?;
        }

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

        // Kroky sampleru posílá ComfyUI přes WebSocket — posloucháme je na
        // pozadí a přeposíláme jako Progress (frontend z nich dělá procenta).
        // Výsledek dál hlídáme pollingem /history (spolehlivější na dokončení).
        let ws_task = tokio::spawn(forward_sampler_progress(
            self.base_url.clone(),
            client_id,
            tx.clone(),
        ));

        let output_dir = self.gallery_dir.clone();
        std::fs::create_dir_all(&output_dir).ok();

        // Backstop proti nekonečnému čekání, kdyby ComfyUI zamrzl bez odpovědi.
        let started = std::time::Instant::now();
        const GEN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(600);

        loop {
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

            if started.elapsed() > GEN_TIMEOUT {
                ws_task.abort();
                return Err(AppError::ComfyUi(
                    "Generování obrázku trvalo přes 10 minut a bylo přerušeno. Časté při \
                     nedostatku VRAM — máš-li současně načtený velký lokální LLM, uvolni ho \
                     (v Nastavení přepni backend nebo restartuj appku) a zkus to znovu."
                        .into(),
                ));
            }

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
                // Běhová chyba ComfyUI (nejčastěji nedostatek VRAM u velkých
                // workflow) → status "error" a žádné outputs. Bez téhle větve
                // by se polling točil donekonečna a v UI se „nic nedělo".
                if let Some(err) = extract_execution_error(entry) {
                    ws_task.abort();
                    return Err(AppError::ComfyUi(format!(
                        "Generování v ComfyUI selhalo: {err}. Bývá to nedostatkem VRAM — \
                         pokud máš načtený velký lokální model, uvolni ho a zkus znovu."
                    )));
                }
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

                        ws_task.abort();

                        // Označení AI původu (metadata + neviditelný vodoznak)
                        // vč. promptu pro galerii. Selhání generování neshodí —
                        // obrázek už na disku je.
                        if let Err(e) = crate::image_stamp::stamp_ai_image(
                            &out_path,
                            Some(&request.prompt),
                            request.negative_prompt.as_deref(),
                        ) {
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
fn build_basic_workflow(
    req: &ImageRequest,
    uploaded_references: &[String],
    uploaded_init: Option<&str>,
) -> serde_json::Value {
    let mut workflow = if uploaded_references.is_empty() {
        build_txt2img_workflow(req)
    } else {
        build_pulid_workflow(req, uploaded_references)
    };
    if let Some(init) = uploaded_init {
        apply_init_image(&mut workflow, init);
    }
    if req.hires_fix {
        apply_hires_fix(&mut workflow, req);
    }
    if req.face_detailer {
        apply_face_detailer(&mut workflow, req);
    }
    workflow
}

/// Doladí detekovaný obličej: `UltralyticsDetectorProvider` (uzel 40) najde
/// tvář a `FaceDetailer` (uzel 41) ji vyřízne, přesampluje s nízkým denoise
/// a vloží zpět — nejvíc pomůže očím, které bývají „divné". Vstupní obrázek
/// bere z `VAEDecode` (6, tj. i po hi-res průchodu), `SaveImage` (7) pak čte
/// z FaceDetaileru. Model/CLIP/podmínky přebírá z hlavního sampleru (5) a text
/// encodéru (2), takže sedí i pro PuLID / LoRA / clip-skip varianty.
///
/// Volá se jen když je `FaceDetailer` v běžícím ComfyUI dostupný (Impact Pack) —
/// dostupnost ověří `generate()` předem a jinak `req.face_detailer` shodí.
fn apply_face_detailer(workflow: &mut serde_json::Value, req: &ImageRequest) {
    let map = workflow.as_object_mut().expect("workflow je JSON objekt");
    let model = map["5"]["inputs"]["model"].clone();
    let positive = map["5"]["inputs"]["positive"].clone();
    let negative = map["5"]["inputs"]["negative"].clone();
    let clip = map["2"]["inputs"]["clip"].clone();
    let seed = req.seed.unwrap_or(42);

    map.insert(
        "40".into(),
        serde_json::json!({
            "class_type": "UltralyticsDetectorProvider",
            "inputs": { "model_name": crate::comfy_installer::FACE_DETECTOR_MODEL_NAME }
        }),
    );
    map.insert(
        "41".into(),
        serde_json::json!({
            "class_type": "FaceDetailer",
            "inputs": {
                "image": ["6", 0],
                "model": model,
                "clip": clip,
                "vae": ["1", 2],
                "positive": positive,
                "negative": negative,
                "bbox_detector": ["40", 0],
                "guide_size": 512,
                "guide_size_for": true,
                "max_size": 1024,
                "seed": seed,
                "steps": 20,
                "cfg": req.cfg_scale,
                "sampler_name": "euler",
                "scheduler": "normal",
                "denoise": 0.5,
                "feather": 5,
                "noise_mask": true,
                "force_inpaint": true,
                "bbox_threshold": 0.5,
                "bbox_dilation": 10,
                "bbox_crop_factor": 3.0,
                "sam_detection_hint": "center-1",
                "sam_dilation": 0,
                "sam_threshold": 0.93,
                "sam_bbox_expansion": 0,
                "sam_mask_hint_threshold": 0.7,
                "sam_mask_hint_use_negative": "False",
                "drop_size": 10,
                "wildcard": "",
                "cycle": 1
            }
        }),
    );
    map["7"]["inputs"]["images"] = serde_json::json!(["41", 0]);
}

/// Hi-res průchod: latent z hlavního sampleru (5) se zvětší 1,5× (uzel 17)
/// a přesampluje druhým KSamplerem (18) s nízkým denoise. Model i prompty
/// se přebírají z uzlu 5 (už správně zapojené i pro PuLID/LoRA větev), takže
/// funguje napříč variantami. VAEDecode (6) pak bere latent z 18.
fn apply_hires_fix(workflow: &mut serde_json::Value, req: &ImageRequest) {
    let map = workflow.as_object_mut().expect("workflow je JSON objekt");
    let model = map["5"]["inputs"]["model"].clone();
    let positive = map["5"]["inputs"]["positive"].clone();
    let negative = map["5"]["inputs"]["negative"].clone();
    let seed = req.seed.unwrap_or(42);
    let hires_w = (req.width as f32 * 1.5) as u32;
    let hires_h = (req.height as f32 * 1.5) as u32;

    map.insert(
        "17".into(),
        serde_json::json!({
            "class_type": "LatentUpscale",
            "inputs": {
                "samples": ["5", 0],
                "upscale_method": "nearest-exact",
                "width": hires_w,
                "height": hires_h,
                "crop": "disabled"
            }
        }),
    );
    map.insert(
        "18".into(),
        serde_json::json!({
            "class_type": "KSampler",
            "inputs": {
                "model": model,
                "positive": positive,
                "negative": negative,
                "latent_image": ["17", 0],
                "steps": 15,
                "cfg": req.cfg_scale,
                "sampler_name": "euler",
                "scheduler": "normal",
                "denoise": 0.45,
                "seed": seed
            }
        }),
    );
    map["6"]["inputs"]["samples"] = serde_json::json!(["18", 0]);
}

/// Přepne workflow na img2img: latent se místo prázdného plátna vezme
/// z výchozího obrázku (LoadImage 15 → VAEEncode 16) a denoise klesne na
/// 0.55 — výsledek zachová kompozici a mění jen to, co říká prompt.
/// Funguje pro txt2img i PuLID větev (obě berou latent z KSampleru "5").
fn apply_init_image(workflow: &mut serde_json::Value, uploaded_init: &str) {
    let map = workflow.as_object_mut().expect("workflow je JSON objekt");
    map.insert(
        "15".into(),
        serde_json::json!({
            "class_type": "LoadImage",
            "inputs": { "image": uploaded_init }
        }),
    );
    map.insert(
        "16".into(),
        serde_json::json!({
            "class_type": "VAEEncode",
            "inputs": { "pixels": ["15", 0], "vae": ["1", 2] }
        }),
    );
    map["5"]["inputs"]["latent_image"] = serde_json::json!(["16", 0]);
    map["5"]["inputs"]["denoise"] = serde_json::json!(0.55);
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

/// Zapojí LoRA (uzel 14): model větev jde checkpoint → LoraLoader →
/// spotřebitel modelu (KSampler u txt2img, ApplyPulid u reference větve),
/// CLIP větev checkpoint → LoraLoader → (CLIPSetLastLayer) → text encody.
/// Trigger words do promptu doplňuje orchestrace, ne builder.
fn apply_lora(workflow: &mut serde_json::Value, req: &ImageRequest, model_consumer: &str) {
    let Some(lora_file) = &req.lora_file else {
        return;
    };
    let map = workflow.as_object_mut().expect("workflow je JSON objekt");
    map.insert(
        "14".into(),
        serde_json::json!({
            "class_type": "LoraLoader",
            "inputs": {
                "model": ["1", 0],
                "clip": ["1", 1],
                "lora_name": lora_file,
                "strength_model": 0.8,
                "strength_clip": 0.8
            }
        }),
    );
    map[model_consumer]["inputs"]["model"] = serde_json::json!(["14", 0]);
    if map.contains_key("13") {
        // Clip skip (Pony) zůstává poslední v řadě před text encody
        map["13"]["inputs"]["clip"] = serde_json::json!(["14", 1]);
    } else {
        map["2"]["inputs"]["clip"] = serde_json::json!(["14", 1]);
        map["3"]["inputs"]["clip"] = serde_json::json!(["14", 1]);
    }
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
    apply_lora(&mut workflow, req, "5");
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
        // fidelity/celý rozsah start_at..end_at — jako v ukázkových workflow
        // z cubiq/PuLID_ComfyUI. `weight` (síla podoby) je per-konverzace
        // laditelná (posuvník v chatu), výchozí 1.0.
        "12": {
            "class_type": "ApplyPulid",
            "inputs": {
                "model": ["1", 0],
                "pulid": ["9", 0],
                "eva_clip": ["11", 0],
                "face_analysis": ["10", 0],
                "image": ["8", 0],
                "method": "fidelity",
                "weight": req.pulid_weight,
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
    apply_lora(&mut workflow, req, "12");
    workflow
}

/// Poslouchá ComfyUI WebSocket a přeposílá kroky sampleru jako
/// `ImageProgress::Progress`. Best-effort — když se WS nepřipojí, generování
/// běží dál jen bez procent (výsledek hlídá polling /history).
async fn forward_sampler_progress(
    base_url: String,
    client_id: String,
    tx: mpsc::Sender<ImageProgress>,
) {
    use futures_util::StreamExt;

    let ws_url = format!(
        "{}/ws?clientId={}",
        base_url.replacen("http", "ws", 1),
        client_id
    );
    let Ok((mut stream, _)) = tokio_tungstenite::connect_async(&ws_url).await else {
        tracing::warn!(%ws_url, "ComfyUI WebSocket se nepřipojil — průběh bez procent");
        return;
    };

    while let Some(Ok(message)) = stream.next().await {
        if let tokio_tungstenite::tungstenite::Message::Text(text) = message {
            if let Some((step, total)) = parse_ws_progress(&text) {
                let _ = tx.send(ImageProgress::Progress { step, total }).await;
            }
        }
    }
}

/// Vytáhne (value, max) ze zprávy `{"type":"progress","data":{"value":..,"max":..}}`.
fn parse_ws_progress(text: &str) -> Option<(u32, u32)> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    if value.get("type")?.as_str()? != "progress" {
        return None;
    }
    let data = value.get("data")?;
    Some((
        data.get("value")?.as_u64()? as u32,
        data.get("max")?.as_u64()? as u32,
    ))
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

/// Vytáhne z `/history` záznamu běhovou chybu ComfyUI, pokud nastala.
/// ComfyUI u neúspěšného promptu nastaví `status.status_str = "error"` a do
/// `status.messages` přidá dvojici `["execution_error", { exception_message, … }]`.
/// Vrací krátký popis chyby, nebo `None` když prompt (zatím) neselhal.
fn extract_execution_error(entry: &serde_json::Value) -> Option<String> {
    let status = entry.get("status")?;
    if status.get("status_str").and_then(|s| s.as_str()) != Some("error") {
        return None;
    }
    let detail = status
        .get("messages")
        .and_then(|m| m.as_array())
        .and_then(|msgs| {
            msgs.iter().find_map(|m| {
                let arr = m.as_array()?;
                if arr.first()?.as_str()? != "execution_error" {
                    return None;
                }
                arr.get(1)?
                    .get("exception_message")?
                    .as_str()
                    .map(|s| s.to_string())
            })
        })
        .unwrap_or_else(|| "neznámá chyba".to_string());
    Some(detail)
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
            lora_file: None,
            init_image_path: None,
            hires_fix: false,
            pulid_weight: 1.0,
            face_detailer: false,
        }
    }

    #[test]
    fn extract_execution_error_reads_status() {
        // Neúspěch (OOM apod.) → vrátí popis chyby
        let failed = serde_json::json!({
            "status": {
                "status_str": "error",
                "messages": [
                    ["execution_start", {}],
                    ["execution_error", { "exception_message": "CUDA out of memory" }]
                ]
            }
        });
        assert_eq!(
            extract_execution_error(&failed).as_deref(),
            Some("CUDA out of memory")
        );

        // Úspěch / probíhající → None
        let ok = serde_json::json!({ "status": { "status_str": "success" } });
        assert!(extract_execution_error(&ok).is_none());
        let running = serde_json::json!({ "outputs": {} });
        assert!(extract_execution_error(&running).is_none());
    }

    #[test]
    fn hires_fix_adds_second_sampler_pass() {
        let mut req = sample_request();
        req.hires_fix = true;
        let workflow = build_basic_workflow(&req, &[], None);

        // Upscale (17) + druhý KSampler (18) existují a decode bere z 18.
        assert_eq!(workflow["17"]["class_type"], "LatentUpscale");
        assert_eq!(workflow["18"]["class_type"], "KSampler");
        assert_eq!(workflow["18"]["inputs"]["latent_image"][0], "17");
        assert_eq!(workflow["6"]["inputs"]["samples"][0], "18");
        // Upscale zvětšuje 1,5× rozměry z requestu.
        assert_eq!(workflow["17"]["inputs"]["width"], (1024.0 * 1.5) as u32);

        // Bez hires_fix se nic nepřidá a decode bere z hlavního sampleru (5).
        let plain = build_basic_workflow(&sample_request(), &[], None);
        assert!(plain.get("17").is_none());
        assert_eq!(plain["6"]["inputs"]["samples"][0], "5");
    }

    #[test]
    fn pulid_weight_from_request_drives_apply_pulid() {
        let mut req = sample_request();
        req.pulid_weight = 0.65;
        let workflow = build_basic_workflow(&req, &["ref.png".into()], None);

        assert_eq!(workflow["12"]["class_type"], "ApplyPulid");
        let weight = workflow["12"]["inputs"]["weight"].as_f64().unwrap();
        assert!((weight - 0.65).abs() < 1e-6, "weight = {weight}");
        assert_eq!(workflow["12"]["inputs"]["method"], "fidelity");
    }

    #[test]
    fn face_detailer_appends_detector_and_rewires_save() {
        let mut req = sample_request();
        req.face_detailer = true;
        let workflow = build_basic_workflow(&req, &["ref.png".into()], None);

        // Detektor (40) + FaceDetailer (41) existují a jsou správně propojené
        assert_eq!(workflow["40"]["class_type"], "UltralyticsDetectorProvider");
        assert_eq!(
            workflow["40"]["inputs"]["model_name"],
            crate::comfy_installer::FACE_DETECTOR_MODEL_NAME
        );
        assert_eq!(workflow["41"]["class_type"], "FaceDetailer");
        assert_eq!(
            workflow["41"]["inputs"]["image"],
            serde_json::json!(["6", 0])
        );
        assert_eq!(
            workflow["41"]["inputs"]["bbox_detector"],
            serde_json::json!(["40", 0])
        );
        // U reference přebírá PuLID-patchnutý model (uzel 12) → drží podobu
        assert_eq!(
            workflow["41"]["inputs"]["model"],
            serde_json::json!(["12", 0])
        );
        // SaveImage (7) teď čte z FaceDetaileru, ne přímo z decode (6)
        assert_eq!(
            workflow["7"]["inputs"]["images"],
            serde_json::json!(["41", 0])
        );

        // Bez přepínače se nic nepřidá a SaveImage čte z decode (6)
        let plain = build_basic_workflow(&sample_request(), &["ref.png".into()], None);
        assert!(plain.get("40").is_none());
        assert!(plain.get("41").is_none());
        assert_eq!(plain["7"]["inputs"]["images"], serde_json::json!(["6", 0]));
    }

    #[test]
    fn face_detailer_runs_after_hires_reads_final_decode() {
        // Kombinace hi-res + FaceDetailer: decode (6) bere z hi-res sampleru (18),
        // FaceDetailer bere hotový obrázek z decode (6).
        let mut req = sample_request();
        req.hires_fix = true;
        req.face_detailer = true;
        let workflow = build_basic_workflow(&req, &[], None);

        assert_eq!(workflow["6"]["inputs"]["samples"][0], "18");
        assert_eq!(
            workflow["41"]["inputs"]["image"],
            serde_json::json!(["6", 0])
        );
    }

    #[test]
    fn init_image_switches_latent_to_vaeencode() {
        let req = sample_request();

        // txt2img + init: latent z VAEEncode, denoise 0.55
        let workflow = build_basic_workflow(&req, &[], Some("edit-me.png"));
        assert_eq!(workflow["15"]["class_type"], "LoadImage");
        assert_eq!(workflow["15"]["inputs"]["image"], "edit-me.png");
        assert_eq!(workflow["16"]["class_type"], "VAEEncode");
        assert_eq!(
            workflow["5"]["inputs"]["latent_image"],
            serde_json::json!(["16", 0])
        );
        assert_eq!(workflow["5"]["inputs"]["denoise"], 0.55);

        // Bez init zůstává prázdný latent a plný denoise
        let plain = build_basic_workflow(&req, &[], None);
        assert_eq!(
            plain["5"]["inputs"]["latent_image"],
            serde_json::json!(["4", 0])
        );
        assert_eq!(plain["5"]["inputs"]["denoise"], 1.0);

        // Kombinace s PuLID referencí funguje taky
        let combined = build_basic_workflow(&req, &["face.png".into()], Some("edit-me.png"));
        assert_eq!(
            combined["5"]["inputs"]["latent_image"],
            serde_json::json!(["16", 0])
        );
        assert_eq!(
            combined["5"]["inputs"]["model"],
            serde_json::json!(["12", 0])
        );
    }

    #[test]
    fn parse_ws_progress_reads_sampler_steps() {
        assert_eq!(
            parse_ws_progress(r#"{"type":"progress","data":{"value":12,"max":20}}"#),
            Some((12, 20))
        );
        assert_eq!(
            parse_ws_progress(r#"{"type":"executing","data":{"node":"5"}}"#),
            None
        );
        assert_eq!(parse_ws_progress("neni json"), None);
    }

    #[test]
    fn lora_is_wired_between_checkpoint_and_consumers() {
        let mut req = sample_request();
        req.lora_file = Some("nikol_v1.safetensors".into());

        // txt2img: KSampler bere model z LoRA, text encody clip z LoRA
        let workflow = build_basic_workflow(&req, &[], None);
        assert_eq!(workflow["14"]["class_type"], "LoraLoader");
        assert_eq!(
            workflow["14"]["inputs"]["lora_name"],
            "nikol_v1.safetensors"
        );
        assert_eq!(
            workflow["5"]["inputs"]["model"],
            serde_json::json!(["14", 0])
        );
        assert_eq!(
            workflow["2"]["inputs"]["clip"],
            serde_json::json!(["14", 1])
        );

        // PuLID větev: ApplyPulid bere model z LoRA, KSampler dál z PuLID
        let pulid = build_basic_workflow(&req, &["ref.png".into()], None);
        assert_eq!(pulid["12"]["inputs"]["model"], serde_json::json!(["14", 0]));
        assert_eq!(pulid["5"]["inputs"]["model"], serde_json::json!(["12", 0]));

        // Pony (clip skip): řetěz LoRA → CLIPSetLastLayer → text encody
        req.style_preset = StylePreset::Anime;
        let pony = build_basic_workflow(&req, &[], None);
        assert_eq!(pony["13"]["inputs"]["clip"], serde_json::json!(["14", 1]));
        assert_eq!(pony["2"]["inputs"]["clip"], serde_json::json!(["13", 0]));
    }

    #[test]
    fn txt2img_workflow_used_without_reference_image() {
        let req = sample_request();
        let workflow = build_basic_workflow(&req, &[], None);

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

        let workflow = build_basic_workflow(&req, &[], None);
        assert_eq!(
            workflow["1"]["inputs"]["ckpt_name"],
            crate::comfy_installer::PONY_CHECKPOINT_FILENAME
        );

        // Pony checkpoint je SDXL architektura → platí i pro PuLID větev
        let pulid = build_basic_workflow(&req, &["ref.png".into()], None);
        assert_eq!(
            pulid["1"]["inputs"]["ckpt_name"],
            crate::comfy_installer::PONY_CHECKPOINT_FILENAME
        );
    }

    #[test]
    fn pony_styles_get_score_tags_and_clip_skip() {
        let mut req = sample_request();
        req.style_preset = StylePreset::Anime;

        let workflow = build_basic_workflow(&req, &[], None);
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
        let semi = build_basic_workflow(&req, &[], None);
        let semi_positive = semi["2"]["inputs"]["text"].as_str().unwrap();
        assert!(semi_positive.contains("realistic, "));
        assert!(!semi_positive.contains("source_anime"));
    }

    #[test]
    fn realistic_style_has_no_score_tags_nor_clip_skip() {
        let req = sample_request();
        let workflow = build_basic_workflow(&req, &[], None);

        assert_eq!(workflow["2"]["inputs"]["text"], "kosmická loď nad planetou");
        assert!(workflow.get("13").is_none());
        assert_eq!(workflow["2"]["inputs"]["clip"], serde_json::json!(["1", 1]));
    }

    #[test]
    fn flux_workflow_carries_over_request_fields() {
        let req = sample_request();
        let workflow = build_basic_workflow(&req, &[], None);

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

        let workflow = build_basic_workflow(&req, &[], None);

        assert_eq!(workflow["3"]["inputs"]["text"], "");
        assert_eq!(workflow["5"]["inputs"]["seed"], 42);
    }

    #[test]
    fn pulid_workflow_used_when_reference_image_present() {
        let req = sample_request();
        let workflow = build_basic_workflow(&req, &["uploaded-ref.png".into()], None);

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
        let workflow = build_basic_workflow(&req, &refs, None);

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
        let workflow = build_basic_workflow(&req, &["uploaded-ref.png".into()], None);

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
        let workflow = build_basic_workflow(&req, &["uploaded-ref.png".into()], None);

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

    fn checkpoint_object_info(names: &[&str]) -> serde_json::Value {
        serde_json::json!({
            "CheckpointLoaderSimple": {
                "input": { "required": { "ckpt_name": [names, { "tooltip": "x" }] } }
            }
        })
    }

    #[tokio::test]
    async fn verify_checkpoint_errors_when_not_in_running_comfyui() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/object_info/CheckpointLoaderSimple"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(checkpoint_object_info(&["sd_xl_base_1.0.safetensors"])),
            )
            .mount(&server)
            .await;

        let client = ComfyUiClient::new(server.uri());
        let result = client
            .verify_checkpoint_available("RealVisXL_V5.0_fp16.safetensors")
            .await;

        let err = result.unwrap_err().to_string();
        assert!(err.contains("RealVisXL_V5.0_fp16.safetensors"));
        assert!(
            err.contains("sd_xl_base_1.0.safetensors"),
            "chyba vypíše dostupné modely"
        );
    }

    #[tokio::test]
    async fn verify_checkpoint_ok_when_present() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/object_info/CheckpointLoaderSimple"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(checkpoint_object_info(&[
                    "sd_xl_base_1.0.safetensors",
                    "RealVisXL_V5.0_fp16.safetensors",
                ])),
            )
            .mount(&server)
            .await;

        let client = ComfyUiClient::new(server.uri());
        assert!(client
            .verify_checkpoint_available("RealVisXL_V5.0_fp16.safetensors")
            .await
            .is_ok());
    }

    #[tokio::test]
    async fn verify_node_available_true_when_object_info_lists_it() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/object_info/FaceDetailer"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "FaceDetailer": { "input": { "required": {} } }
            })))
            .mount(&server)
            .await;

        let client = ComfyUiClient::new(server.uri());
        assert!(client.verify_node_available("FaceDetailer").await);
    }

    #[tokio::test]
    async fn verify_node_available_false_when_absent_or_unreachable() {
        // 404 (uzel/instalace ho nezná) → false, workflow se poskládá bez něj
        let server = MockServer::start().await;
        let client = ComfyUiClient::new(server.uri());
        assert!(!client.verify_node_available("FaceDetailer").await);
    }

    #[tokio::test]
    async fn verify_checkpoint_skips_when_object_info_unavailable() {
        // Nedostupné /object_info (404) → kontrola se přeskočí (Ok), chybu
        // ať případně ukáže až /prompt.
        let server = MockServer::start().await;
        let client = ComfyUiClient::new(server.uri());
        assert!(client
            .verify_checkpoint_available("cokoliv.safetensors")
            .await
            .is_ok());
    }
}
