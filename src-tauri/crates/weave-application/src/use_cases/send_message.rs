use std::sync::Arc;

use tokio::sync::mpsc;
use weave_domain::{
    conversation::ConversationId,
    message::{Attachment, Message},
    model::IntentClassifier,
};

use crate::{
    error::{AppError, AppResult},
    ports::{
        attachment_store_port::AttachmentStorePort,
        comfy_installer_port::ComfyInstallerPort,
        conversation_repository::{ConversationRepository, MessageRepository},
        generation_settings_repository::GenerationSettingsRepository,
        image_gen_port::{ImageGenPort, ImageProgress, ImageRequest, StylePreset},
        llm_port::{ChatRequest, LlmPort, StreamChunk},
        lora_catalog_port::LoraCatalogPort,
        persona_repository::PersonaRepository,
        workspace_port::WorkspaceRepository,
    },
};

/// Instrukce pro převod požadavku na anglický Stable Diffusion prompt
/// + návrh konceptu pro vyhledání LoRA (druhý řádek).
const IMAGE_PROMPT_SYSTEM: &str = "You convert user requests into English Stable Diffusion \
    prompts. Reply with EXACTLY two lines:\n\
    Line 1: comma-separated English descriptors (subject, appearance, setting, lighting, \
    style, quality tags like 'highly detailed, sharp focus'). Translate non-English requests. \
    Keep every detail the user asked for, including names used for people.\n\
    Line 2: 'LORA: <2-4 English words naming a specific character, celebrity, art style or \
    concept a LoRA model could exist for>' or 'LORA: none' when the request is generic.\n\
    No explanations, no quotes.";

/// Výchozí negative prompt — potlačuje typické artefakty SDXL. Oční
/// artefakty (šilhání, deformované duhovky, asymetrie) jsou u PuLID
/// obzvlášť časté, proto explicitně.
const DEFAULT_NEGATIVE_PROMPT: &str = "blurry, low quality, deformed, disfigured, extra limbs, \
    bad anatomy, bad hands, watermark, text, signature, jpeg artifacts, \
    bad eyes, deformed eyes, deformed iris, deformed pupils, extra pupils, \
    cross-eyed, asymmetric eyes, misaligned eyes";

/// Tagy pro věrné oči — přidávají se k pozitivnímu promptu u generování
/// podle referenční fotky (PuLID = portrét osoby, kde oči nejvíc „táhnou").
const EYE_QUALITY_TAGS: &str = "detailed symmetric eyes, natural eyes, sharp focus";

/// Rozparsuje výstup LLM vylepšení promptu: řádky bez prefixu LORA: tvoří
/// prompt, řádek `LORA: <koncept>` je dotaz pro katalog LoRA (`none`/prázdno
/// = žádný). Když je prompt prázdný, vrací fallback.
fn parse_prompt_enhancement(raw: &str, fallback: &str) -> (String, Option<String>) {
    let mut prompt_lines: Vec<&str> = Vec::new();
    let mut lora = None;
    for line in raw.lines() {
        let trimmed = line.trim().trim_matches('"');
        if trimmed.len() >= 5 && trimmed[..5].eq_ignore_ascii_case("lora:") {
            let value = trimmed[5..].trim().trim_matches('"');
            if !value.is_empty() && !value.eq_ignore_ascii_case("none") {
                lora = Some(value.to_string());
            }
        } else if !trimmed.is_empty() {
            prompt_lines.push(trimmed);
        }
    }
    let prompt = prompt_lines.join(" ");
    if prompt.is_empty() {
        (fallback.to_string(), lora)
    } else {
        (prompt, lora)
    }
}

pub struct SendMessageUseCase {
    conv_repo: Arc<dyn ConversationRepository>,
    msg_repo: Arc<dyn MessageRepository>,
    llm: Arc<dyn LlmPort>,
    image_gen: Arc<dyn ImageGenPort>,
    workspace_repo: Arc<dyn WorkspaceRepository>,
    persona_repo: Arc<dyn PersonaRepository>,
    attachment_store: Arc<dyn AttachmentStorePort>,
    gen_settings_repo: Arc<dyn GenerationSettingsRepository>,
    comfy_installer: Arc<dyn ComfyInstallerPort>,
    lora_catalog: Arc<dyn LoraCatalogPort>,
}

impl SendMessageUseCase {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        conv_repo: Arc<dyn ConversationRepository>,
        msg_repo: Arc<dyn MessageRepository>,
        llm: Arc<dyn LlmPort>,
        image_gen: Arc<dyn ImageGenPort>,
        workspace_repo: Arc<dyn WorkspaceRepository>,
        persona_repo: Arc<dyn PersonaRepository>,
        attachment_store: Arc<dyn AttachmentStorePort>,
        gen_settings_repo: Arc<dyn GenerationSettingsRepository>,
        comfy_installer: Arc<dyn ComfyInstallerPort>,
        lora_catalog: Arc<dyn LoraCatalogPort>,
    ) -> Self {
        Self {
            conv_repo,
            msg_repo,
            llm,
            image_gen,
            workspace_repo,
            persona_repo,
            attachment_store,
            gen_settings_repo,
            comfy_installer,
            lora_catalog,
        }
    }

    pub async fn execute(
        &self,
        conversation_id: ConversationId,
        content: String,
        file_refs: Vec<String>,
        reference_images: Vec<String>,
        stream_tx: mpsc::Sender<StreamChunk>,
    ) -> AppResult<()> {
        // Ověříme že konverzace existuje a získáme její personu
        let conversation = self
            .conv_repo
            .find_by_id(&conversation_id)
            .await?
            .ok_or_else(|| {
                AppError::Repository(format!("Konverzace {conversation_id} neexistuje"))
            })?;

        // Referenční obrázky zkopírujeme do vlastního úložiště appky (přežije
        // i smazání/přesun originálu) a uložíme je jako přílohy zprávy.
        let mut attachments = Vec::with_capacity(reference_images.len());
        for path in &reference_images {
            let stored = self.attachment_store.store_reference_image(path).await?;
            attachments.push(Attachment::Image {
                path: stored.path,
                mime: stored.mime,
            });
        }

        // Uložíme zprávu uživatele
        let user_msg =
            Message::user(conversation_id.clone(), &content).with_attachments(attachments.clone());
        self.msg_repo.save(&user_msg).await?;

        let reference_image_paths: Vec<String> = attachments
            .iter()
            .filter_map(|a| match a {
                Attachment::Image { path, .. } => Some(path.clone()),
                Attachment::Document { .. } => None,
            })
            .collect();

        self.generate_reply(
            &conversation,
            conversation_id,
            content,
            file_refs,
            reference_image_paths,
            None,
            stream_tx,
        )
        .await
    }

    /// Znovu vygeneruje odpověď na poslední zprávu uživatele: smaže poslední
    /// odpověď asistenta (z DB) a spustí generování nad zbylou historií.
    pub async fn regenerate(
        &self,
        conversation_id: ConversationId,
        stream_tx: mpsc::Sender<StreamChunk>,
    ) -> AppResult<()> {
        let conversation = self
            .conv_repo
            .find_by_id(&conversation_id)
            .await?
            .ok_or_else(|| {
                AppError::Repository(format!("Konverzace {conversation_id} neexistuje"))
            })?;

        self.msg_repo
            .delete_trailing_assistant_messages(&conversation_id)
            .await?;

        let history = self.msg_repo.list_by_conversation(&conversation_id).await?;
        let last_user = history
            .iter()
            .rev()
            .find(|m| m.role == weave_domain::message::Role::User)
            .ok_or_else(|| {
                AppError::Repository(
                    "Není co znovu vygenerovat — konverzace nemá žádnou zprávu uživatele".into(),
                )
            })?;

        let content = last_user.content.clone();
        let reference_image_paths: Vec<String> = last_user
            .attachments
            .iter()
            .filter_map(|a| match a {
                Attachment::Image { path, .. } => Some(path.clone()),
                Attachment::Document { .. } => None,
            })
            .collect();

        // Obsah @souborů z původní zprávy se neukládá, takže regenerace běží
        // jen nad uloženou historií (bez file kontextu).
        self.generate_reply(
            &conversation,
            conversation_id,
            content,
            vec![],
            reference_image_paths,
            None,
            stream_tx,
        )
        .await
    }

    /// „Poslat znovu": smaže vše PO dané zprávě uživatele (další dotazy
    /// i odpovědi) a vygeneruje na ni čerstvou odpověď — konverzace se
    /// vrátí do stavu těsně po tomto dotazu.
    pub async fn resend(
        &self,
        conversation_id: ConversationId,
        message_id: weave_domain::message::MessageId,
        stream_tx: mpsc::Sender<StreamChunk>,
    ) -> AppResult<()> {
        self.msg_repo
            .delete_messages_after(&conversation_id, &message_id)
            .await?;
        self.regenerate(conversation_id, stream_tx).await
    }

    /// Úprava vygenerovaného obrázku (img2img): instrukce uživatele se uloží
    /// jako zpráva s náhledem upravovaného obrázku a generuje se z něj
    /// (latent z init obrázku, denoise 0.55 — kompozice zůstává).
    pub async fn edit_image(
        &self,
        conversation_id: ConversationId,
        content: String,
        init_image: String,
        stream_tx: mpsc::Sender<StreamChunk>,
    ) -> AppResult<()> {
        let conversation = self
            .conv_repo
            .find_by_id(&conversation_id)
            .await?
            .ok_or_else(|| {
                AppError::Repository(format!("Konverzace {conversation_id} neexistuje"))
            })?;

        // Náhled upravovaného obrázku u zprávy — soubor už je v galerii
        // (spravované úložiště), takže se nekopíruje znovu.
        let user_msg = Message::user(conversation_id.clone(), &content).with_attachments(vec![
            Attachment::Image {
                path: init_image.clone(),
                mime: "image/png".into(),
            },
        ]);
        self.msg_repo.save(&user_msg).await?;

        self.generate_reply(
            &conversation,
            conversation_id,
            content,
            vec![],
            vec![],
            Some(init_image),
            stream_tx,
        )
        .await
    }

    /// Společné jádro generování: obalí stream tak, aby se hotová odpověď
    /// asistenta uložila do DB (jinak by po přepnutí konverzace zmizela),
    /// a routuje podle záměru na obrázek/text.
    #[allow(clippy::too_many_arguments)]
    async fn generate_reply(
        &self,
        conversation: &weave_domain::conversation::Conversation,
        conversation_id: ConversationId,
        content: String,
        file_refs: Vec<String>,
        reference_image_paths: Vec<String>,
        init_image: Option<String>,
        stream_tx: mpsc::Sender<StreamChunk>,
    ) -> AppResult<()> {
        let intent = IntentClassifier::classify(&content);
        tracing::debug!(?intent, "Intent klasifikován");

        // Mezikanál: skládá text odpovědi průběžně a po dokončení (nebo po
        // zastavení uživatelem — tehdy se uloží aspoň částečná odpověď) ji
        // zapíše do DB. Chunky přeposílá dál beze změny.
        let (tee_tx, mut tee_rx) = mpsc::channel::<StreamChunk>(128);
        let persist_repo = self.msg_repo.clone();
        let persist_conv = conversation_id.clone();
        let persist = tokio::spawn(async move {
            let mut text = String::new();
            let mut stats = None;
            while let Some(chunk) = tee_rx.recv().await {
                match &chunk {
                    StreamChunk::Token(t) => text.push_str(t),
                    StreamChunk::Done(s) => stats = Some(s.clone()),
                    StreamChunk::Error(_) | StreamChunk::ImageStage(_) => {}
                }
                if stream_tx.send(chunk).await.is_err() {
                    break; // příjemce zmizel (Zastavit) — částečný text přesto uložíme
                }
            }
            if !text.is_empty() {
                let msg = Message::assistant(persist_conv, text, stats);
                if let Err(e) = persist_repo.save(&msg).await {
                    tracing::error!("Uložení odpovědi asistenta selhalo: {e}");
                }
            }
        });

        // Úprava obrázku (init_image) je vždy generování obrázku, bez ohledu
        // na to, jak by heuristika klasifikovala samotný text instrukce.
        let is_image =
            init_image.is_some() || matches!(intent, weave_domain::model::Intent::ImageGeneration);
        let result = match is_image {
            true => {
                self.handle_image(content, reference_image_paths, init_image, tee_tx)
                    .await
            }
            false => {
                let mut history = self.msg_repo.list_by_conversation(&conversation_id).await?;

                // Přiložené @soubory → system kontext na začátku (neukládá se do historie)
                if let Some(context) = self.build_file_context(&file_refs).await? {
                    history.insert(0, Message::system(conversation_id.clone(), context));
                }

                // Persona konverzace → system prompt úplně na začátek
                if let Some(prompt) = self
                    .resolve_persona_prompt(&conversation.persona_id)
                    .await?
                {
                    history.insert(0, Message::system(conversation_id.clone(), prompt));
                }

                // Per-konverzační parametry (posuvníky v chatu)
                let gen = self.gen_settings_repo.get(&conversation_id).await?;

                let model_id = Self::model_for_intent(&intent);
                let request = ChatRequest {
                    messages: history,
                    model_id,
                    max_tokens: gen.max_tokens,
                    temperature: gen.temperature_or_default(),
                    context_length: gen.context_length,
                    stream: true,
                };
                self.llm.chat_stream(request, tee_tx).await
            }
        };

        // Počkej na doběhnutí ukládací smyčky — u vestavěné inference tím
        // příkaz skončí až po dokončení generování, což je žádoucí.
        let _ = persist.await;
        result
    }

    /// Vyřeší system prompt persony konverzace (vestavěná z domény, vlastní z repo).
    async fn resolve_persona_prompt(
        &self,
        persona_id: &Option<String>,
    ) -> AppResult<Option<String>> {
        let Some(id) = persona_id else {
            return Ok(None);
        };

        if weave_domain::persona::Persona::is_builtin(id) {
            return Ok(weave_domain::persona::builtin_personas()
                .into_iter()
                .find(|p| &p.id == id)
                .map(|p| p.system_prompt));
        }

        Ok(self
            .persona_repo
            .find_by_id(id)
            .await?
            .map(|p| p.system_prompt))
    }

    /// Sestaví system kontext z obsahu @souborů (z workspace indexu).
    /// Vrátí None pokud nejsou žádné reference nebo žádný soubor nemá obsah.
    async fn build_file_context(&self, file_refs: &[String]) -> AppResult<Option<String>> {
        if file_refs.is_empty() {
            return Ok(None);
        }

        let mut context = String::from("Uživatel přiložil tyto soubory jako kontext k dotazu:\n\n");
        let mut any = false;

        for path in file_refs {
            if let Some(file) = self.workspace_repo.get_file(path).await? {
                if file.text_content.is_empty() {
                    continue;
                }
                context.push_str(&format!(
                    "### {}\n```\n{}\n```\n\n",
                    file.name, file.text_content
                ));
                any = true;
            }
        }

        Ok(if any { Some(context) } else { None })
    }

    /// Kompletní pipeline generování obrázku: zkontroluje prostředí, případně
    /// doinstaluje ComfyUI, stáhne model podle stylu promptu, spustí server,
    /// vygeneruje, a nakonec server zastaví (uvolní VRAM — soubory modelů
    /// zůstávají na disku pro příště). Průběh hlásí přes ImageStage chunky.
    async fn handle_image(
        &self,
        prompt: String,
        reference_image_paths: Vec<String>,
        init_image: Option<String>,
        stream_tx: mpsc::Sender<StreamChunk>,
    ) -> AppResult<()> {
        use crate::ports::comfy_installer_port::ComfyStatus;
        use crate::ports::llm_port::{ImageStage, ImageStageInfo};

        let style = StylePreset::classify(&prompt);
        let stage = |s: ImageStage| {
            StreamChunk::ImageStage(ImageStageInfo {
                stage: s,
                detail: None,
                percent: None,
            })
        };

        // 1. Prostředí — je ComfyUI vůbec nainstalované?
        let _ = stream_tx.send(stage(ImageStage::Checking)).await;
        if self.comfy_installer.status().await? == ComfyStatus::NotInstalled {
            let _ = stream_tx.send(stage(ImageStage::Installing)).await;
            let (itx, irx) = mpsc::channel(64);
            let fwd = tokio::spawn(forward_install_progress(
                ImageStage::Installing,
                irx,
                stream_tx.clone(),
            ));
            let result = self.comfy_installer.install(itx).await;
            let _ = fwd.await;
            result?;
        }

        // 2. Model podle stylu promptu (stáhne se, jen když chybí)
        {
            let (itx, irx) = mpsc::channel(64);
            let fwd = tokio::spawn(forward_install_progress(
                ImageStage::DownloadingModel,
                irx,
                stream_tx.clone(),
            ));
            let result = self
                .comfy_installer
                .ensure_style_checkpoint(style, itx)
                .await;
            let _ = fwd.await;
            result?;
        }

        // 2b. Assety pro referenční obrázek (PuLID node, váhy, InsightFace) —
        // instalace z dřívějších verzí appky je mít nemusí, workflow by pak
        // spadl na "pulid_file not in []".
        if !reference_image_paths.is_empty() {
            let (itx, irx) = mpsc::channel(64);
            let fwd = tokio::spawn(forward_install_progress(
                ImageStage::DownloadingModel,
                irx,
                stream_tx.clone(),
            ));
            let result = self.comfy_installer.ensure_reference_assets(itx).await;
            let _ = fwd.await;
            result?;
        }

        // 3. Server
        if self.comfy_installer.status().await? != ComfyStatus::Running {
            let _ = stream_tx.send(stage(ImageStage::StartingServer)).await;
            self.comfy_installer.start_server().await?;
        }

        // 3b. Prompt pro SDXL — model rozumí jen anglickým, komma-separovaným
        // popisům. Český požadavek („nakresli mi…") poslaný napřímo vede na
        // nesmyslné výstupy, proto ho LLM nejdřív převede; při selhání LLM
        // se použije původní text (generování kvůli tomu nespadne). LLM
        // zároveň navrhne koncept pro vyhledání LoRA.
        let _ = stream_tx.send(stage(ImageStage::PreparingPrompt)).await;
        let (mut sd_prompt, lora_query) = self.enhance_image_prompt(&prompt).await;

        // U generování podle reference (PuLID) jde vždy o osobu — přidáme
        // tagy na věrné oči, nejčastější zdroj „divných" výsledků.
        if !reference_image_paths.is_empty() {
            sd_prompt = format!("{sd_prompt}, {EYE_QUALITY_TAGS}");
        }

        tracing::info!(original = %prompt, enhanced = %sd_prompt, ?lora_query,
            "Prompt pro generátor obrázků");

        // 3c. LoRA — když LLM navrhl koncept, zkusíme na CivitAI najít
        // vhodnou LoRA, stáhnout ji (pokud chybí) a zapojit: soubor do
        // workflow, trigger words do promptu. Jakékoli selhání = generuje
        // se bez LoRA, nikdy to neshodí celou pipeline.
        let mut lora_file = None;
        if let Some(query) = lora_query {
            let base_model = match style {
                StylePreset::SemiRealistic | StylePreset::Anime => "Pony",
                _ => "SDXL 1.0",
            };
            let _ = stream_tx
                .send(StreamChunk::ImageStage(ImageStageInfo {
                    stage: ImageStage::DownloadingModel,
                    detail: Some(format!("Hledám LoRA: {query}")),
                    percent: None,
                }))
                .await;
            match self.lora_catalog.find_lora(&query, base_model).await {
                Ok(Some(lora)) => {
                    let (itx, irx) = mpsc::channel(64);
                    let fwd = tokio::spawn(forward_install_progress(
                        ImageStage::DownloadingModel,
                        irx,
                        stream_tx.clone(),
                    ));
                    let result = self
                        .comfy_installer
                        .ensure_lora(&lora.file_name, &lora.download_url, itx)
                        .await;
                    let _ = fwd.await;
                    match result {
                        Ok(()) => {
                            if !lora.trigger_words.is_empty() {
                                sd_prompt =
                                    format!("{sd_prompt}, {}", lora.trigger_words.join(", "));
                            }
                            tracing::info!(name = %lora.name, file = %lora.file_name,
                                "LoRA zapojena do generování");
                            lora_file = Some(lora.file_name);
                        }
                        Err(e) => {
                            tracing::warn!("Stažení LoRA selhalo ({e}) — generuji bez ní");
                        }
                    }
                }
                Ok(None) => tracing::info!(%query, "Žádná vhodná LoRA nenalezena"),
                Err(e) => tracing::warn!("Hledání LoRA selhalo ({e}) — generuji bez ní"),
            }
        }

        // 4. Generování
        let _ = stream_tx.send(stage(ImageStage::Generating)).await;

        let (img_tx, mut img_rx) = mpsc::channel(32);
        let request = ImageRequest {
            prompt: sd_prompt,
            negative_prompt: Some(DEFAULT_NEGATIVE_PROMPT.to_string()),
            width: 1024,
            height: 1024,
            steps: 20,
            cfg_scale: 7.0,
            seed: None,
            style_preset: style,
            reference_image_paths,
            lora_file,
            init_image_path: init_image,
        };

        let gen_result = self.image_gen.generate(request, img_tx).await;
        if gen_result.is_err() {
            // I po chybě uvolníme VRAM — server by jinak zůstal běžet naprázdno
            let _ = self.comfy_installer.stop_server().await;
        }
        gen_result?;

        while let Some(progress) = img_rx.recv().await {
            match progress {
                ImageProgress::Done { output_path } => {
                    // 5. Uvolnit VRAM — soubory modelů zůstávají pro příště
                    let _ = stream_tx.send(stage(ImageStage::Finishing)).await;
                    if let Err(e) = self.comfy_installer.stop_server().await {
                        tracing::warn!("Zastavení ComfyUI po generování selhalo: {e}");
                    }
                    let _ = stream_tx
                        .send(StreamChunk::Token(format!("![obrázek]({output_path})")))
                        .await;
                    let _ = stream_tx.send(StreamChunk::Done(Default::default())).await;
                }
                ImageProgress::Error(e) => {
                    let _ = stream_tx.send(StreamChunk::Error(e)).await;
                }
                ImageProgress::Progress { step, total } => {
                    // Skutečné kroky sampleru (ComfyUI WebSocket) → progress bar
                    let percent = (step * 100).checked_div(total).unwrap_or(0).min(100) as u8;
                    let _ = stream_tx
                        .send(StreamChunk::ImageStage(ImageStageInfo {
                            stage: ImageStage::Generating,
                            detail: Some(format!("Krok {step}/{total}")),
                            percent: Some(percent),
                        }))
                        .await;
                }
            }
        }
        Ok(())
    }

    /// Převede požadavek uživatele (typicky česky) na anglický Stable
    /// Diffusion prompt + volitelný koncept pro vyhledání LoRA. Při
    /// jakémkoli selhání LLM vrací původní text — horší prompt je lepší
    /// než spadlé generování.
    async fn enhance_image_prompt(&self, user_prompt: &str) -> (String, Option<String>) {
        let conv = ConversationId::new();
        let messages = vec![
            Message::system(conv.clone(), IMAGE_PROMPT_SYSTEM),
            Message::user(conv, user_prompt),
        ];
        let request = ChatRequest {
            messages,
            model_id: "mistral-small-latest".into(),
            max_tokens: Some(200),
            context_length: None,
            temperature: 0.4,
            stream: true,
        };

        let (tx, mut rx) = mpsc::channel(64);
        if self.llm.chat_stream(request, tx).await.is_err() {
            return (user_prompt.to_string(), None);
        }
        let mut out = String::new();
        while let Some(chunk) = rx.recv().await {
            match chunk {
                StreamChunk::Token(t) => out.push_str(&t),
                StreamChunk::Error(e) => {
                    tracing::warn!("Vylepšení promptu selhalo ({e}) — použije se původní");
                    return (user_prompt.to_string(), None);
                }
                StreamChunk::Done(_) | StreamChunk::ImageStage(_) => {}
            }
        }
        parse_prompt_enhancement(&out, user_prompt)
    }

    fn model_for_intent(intent: &weave_domain::model::Intent) -> String {
        use weave_domain::model::Intent::*;
        match intent {
            TextChat => "mistral-small-latest",
            StoryWriting => "mistral-large-latest",
            CodeAssistance => "codestral-latest",
            Reasoning => "magistral-medium-latest",
            FileAnalysis => "pixtral-large-latest",
            ImageGeneration => unreachable!("Image handled separately"),
        }
        .to_string()
    }
}

/// Přeposílá průběh instalace/stahování (InstallProgress) do chat streamu
/// jako ImageStage chunky s detailem — uživatel vidí, co se právě děje.
async fn forward_install_progress(
    stage: crate::ports::llm_port::ImageStage,
    mut rx: mpsc::Receiver<crate::ports::comfy_installer_port::InstallProgress>,
    stream_tx: mpsc::Sender<StreamChunk>,
) {
    use crate::ports::comfy_installer_port::InstallProgress;
    use crate::ports::llm_port::ImageStageInfo;

    while let Some(progress) = rx.recv().await {
        let detail = match progress {
            InstallProgress::Step { name } => Some(name),
            InstallProgress::Output(line) => Some(line),
            InstallProgress::Error(e) => Some(e),
            InstallProgress::Done => None,
        };
        if let Some(detail) = detail {
            let _ = stream_tx
                .send(StreamChunk::ImageStage(ImageStageInfo {
                    stage,
                    detail: Some(detail),
                    percent: None,
                }))
                .await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::{
        attachment_store_port::{MockAttachmentStorePort, StoredImage},
        comfy_installer_port::{ComfyStatus, MockComfyInstallerPort},
        conversation_repository::{MockConversationRepository, MockMessageRepository},
        generation_settings_repository::MockGenerationSettingsRepository,
        image_gen_port::MockImageGenPort,
        llm_port::{ImageStage, MockLlmPort},
        lora_catalog_port::MockLoraCatalogPort,
        persona_repository::MockPersonaRepository,
        workspace_port::MockWorkspaceRepository,
    };
    use weave_domain::{conversation::Conversation, workspace::IndexedFile};

    /// Mock parametrů generování vracející výchozí (prázdné) hodnoty.
    fn default_gen_settings() -> MockGenerationSettingsRepository {
        let mut m = MockGenerationSettingsRepository::new();
        m.expect_get()
            .returning(|_| Box::pin(async { Ok(Default::default()) }));
        m
    }

    /// Mock LLM pro obrázkové testy: na žádost o vylepšení promptu vrátí
    /// anglický SD prompt (handle_image ho volá před generováním).
    fn image_prompt_llm() -> MockLlmPort {
        let mut llm = MockLlmPort::new();
        llm.expect_chat_stream().returning(|_, tx| {
            Box::pin(async move {
                let _ = tx
                    .send(StreamChunk::Token(
                        "young woman on a beach, detailed".into(),
                    ))
                    .await;
                let _ = tx.send(StreamChunk::Done(Default::default())).await;
                Ok(())
            })
        });
        llm
    }

    /// Mock LoRA katalogu: nikdy nic nenajde (výchozí pro testy, kde LoRA
    /// není předmětem — LLM mock stejně žádný LORA řádek nevrací).
    fn default_lora_catalog() -> MockLoraCatalogPort {
        let mut m = MockLoraCatalogPort::new();
        m.expect_find_lora()
            .returning(|_, _| Box::pin(async { Ok(None) }));
        m
    }

    /// Mock ComfyUI installeru: „vše připraveno, server běží" — orchestrace
    /// tak nic neinstaluje ani nespouští (výchozí pro testy mimo orchestraci).
    fn default_comfy_installer() -> MockComfyInstallerPort {
        let mut m = MockComfyInstallerPort::new();
        m.expect_status()
            .returning(|| Box::pin(async { Ok(ComfyStatus::Running) }));
        m.expect_ensure_style_checkpoint()
            .returning(|_, _| Box::pin(async { Ok(()) }));
        m.expect_ensure_reference_assets()
            .returning(|_| Box::pin(async { Ok(()) }));
        m.expect_stop_server()
            .returning(|| Box::pin(async { Ok(()) }));
        m
    }

    fn make_uc(ws: MockWorkspaceRepository) -> SendMessageUseCase {
        SendMessageUseCase::new(
            Arc::new(MockConversationRepository::new()),
            Arc::new(MockMessageRepository::new()),
            Arc::new(MockLlmPort::new()),
            Arc::new(MockImageGenPort::new()),
            Arc::new(ws),
            Arc::new(MockPersonaRepository::new()),
            Arc::new(MockAttachmentStorePort::new()),
            Arc::new(default_gen_settings()),
            Arc::new(default_comfy_installer()),
            Arc::new(default_lora_catalog()),
        )
    }

    fn dummy_conversation() -> Conversation {
        Conversation::new(weave_domain::conversation::ConversationTitle::new("Test").unwrap())
    }

    #[allow(clippy::too_many_arguments)]
    fn make_full_uc(
        conv_repo: MockConversationRepository,
        msg_repo: MockMessageRepository,
        llm: MockLlmPort,
        image_gen: MockImageGenPort,
        attachment_store: MockAttachmentStorePort,
    ) -> SendMessageUseCase {
        SendMessageUseCase::new(
            Arc::new(conv_repo),
            Arc::new(msg_repo),
            Arc::new(llm),
            Arc::new(image_gen),
            Arc::new(MockWorkspaceRepository::new()),
            Arc::new(MockPersonaRepository::new()),
            Arc::new(attachment_store),
            Arc::new(default_gen_settings()),
            Arc::new(default_comfy_installer()),
            Arc::new(default_lora_catalog()),
        )
    }

    #[tokio::test]
    async fn build_file_context_includes_file_content() {
        let mut ws = MockWorkspaceRepository::new();
        ws.expect_get_file().returning(|path: &str| {
            let name = path.rsplit(['/', '\\']).next().unwrap_or("f").to_string();
            Box::pin(async move {
                Ok(Some(IndexedFile {
                    path: format!("/ws/{name}"),
                    name,
                    extension: Some("txt".into()),
                    size_bytes: 12,
                    modified_at: chrono::Utc::now(),
                    indexed_at: chrono::Utc::now(),
                    text_content: "obsah souboru".into(),
                }))
            })
        });

        let uc = make_uc(ws);
        let ctx = uc
            .build_file_context(&["/ws/poznamky.txt".into()])
            .await
            .unwrap();

        assert!(ctx.is_some());
        let ctx = ctx.unwrap();
        assert!(ctx.contains("poznamky.txt"));
        assert!(ctx.contains("obsah souboru"));
    }

    #[tokio::test]
    async fn build_file_context_none_for_no_refs() {
        let uc = make_uc(MockWorkspaceRepository::new());
        assert!(uc.build_file_context(&[]).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn build_file_context_none_when_files_have_no_text() {
        let mut ws = MockWorkspaceRepository::new();
        ws.expect_get_file()
            .returning(|_| Box::pin(async { Ok(None) }));
        let uc = make_uc(ws);
        assert!(uc
            .build_file_context(&["/ws/binary.png".into()])
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn execute_stores_reference_image_as_attachment_on_text_chat() {
        let mut conv_repo = MockConversationRepository::new();
        conv_repo
            .expect_find_by_id()
            .returning(|_| Box::pin(async { Ok(Some(dummy_conversation())) }));

        let mut msg_repo = MockMessageRepository::new();
        msg_repo
            .expect_save()
            .withf(|m: &Message| {
                m.role == weave_domain::message::Role::User
                    && m.attachments
                        == vec![Attachment::Image {
                            path: "/data/weave/reference-images/stored.png".into(),
                            mime: "image/png".into(),
                        }]
            })
            .returning(|_| Box::pin(async { Ok(()) }));
        msg_repo
            .expect_list_by_conversation()
            .returning(|_| Box::pin(async { Ok(vec![]) }));

        let mut llm = MockLlmPort::new();
        llm.expect_chat_stream()
            .returning(|_, _| Box::pin(async { Ok(()) }));

        let mut attachment_store = MockAttachmentStorePort::new();
        attachment_store
            .expect_store_reference_image()
            .withf(|p: &str| p == "/tmp/original.png")
            .returning(|_| {
                Box::pin(async {
                    Ok(StoredImage {
                        path: "/data/weave/reference-images/stored.png".into(),
                        mime: "image/png".into(),
                    })
                })
            });

        let uc = make_full_uc(
            conv_repo,
            msg_repo,
            llm,
            MockImageGenPort::new(),
            attachment_store,
        );
        let (tx, _rx) = mpsc::channel(8);

        uc.execute(
            ConversationId::new(),
            "jak se dnes máš?".into(),
            vec![],
            vec!["/tmp/original.png".into()],
            tx,
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn execute_passes_reference_image_to_image_request_on_image_intent() {
        let mut conv_repo = MockConversationRepository::new();
        conv_repo
            .expect_find_by_id()
            .returning(|_| Box::pin(async { Ok(Some(dummy_conversation())) }));

        let mut msg_repo = MockMessageRepository::new();
        msg_repo
            .expect_save()
            .returning(|_| Box::pin(async { Ok(()) }));

        let mut image_gen = MockImageGenPort::new();
        image_gen
            .expect_generate()
            .withf(|req: &ImageRequest, _tx| {
                req.reference_image_paths == ["/data/weave/reference-images/stored.png"]
            })
            .returning(|_, _| Box::pin(async { Ok(()) }));

        let mut attachment_store = MockAttachmentStorePort::new();
        attachment_store
            .expect_store_reference_image()
            .returning(|_| {
                Box::pin(async {
                    Ok(StoredImage {
                        path: "/data/weave/reference-images/stored.png".into(),
                        mime: "image/png".into(),
                    })
                })
            });

        let uc = make_full_uc(
            conv_repo,
            msg_repo,
            image_prompt_llm(),
            image_gen,
            attachment_store,
        );
        let (tx, _rx) = mpsc::channel(8);

        uc.execute(
            ConversationId::new(),
            "nakresli mě jako rytíře".into(),
            vec![],
            vec!["/tmp/selfie.png".into()],
            tx,
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn edit_image_forces_image_intent_and_passes_init_image() {
        let mut conv_repo = MockConversationRepository::new();
        conv_repo
            .expect_find_by_id()
            .returning(|_| Box::pin(async { Ok(Some(dummy_conversation())) }));

        let mut msg_repo = MockMessageRepository::new();
        // Zpráva uživatele nese náhled upravovaného obrázku jako přílohu.
        msg_repo
            .expect_save()
            .withf(|m: &Message| {
                m.role != weave_domain::message::Role::User
                    || m.attachments
                        == vec![Attachment::Image {
                            path: "/data/weave/gallery/original.png".into(),
                            mime: "image/png".into(),
                        }]
            })
            .returning(|_| Box::pin(async { Ok(()) }));

        let mut image_gen = MockImageGenPort::new();
        image_gen
            .expect_generate()
            .withf(|req: &ImageRequest, _tx| {
                req.init_image_path.as_deref() == Some("/data/weave/gallery/original.png")
                    && req.reference_image_paths.is_empty()
            })
            .times(1)
            .returning(|_, _| Box::pin(async { Ok(()) }));

        let uc = make_full_uc(
            conv_repo,
            msg_repo,
            image_prompt_llm(),
            image_gen,
            MockAttachmentStorePort::new(),
        );
        let (tx, _rx) = mpsc::channel(8);

        // Instrukce bez obrázkových klíčových slov — obrázková větev se
        // přesto musí vynutit (init obrázek rozhoduje, ne klasifikace).
        uc.edit_image(
            ConversationId::new(),
            "změň pozadí na západ slunce".into(),
            "/data/weave/gallery/original.png".into(),
            tx,
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn execute_persists_assistant_response_after_stream() {
        let mut conv_repo = MockConversationRepository::new();
        conv_repo
            .expect_find_by_id()
            .returning(|_| Box::pin(async { Ok(Some(dummy_conversation())) }));

        let mut msg_repo = MockMessageRepository::new();
        msg_repo
            .expect_save()
            .withf(|m: &Message| m.role == weave_domain::message::Role::User)
            .times(1)
            .returning(|_| Box::pin(async { Ok(()) }));
        msg_repo
            .expect_save()
            .withf(|m: &Message| {
                m.role == weave_domain::message::Role::Assistant
                    && m.content == "Ahoj světe"
                    && m.stats.is_some()
            })
            .times(1)
            .returning(|_| Box::pin(async { Ok(()) }));
        msg_repo
            .expect_list_by_conversation()
            .returning(|_| Box::pin(async { Ok(vec![]) }));

        let mut llm = MockLlmPort::new();
        llm.expect_chat_stream().returning(|_, tx| {
            Box::pin(async move {
                let _ = tx.send(StreamChunk::Token("Ahoj".into())).await;
                let _ = tx.send(StreamChunk::Token(" světe".into())).await;
                let _ = tx.send(StreamChunk::Done(Default::default())).await;
                Ok(())
            })
        });

        let uc = make_full_uc(
            conv_repo,
            msg_repo,
            llm,
            MockImageGenPort::new(),
            MockAttachmentStorePort::new(),
        );
        let (tx, _rx) = mpsc::channel(16);

        uc.execute(
            ConversationId::new(),
            "jak se máš?".into(),
            vec![],
            vec![],
            tx,
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn regenerate_deletes_trailing_reply_and_streams_new_one() {
        let mut conv_repo = MockConversationRepository::new();
        conv_repo
            .expect_find_by_id()
            .returning(|_| Box::pin(async { Ok(Some(dummy_conversation())) }));

        let mut msg_repo = MockMessageRepository::new();
        msg_repo
            .expect_delete_trailing_assistant_messages()
            .times(1)
            .returning(|_| Box::pin(async { Ok(()) }));
        msg_repo.expect_list_by_conversation().returning(|_| {
            Box::pin(async { Ok(vec![Message::user(ConversationId::new(), "jak se máš?")]) })
        });
        // Jediná povolená save() je nová odpověď asistenta — uložení další
        // zprávy uživatele by test shodilo (regenerace ji přidávat nesmí).
        msg_repo
            .expect_save()
            .withf(|m: &Message| {
                m.role == weave_domain::message::Role::Assistant && m.content == "Nová odpověď"
            })
            .times(1)
            .returning(|_| Box::pin(async { Ok(()) }));

        let mut llm = MockLlmPort::new();
        llm.expect_chat_stream().returning(|_, tx| {
            Box::pin(async move {
                let _ = tx.send(StreamChunk::Token("Nová odpověď".into())).await;
                let _ = tx.send(StreamChunk::Done(Default::default())).await;
                Ok(())
            })
        });

        let uc = make_full_uc(
            conv_repo,
            msg_repo,
            llm,
            MockImageGenPort::new(),
            MockAttachmentStorePort::new(),
        );
        let (tx, _rx) = mpsc::channel(16);

        uc.regenerate(ConversationId::new(), tx).await.unwrap();
    }

    #[tokio::test]
    async fn regenerate_fails_without_any_user_message() {
        let mut conv_repo = MockConversationRepository::new();
        conv_repo
            .expect_find_by_id()
            .returning(|_| Box::pin(async { Ok(Some(dummy_conversation())) }));

        let mut msg_repo = MockMessageRepository::new();
        msg_repo
            .expect_delete_trailing_assistant_messages()
            .returning(|_| Box::pin(async { Ok(()) }));
        msg_repo
            .expect_list_by_conversation()
            .returning(|_| Box::pin(async { Ok(vec![]) }));

        let uc = make_full_uc(
            conv_repo,
            msg_repo,
            MockLlmPort::new(),
            MockImageGenPort::new(),
            MockAttachmentStorePort::new(),
        );
        let (tx, _rx) = mpsc::channel(4);

        assert!(uc.regenerate(ConversationId::new(), tx).await.is_err());
    }

    #[tokio::test]
    async fn execute_applies_per_conversation_generation_settings() {
        use weave_domain::generation_settings::GenerationSettings;

        let mut conv_repo = MockConversationRepository::new();
        conv_repo
            .expect_find_by_id()
            .returning(|_| Box::pin(async { Ok(Some(dummy_conversation())) }));

        let mut msg_repo = MockMessageRepository::new();
        msg_repo
            .expect_save()
            .returning(|_| Box::pin(async { Ok(()) }));
        msg_repo
            .expect_list_by_conversation()
            .returning(|_| Box::pin(async { Ok(vec![]) }));

        let mut gen_repo = MockGenerationSettingsRepository::new();
        gen_repo.expect_get().returning(|_| {
            Box::pin(async {
                Ok(GenerationSettings {
                    context_length: Some(16384),
                    temperature: Some(1.4),
                    max_tokens: Some(512),
                })
            })
        });

        let mut llm = MockLlmPort::new();
        llm.expect_chat_stream()
            .withf(|req: &ChatRequest, _tx| {
                req.temperature == 1.4
                    && req.max_tokens == Some(512)
                    && req.context_length == Some(16384)
            })
            .times(1)
            .returning(|_, _| Box::pin(async { Ok(()) }));

        let uc = SendMessageUseCase::new(
            Arc::new(conv_repo),
            Arc::new(msg_repo),
            Arc::new(llm),
            Arc::new(MockImageGenPort::new()),
            Arc::new(MockWorkspaceRepository::new()),
            Arc::new(MockPersonaRepository::new()),
            Arc::new(MockAttachmentStorePort::new()),
            Arc::new(gen_repo),
            Arc::new(default_comfy_installer()),
            Arc::new(default_lora_catalog()),
        );
        let (tx, _rx) = mpsc::channel(8);

        uc.execute(
            ConversationId::new(),
            "jak se máš?".into(),
            vec![],
            vec![],
            tx,
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn image_generation_orchestrates_environment_and_frees_vram() {
        let mut conv_repo = MockConversationRepository::new();
        conv_repo
            .expect_find_by_id()
            .returning(|_| Box::pin(async { Ok(Some(dummy_conversation())) }));

        let mut msg_repo = MockMessageRepository::new();
        msg_repo
            .expect_save()
            .returning(|_| Box::pin(async { Ok(()) }));

        // Nainstalovano, ale server nebezi -> orchestrace ho musi spustit
        // a po dokonceni zase zastavit (uvolneni VRAM).
        let mut installer = MockComfyInstallerPort::new();
        installer
            .expect_status()
            .returning(|| Box::pin(async { Ok(ComfyStatus::Installed) }));
        installer
            .expect_ensure_style_checkpoint()
            .times(1)
            .returning(|_, _| Box::pin(async { Ok(()) }));
        // Bez referenčního obrázku se PuLID assety nesmí řešit
        installer.expect_ensure_reference_assets().never();
        installer
            .expect_start_server()
            .times(1)
            .returning(|| Box::pin(async { Ok(()) }));
        installer
            .expect_stop_server()
            .times(1)
            .returning(|| Box::pin(async { Ok(()) }));

        let mut image_gen = MockImageGenPort::new();
        image_gen.expect_generate().returning(|req, tx| {
            // Prompt musí projít LLM vylepšením (anglický SD prompt),
            // ne originál „nakresli hrad na skale".
            assert_eq!(req.prompt, "young woman on a beach, detailed");
            assert!(req.negative_prompt.is_some());
            Box::pin(async move {
                let _ = tx
                    .send(ImageProgress::Done {
                        output_path: "/gallery/obrazek.png".into(),
                    })
                    .await;
                Ok(())
            })
        });

        let uc = SendMessageUseCase::new(
            Arc::new(conv_repo),
            Arc::new(msg_repo),
            Arc::new(image_prompt_llm()),
            Arc::new(image_gen),
            Arc::new(MockWorkspaceRepository::new()),
            Arc::new(MockPersonaRepository::new()),
            Arc::new(MockAttachmentStorePort::new()),
            Arc::new(default_gen_settings()),
            Arc::new(installer),
            Arc::new(default_lora_catalog()),
        );

        let (tx, mut rx) = mpsc::channel(64);
        uc.execute(
            ConversationId::new(),
            "nakresli hrad na skale".into(),
            vec![],
            vec![],
            tx,
        )
        .await
        .unwrap();

        // Posbirej stream a over faze + vysledny obrazek
        let mut stages = Vec::new();
        let mut got_image_token = false;
        let mut got_done = false;
        while let Some(chunk) = rx.recv().await {
            match chunk {
                StreamChunk::ImageStage(info) => stages.push(info.stage),
                StreamChunk::Token(t) => got_image_token = t.contains("/gallery/obrazek.png"),
                StreamChunk::Done(_) => got_done = true,
                StreamChunk::Error(e) => panic!("necekana chyba: {e}"),
            }
        }

        assert!(stages.contains(&ImageStage::Checking));
        assert!(stages.contains(&ImageStage::StartingServer));
        assert!(stages.contains(&ImageStage::Generating));
        assert!(stages.contains(&ImageStage::Finishing));
        assert!(got_image_token, "chybi markdown s cestou k obrazku");
        assert!(got_done);
    }

    /// Regresní test na "pulid_file not in []" — generování s referenčním
    /// obrázkem musí před spuštěním workflow zajistit PuLID assety (váhy,
    /// InsightFace), protože instalace z dřívějších verzí appky je nemá.
    #[tokio::test]
    async fn reference_image_ensures_pulid_assets() {
        let mut conv_repo = MockConversationRepository::new();
        conv_repo
            .expect_find_by_id()
            .returning(|_| Box::pin(async { Ok(Some(dummy_conversation())) }));

        let mut msg_repo = MockMessageRepository::new();
        msg_repo
            .expect_save()
            .returning(|_| Box::pin(async { Ok(()) }));

        let mut attachment_store = MockAttachmentStorePort::new();
        attachment_store
            .expect_store_reference_image()
            .returning(|_| {
                Box::pin(async {
                    Ok(StoredImage {
                        path: "/appdata/reference/ref.png".into(),
                        mime: "image/png".into(),
                    })
                })
            });

        let mut installer = MockComfyInstallerPort::new();
        installer
            .expect_status()
            .returning(|| Box::pin(async { Ok(ComfyStatus::Running) }));
        installer
            .expect_ensure_style_checkpoint()
            .times(1)
            .returning(|_, _| Box::pin(async { Ok(()) }));
        installer
            .expect_ensure_reference_assets()
            .times(1)
            .returning(|_| Box::pin(async { Ok(()) }));
        installer
            .expect_stop_server()
            .returning(|| Box::pin(async { Ok(()) }));

        let mut image_gen = MockImageGenPort::new();
        image_gen.expect_generate().returning(|req, tx| {
            assert_eq!(req.reference_image_paths, ["/appdata/reference/ref.png"]);
            Box::pin(async move {
                let _ = tx
                    .send(ImageProgress::Done {
                        output_path: "/gallery/portret.png".into(),
                    })
                    .await;
                Ok(())
            })
        });

        let uc = SendMessageUseCase::new(
            Arc::new(conv_repo),
            Arc::new(msg_repo),
            Arc::new(image_prompt_llm()),
            Arc::new(image_gen),
            Arc::new(MockWorkspaceRepository::new()),
            Arc::new(MockPersonaRepository::new()),
            Arc::new(attachment_store),
            Arc::new(default_gen_settings()),
            Arc::new(installer),
            Arc::new(default_lora_catalog()),
        );

        let (tx, mut rx) = mpsc::channel(64);
        uc.execute(
            ConversationId::new(),
            "nakresli mě jako rytíře".into(),
            vec![],
            vec!["C:/fotky/ja.png".into()],
            tx,
        )
        .await
        .unwrap();

        while let Some(chunk) = rx.recv().await {
            if let StreamChunk::Error(e) = chunk {
                panic!("necekana chyba: {e}");
            }
        }
    }

    /// „Poslat znovu" musí nejdřív smazat vše po dané zprávě a pak
    /// regenerovat odpověď na ni (jako poslední zprávu v historii).
    #[tokio::test]
    async fn resend_truncates_after_message_then_regenerates() {
        let conv = ConversationId::new();
        let target = Message::user(conv.clone(), "původní dotaz");
        let target_id = target.id.clone();
        let target_for_list = target.clone();

        let mut conv_repo = MockConversationRepository::new();
        conv_repo
            .expect_find_by_id()
            .returning(|_| Box::pin(async { Ok(Some(dummy_conversation())) }));

        let mut msg_repo = MockMessageRepository::new();
        let expected_id = target_id.clone();
        msg_repo
            .expect_delete_messages_after()
            .times(1)
            .withf(move |_, mid| *mid == expected_id)
            .returning(|_, _| Box::pin(async { Ok(()) }));
        msg_repo
            .expect_delete_trailing_assistant_messages()
            .returning(|_| Box::pin(async { Ok(()) }));
        msg_repo.expect_list_by_conversation().returning(move |_| {
            let history = vec![target_for_list.clone()];
            Box::pin(async move { Ok(history) })
        });
        msg_repo
            .expect_save()
            .returning(|_| Box::pin(async { Ok(()) }));

        let mut llm = MockLlmPort::new();
        llm.expect_chat_stream().returning(|_, tx| {
            Box::pin(async move {
                let _ = tx.send(StreamChunk::Token("čerstvá odpověď".into())).await;
                let _ = tx.send(StreamChunk::Done(Default::default())).await;
                Ok(())
            })
        });

        let uc = SendMessageUseCase::new(
            Arc::new(conv_repo),
            Arc::new(msg_repo),
            Arc::new(llm),
            Arc::new(MockImageGenPort::new()),
            Arc::new(MockWorkspaceRepository::new()),
            Arc::new(MockPersonaRepository::new()),
            Arc::new(MockAttachmentStorePort::new()),
            Arc::new(default_gen_settings()),
            Arc::new(default_comfy_installer()),
            Arc::new(default_lora_catalog()),
        );

        let (tx, mut rx) = mpsc::channel(64);
        uc.resend(conv, target_id, tx).await.unwrap();

        let mut got_token = false;
        while let Some(chunk) = rx.recv().await {
            if let StreamChunk::Token(t) = chunk {
                got_token = t.contains("čerstvá odpověď");
            }
        }
        assert!(got_token);
    }

    #[test]
    fn parse_enhancement_extracts_prompt_and_lora() {
        let (prompt, lora) = parse_prompt_enhancement(
            "young woman on a beach, detailed\nLORA: nikol woman",
            "fallback",
        );
        assert_eq!(prompt, "young woman on a beach, detailed");
        assert_eq!(lora.as_deref(), Some("nikol woman"));

        // none / chybějící řádek / prázdný výstup
        let (p2, l2) = parse_prompt_enhancement("castle on a rock\nLORA: none", "fb");
        assert_eq!(p2, "castle on a rock");
        assert!(l2.is_none());

        let (p3, l3) = parse_prompt_enhancement("just a prompt", "fb");
        assert_eq!(p3, "just a prompt");
        assert!(l3.is_none());

        let (p4, _) = parse_prompt_enhancement("  \n LORA: x", "fallback");
        assert_eq!(p4, "fallback", "prázdný prompt padá na fallback");

        // case-insensitive prefix
        let (_, l5) = parse_prompt_enhancement("p\nlora: Sailor Moon style", "fb");
        assert_eq!(l5.as_deref(), Some("Sailor Moon style"));
    }

    /// Celá LoRA cesta: LLM navrhne koncept → katalog najde LoRA →
    /// stáhne se → soubor jde do requestu a trigger words do promptu.
    #[tokio::test]
    async fn lora_is_found_downloaded_and_wired_into_request() {
        use crate::ports::lora_catalog_port::LoraInfo;

        let mut conv_repo = MockConversationRepository::new();
        conv_repo
            .expect_find_by_id()
            .returning(|_| Box::pin(async { Ok(Some(dummy_conversation())) }));
        let mut msg_repo = MockMessageRepository::new();
        msg_repo
            .expect_save()
            .returning(|_| Box::pin(async { Ok(()) }));

        let mut llm = MockLlmPort::new();
        llm.expect_chat_stream().returning(|_, tx| {
            Box::pin(async move {
                let _ = tx
                    .send(StreamChunk::Token(
                        "young woman portrait\nLORA: nikol".into(),
                    ))
                    .await;
                let _ = tx.send(StreamChunk::Done(Default::default())).await;
                Ok(())
            })
        });

        let mut catalog = MockLoraCatalogPort::new();
        catalog
            .expect_find_lora()
            .times(1)
            .withf(|query, base| query == "nikol" && base == "SDXL 1.0")
            .returning(|_, _| {
                Box::pin(async {
                    Ok(Some(LoraInfo {
                        name: "Nikol Style".into(),
                        file_name: "nikol_v1.safetensors".into(),
                        download_url: "https://cdn/nikol.safetensors".into(),
                        trigger_words: vec!["nikol woman".into()],
                    }))
                })
            });

        let mut installer = MockComfyInstallerPort::new();
        installer
            .expect_status()
            .returning(|| Box::pin(async { Ok(ComfyStatus::Running) }));
        installer
            .expect_ensure_style_checkpoint()
            .returning(|_, _| Box::pin(async { Ok(()) }));
        installer
            .expect_ensure_lora()
            .times(1)
            .withf(|file, url, _| file == "nikol_v1.safetensors" && url.contains("cdn"))
            .returning(|_, _, _| Box::pin(async { Ok(()) }));
        installer
            .expect_stop_server()
            .returning(|| Box::pin(async { Ok(()) }));

        let mut image_gen = MockImageGenPort::new();
        image_gen.expect_generate().returning(|req, tx| {
            assert_eq!(req.lora_file.as_deref(), Some("nikol_v1.safetensors"));
            assert!(req.prompt.contains("nikol woman"), "chybí trigger words");
            Box::pin(async move {
                let _ = tx
                    .send(ImageProgress::Done {
                        output_path: "/gallery/lora.png".into(),
                    })
                    .await;
                Ok(())
            })
        });

        let uc = SendMessageUseCase::new(
            Arc::new(conv_repo),
            Arc::new(msg_repo),
            Arc::new(llm),
            Arc::new(image_gen),
            Arc::new(MockWorkspaceRepository::new()),
            Arc::new(MockPersonaRepository::new()),
            Arc::new(MockAttachmentStorePort::new()),
            Arc::new(default_gen_settings()),
            Arc::new(installer),
            Arc::new(catalog),
        );

        let (tx, mut rx) = mpsc::channel(64);
        uc.execute(
            ConversationId::new(),
            "nakresli Nikol jako portrét".into(),
            vec![],
            vec![],
            tx,
        )
        .await
        .unwrap();

        while let Some(chunk) = rx.recv().await {
            if let StreamChunk::Error(e) = chunk {
                panic!("necekana chyba: {e}");
            }
        }
    }

    /// Když LLM vylepšení promptu selže, generování běží dál s původním
    /// textem — horší prompt je lepší než spadlá pipeline.
    #[tokio::test]
    async fn image_prompt_falls_back_to_original_on_llm_error() {
        let mut conv_repo = MockConversationRepository::new();
        conv_repo
            .expect_find_by_id()
            .returning(|_| Box::pin(async { Ok(Some(dummy_conversation())) }));

        let mut msg_repo = MockMessageRepository::new();
        msg_repo
            .expect_save()
            .returning(|_| Box::pin(async { Ok(()) }));

        let mut llm = MockLlmPort::new();
        llm.expect_chat_stream().returning(|_, tx| {
            Box::pin(async move {
                let _ = tx.send(StreamChunk::Error("LLM nedostupné".into())).await;
                Ok(())
            })
        });

        let mut image_gen = MockImageGenPort::new();
        image_gen.expect_generate().returning(|req, tx| {
            assert_eq!(req.prompt, "nakresli hrad na skale");
            Box::pin(async move {
                let _ = tx
                    .send(ImageProgress::Done {
                        output_path: "/gallery/hrad.png".into(),
                    })
                    .await;
                Ok(())
            })
        });

        let uc = SendMessageUseCase::new(
            Arc::new(conv_repo),
            Arc::new(msg_repo),
            Arc::new(llm),
            Arc::new(image_gen),
            Arc::new(MockWorkspaceRepository::new()),
            Arc::new(MockPersonaRepository::new()),
            Arc::new(MockAttachmentStorePort::new()),
            Arc::new(default_gen_settings()),
            Arc::new(default_comfy_installer()),
            Arc::new(default_lora_catalog()),
        );

        let (tx, mut rx) = mpsc::channel(64);
        uc.execute(
            ConversationId::new(),
            "nakresli hrad na skale".into(),
            vec![],
            vec![],
            tx,
        )
        .await
        .unwrap();

        let mut got_done = false;
        while let Some(chunk) = rx.recv().await {
            if matches!(chunk, StreamChunk::Done(_)) {
                got_done = true;
            }
        }
        assert!(got_done, "generovani nedobehlo");
    }
}
