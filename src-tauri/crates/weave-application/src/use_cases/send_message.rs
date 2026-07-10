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
        subject_repository::SubjectRepository,
        workspace_port::WorkspaceRepository,
    },
};

/// Instrukce pro převod požadavku na anglický Stable Diffusion prompt
/// + návrh konceptu pro vyhledání LoRA (druhý řádek).
const IMAGE_PROMPT_SYSTEM: &str = "You convert user requests into English Stable Diffusion \
    prompts. Reply with EXACTLY two lines:\n\
    Line 1: comma-separated English descriptors (subject, appearance, setting, lighting, \
    style, quality tags like 'highly detailed, sharp focus'). Translate non-English requests. \
    Czech and Slovak input is common: translate it completely into natural English image tags, \
    never leave Czech/Slovak command words like 'nakresli', 'vygeneruj', 'obrazek' or 'fotka'. \
    Keep every detail the user asked for, including names used for people and fictional \
    characters. For recognizable fictional characters, include their canonical visual identifiers \
    and set Line 2 to the exact character name. If a character has child/teen and adult versions \
    and the prompt asks for revealing clothing, make the character explicitly adult. If reference \
    images are attached, describe the requested result and include 'same person as reference \
    image, consistent facial identity' when the prompt concerns a person. \
    Always include an explicit shot framing tag: use 'full body shot, full length portrait, \
    head to toe, standing' whenever the whole figure, outfit or pose matters (a person in \
    described clothing, a character), otherwise a fitting one like 'portrait' or 'close-up'.\n\
    Line 2: 'LORA: <2-4 English words naming a specific character, celebrity, art style or \
    concept a LoRA model could exist for>' or 'LORA: none' when the request is generic.\n\
    Output ONLY these two lines. Never repeat the conversation or add role markers.";

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
const MAX_IMAGE_GENERATION_REFERENCES: usize = 4;

/// Klíčová slova (cz+en) značící požadavek na celou postavu. Když je prompt
/// obsahuje, generuje se na výšku (SDXL jinak do čtverce postavu ořízne
/// u stehen — to byl hlavní rozdíl oproti výsledkům à la ChatGPT).
const FULL_BODY_KEYWORDS: &[&str] = &[
    "full body",
    "full length",
    "head to toe",
    "whole body",
    "entire body",
    "celá postava",
    "cela postava",
    "celé tělo",
    "cele telo",
    "od hlavy",
    "na výšku",
];

const WIDE_IMAGE_KEYWORDS: &[&str] = &[
    "16:9",
    "16x9",
    "wide",
    "widescreen",
    "ultrawide",
    "landscape",
    "horizontal",
    "wallpaper",
    "desktop wallpaper",
    "qhd",
    "4k",
    "uhd",
    "tapeta",
    "sirokouhly",
    "sirokouhla",
    "na sirku",
];

/// Rozpozná záběr celé postavy (cz+en) v promptu.
fn wants_full_body(prompt: &str) -> bool {
    let lower = strip_czech_diacritics(prompt).to_lowercase();
    FULL_BODY_KEYWORDS.iter().any(|k| lower.contains(k))
}

fn wants_wide_image(prompt: &str) -> bool {
    let lower = strip_czech_diacritics(prompt).to_lowercase();
    WIDE_IMAGE_KEYWORDS.iter().any(|k| lower.contains(k))
}

/// Vypadá text jako hotový anglický SD prompt? (převážně ASCII, komma-
/// separovaný výčet deskriptorů, dost dlouhý). Takový prompt pošleme do
/// generátoru rovnou, bez LLM překladu — mj. aby ho cenzurovaný model
/// neodmítl. Český požadavek („vygeneruj mi…") tímhle neprojde a přeloží se.
fn is_ready_english_prompt(prompt: &str) -> bool {
    let trimmed = prompt.trim();
    let total = trimmed.chars().count();
    if total < 30 {
        return false;
    }
    let non_ascii = trimmed.chars().filter(|c| !c.is_ascii()).count();
    let commas = trimmed.matches(',').count();
    (non_ascii as f32 / total as f32) < 0.05 && commas >= 2
}

fn strip_czech_diacritics(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'á' | 'Á' => 'a',
            'č' | 'Č' => 'c',
            'ď' | 'Ď' => 'd',
            'é' | 'É' | 'ě' | 'Ě' => 'e',
            'í' | 'Í' => 'i',
            'ň' | 'Ň' => 'n',
            'ó' | 'Ó' => 'o',
            'ř' | 'Ř' => 'r',
            'š' | 'Š' => 's',
            'ť' | 'Ť' => 't',
            'ú' | 'Ú' | 'ů' | 'Ů' => 'u',
            'ý' | 'Ý' => 'y',
            'ž' | 'Ž' => 'z',
            other => other,
        })
        .collect()
}

fn fallback_image_prompt(user_prompt: &str, has_reference_images: bool) -> String {
    let normalized = strip_czech_diacritics(user_prompt).to_lowercase();
    let mut tags: Vec<String> = Vec::new();
    fn push_tag(tags: &mut Vec<String>, tag: &str) {
        if !tags.iter().any(|existing| existing == tag) {
            tags.push(tag.to_string());
        }
    }

    let phrase_tags = [
        ("cela postava", "full body shot"),
        ("cele telo", "full body shot"),
        ("od hlavy", "head to toe"),
        ("mlada zena", "young adult woman"),
        ("mlady muz", "young adult man"),
        ("dlouhe vlasy", "long hair"),
        ("cerne vlasy", "black hair"),
        ("blond vlasy", "blonde hair"),
        ("modre oci", "blue eyes"),
        ("zelene oci", "green eyes"),
        ("cervene saty", "red dress"),
        ("zapad slunce", "sunset"),
        ("ahsoka tano", "adult Ahsoka Tano-inspired character"),
        ("star wars", "Star Wars inspired"),
        ("kocka v klobouku", "cat wearing a hat"),
        ("kocky v klobouku", "cat wearing a hat"),
        ("kocku v klobouku", "cat wearing a hat"),
    ];
    for (needle, tag) in phrase_tags {
        if normalized.contains(needle) {
            push_tag(&mut tags, tag);
        }
    }

    let word_tags = [
        ("portret", "portrait"),
        ("zena", "woman"),
        ("divka", "young adult woman"),
        ("muz", "man"),
        ("hrad", "castle"),
        ("skala", "rock cliff"),
        ("skale", "rock cliff"),
        ("les", "forest"),
        ("plaz", "beach"),
        ("more", "sea"),
        ("bikiny", "wearing a bikini"),
        ("bikinach", "wearing a bikini"),
        ("plavky", "wearing swimwear"),
        ("sama", "solo"),
        ("samotna", "solo"),
        ("alone", "solo"),
        ("solo", "solo"),
        ("animated", "stylized animated illustration"),
        ("cartoon", "stylized animated illustration"),
        ("animovany", "stylized animated illustration"),
        ("stylizovany", "stylized animated illustration"),
        ("mesto", "city"),
        ("ulice", "street"),
        ("drak", "dragon"),
        ("kocka", "cat"),
        ("pes", "dog"),
        ("kun", "horse"),
        ("auto", "car"),
        ("dum", "house"),
        ("pokoj", "room"),
        ("realisticky", "realistic"),
        ("fotorealisticky", "photorealistic"),
        ("malba", "painting"),
        ("ilustrace", "illustration"),
        ("anime", "anime"),
        ("detailni", "highly detailed"),
        ("usmev", "smile"),
        ("rytir", "knight"),
        ("rytire", "knight"),
        ("kouzelnik", "wizard"),
        ("priroda", "nature"),
        ("noc", "night"),
        ("noci", "night"),
        ("dest", "rain"),
        ("snih", "snow"),
    ];
    for (needle, tag) in word_tags {
        if normalized.contains(needle) {
            push_tag(&mut tags, tag);
        }
    }

    let stem_tags = [
        ("kock", "cat"),
        ("kot", "kitten"),
        ("klobouk", "hat"),
        ("cep", "hat"),
        ("barevn", "colorful"),
        ("roztomil", "cute"),
        ("sedic", "sitting"),
        ("sed", "sitting"),
        ("lezic", "lying down"),
        ("spic", "sleeping"),
        ("ahsoka", "adult Ahsoka Tano-inspired character"),
    ];
    for token in normalized
        .split(|c: char| !c.is_alphanumeric())
        .filter(|token| !token.is_empty())
    {
        for (stem, tag) in stem_tags {
            if token.starts_with(stem) {
                push_tag(&mut tags, tag);
            }
        }
    }

    if has_reference_images {
        push_tag(&mut tags, "same person as reference image");
        push_tag(&mut tags, "consistent facial identity");
    }
    push_tag(&mut tags, "high quality");
    push_tag(&mut tags, "sharp focus");
    push_tag(&mut tags, "highly detailed");

    if tags.len() <= 3 {
        tags.insert(0, "image based on user request".to_string());
        let cleaned = normalized
            .split_whitespace()
            .filter(|w| {
                !matches!(
                    *w,
                    "nakresli" | "vygeneruj" | "udelaj" | "mi" | "prosim" | "obrazek" | "fotku"
                )
            })
            .collect::<Vec<_>>()
            .join(" ");
        if !cleaned.is_empty() {
            tags.insert(1, cleaned);
        }
    }

    tags.join(", ")
}

fn mentions_ahsoka(prompt: &str) -> bool {
    strip_czech_diacritics(prompt)
        .to_lowercase()
        .contains("ahsoka")
}

fn detect_lora_query(user_prompt: &str, enhanced_prompt: &str) -> Option<String> {
    let combined = format!("{user_prompt}\n{enhanced_prompt}");
    let normalized = strip_czech_diacritics(&combined).to_lowercase();

    let known_queries = [
        ("ahsoka tano", "Ahsoka Tano"),
        ("ahsoka", "Ahsoka Tano"),
        ("sailor moon", "Sailor Moon"),
        ("lara croft", "Lara Croft"),
        ("harley quinn", "Harley Quinn"),
        ("darth vader", "Darth Vader"),
    ];
    known_queries
        .iter()
        .find_map(|(needle, query)| normalized.contains(needle).then(|| (*query).to_string()))
}

fn reinforce_known_subject_tags(sd_prompt: &str, user_prompt: &str) -> String {
    if !mentions_ahsoka(user_prompt) {
        return sd_prompt.to_string();
    }

    let normalized_sd = strip_czech_diacritics(sd_prompt).to_lowercase();
    let mut prefix = Vec::new();
    if !normalized_sd.contains("ahsoka") {
        prefix.push("adult Ahsoka Tano-inspired character");
    } else if !normalized_sd.contains("adult") {
        prefix.push("adult version");
    }
    if !normalized_sd.contains("montral") && !normalized_sd.contains("lekku") {
        prefix.push("orange skin");
        prefix.push("white facial markings");
        prefix.push("blue and white montrals and lekku");
    }
    if !normalized_sd.contains("star wars") {
        prefix.push("Star Wars inspired");
    }

    if prefix.is_empty() {
        sd_prompt.to_string()
    } else {
        format!("{}, {}", prefix.join(", "), sd_prompt)
    }
}

/// Ořízne text u prvního ChatML řídicího tokenu (`<|…`) a osekne uvozovky.
/// Malé lokální modely (Qwen apod.) za odpovědí občas „ukecají" celou
/// šablonu konverzace — vše od `<|` je smetí, ne prompt.
fn strip_chat_tokens(s: &str) -> &str {
    s.split("<|").next().unwrap_or(s).trim().trim_matches('"')
}

/// Rozparsuje výstup LLM vylepšení promptu: řádky bez prefixu LORA: tvoří
/// prompt, řádek `LORA: <koncept>` je dotaz pro katalog LoRA (`none`/prázdno
/// = žádný). Řídicí ChatML tokeny se odstraní. Prázdný prompt → fallback.
fn parse_prompt_enhancement(raw: &str, fallback: &str) -> (String, Option<String>) {
    // Prompt bereme jen z části PŘED prvním ChatML tokenem — tím odpadne
    // zopakovaná konverzace, kterou malé modely někdy přilepí.
    let head = raw.split("<|").next().unwrap_or(raw);
    let prompt = head
        .lines()
        .map(|l| l.trim().trim_matches('"'))
        .filter(|l| !(l.len() >= 5 && l[..5].eq_ignore_ascii_case("lora:")))
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ");

    // LoRA řádek může přijít i za promptem, tak ho hledáme v celém výstupu;
    // jeho hodnotu ale taky očistíme od případných tokenů.
    let mut lora = None;
    for line in raw.lines() {
        let trimmed = line.trim().trim_matches('"');
        if trimmed.len() >= 5 && trimmed[..5].eq_ignore_ascii_case("lora:") {
            let value = strip_chat_tokens(&trimmed[5..]);
            if !value.is_empty() && !value.eq_ignore_ascii_case("none") {
                lora = Some(value.to_string());
            }
            break;
        }
    }

    if prompt.is_empty() {
        (fallback.to_string(), lora)
    } else {
        (prompt, lora)
    }
}

/// Je jméno postavy zmíněné v (už normalizovaném) textu? Porovnává se bez
/// diakritiky a velikosti písmen. Česká ženská jména na -a se skloňují
/// (Růženka → Růžence/Růženku…), proto se u nich zkouší i kmen bez koncovky
/// a bez posledních dvou znaků (palatalizace k→c v dativu). Jména do 3 znaků
/// se ignorují, aby se neshodovala běžná slova.
fn subject_name_mentioned(name: &str, normalized_haystack: &str) -> bool {
    let name = strip_czech_diacritics(name.trim()).to_lowercase();
    if name.chars().count() < 3 {
        return false;
    }
    if normalized_haystack.contains(&name) {
        return true;
    }
    if name.ends_with('a') {
        for cut in [1usize, 2] {
            let stem: String = {
                let chars: Vec<char> = name.chars().collect();
                chars[..chars.len().saturating_sub(cut)].iter().collect()
            };
            if stem.chars().count() >= 4 && normalized_haystack.contains(&stem) {
                return true;
            }
        }
    }
    false
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
    subject_repo: Arc<dyn SubjectRepository>,
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
        subject_repo: Arc<dyn SubjectRepository>,
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
            subject_repo,
        }
    }

    /// Postavy zmíněné jménem v textu (bez ohledu na velikost písmen
    /// a diakritiku, včetně běžného skloňování ženských jmen na -a).
    async fn mentioned_subjects(&self, text: &str) -> Vec<weave_domain::subject::Subject> {
        let subjects = match self.subject_repo.list().await {
            Ok(subjects) => subjects,
            Err(e) => {
                tracing::warn!("Načtení referenčních postav selhalo: {e}");
                return vec![];
            }
        };
        let haystack = strip_czech_diacritics(text).to_lowercase();
        subjects
            .into_iter()
            .filter(|s| subject_name_mentioned(&s.name, &haystack))
            .collect()
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn execute(
        &self,
        conversation_id: ConversationId,
        content: String,
        file_refs: Vec<String>,
        reference_images: Vec<String>,
        reference_preservation: Option<String>,
        translate_image_prompt: bool,
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
            reference_preservation,
            translate_image_prompt,
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
            true,
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
            None,
            true,
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
        reference_preservation: Option<String>,
        translate_image_prompt: bool,
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

        // Per-konverzační parametry (posuvníky v chatu) — použijí se pro text
        // (kontext/teplota/tokeny) i pro obrázky (síla PuLID, FaceDetailer).
        let gen = self.gen_settings_repo.get(&conversation_id).await?;

        // Úprava obrázku (init_image) je vždy generování obrázku, bez ohledu
        // na to, jak by heuristika klasifikovala samotný text instrukce.
        let is_image =
            init_image.is_some() || matches!(intent, weave_domain::model::Intent::ImageGeneration);
        let result = match is_image {
            true => {
                // Zmíněná referenční postava bez přiložených fotek → její fotky
                // se přiloží automaticky (PuLID). Jen při PRÁVĚ jedné shodě —
                // průměrovat identity dvou různých lidí nedává smysl.
                let mut reference_image_paths = reference_image_paths;
                if reference_image_paths.is_empty() && init_image.is_none() {
                    let mentioned = self.mentioned_subjects(&content).await;
                    if let [subject] = mentioned.as_slice() {
                        if !subject.images.is_empty() {
                            tracing::info!(subject = %subject.name,
                                "Postava zmíněna v promptu — přikládám její fotky jako referenci");
                            reference_image_paths =
                                subject.images.iter().map(|i| i.path.clone()).collect();
                        }
                    }
                }
                self.handle_image(
                    content,
                    reference_image_paths,
                    reference_preservation,
                    translate_image_prompt,
                    init_image,
                    gen.pulid_weight_or_default(),
                    gen.face_detailer_enabled(),
                    gen.image_checkpoint().map(str::to_string),
                    gen.image_lora().map(str::to_string),
                    tee_tx,
                )
                .await
            }
            false => {
                let mut history = self.msg_repo.list_by_conversation(&conversation_id).await?;

                // Přiložené @soubory → system kontext na začátku (neukládá se do historie)
                if let Some(context) = self.build_file_context(&file_refs).await? {
                    history.insert(0, Message::system(conversation_id.clone(), context));
                }

                // Zmíněné referenční postavy → jejich poznámky (vzhled, věk…)
                // jako system kontext, aby spisovatel držel charakter konzistentní.
                if let Some(context) = self.build_subject_context(&content).await {
                    history.insert(0, Message::system(conversation_id.clone(), context));
                }

                // Persona konverzace → system prompt úplně na začátek
                if let Some(prompt) = self
                    .resolve_persona_prompt(&conversation.persona_id)
                    .await?
                {
                    history.insert(0, Message::system(conversation_id.clone(), prompt));
                }

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

    /// System kontext s popisy postav zmíněných v aktuální zprávě (jméno
    /// a poznámky uživatele: vzhled, věk, povaha…). `None` když žádná
    /// zmíněná postava nemá neprázdné poznámky.
    async fn build_subject_context(&self, content: &str) -> Option<String> {
        let mentioned = self.mentioned_subjects(content).await;
        let described: Vec<String> = mentioned
            .iter()
            .filter(|s| !s.notes.trim().is_empty())
            .map(|s| format!("- {}: {}", s.name.trim(), s.notes.trim()))
            .collect();
        if described.is_empty() {
            return None;
        }
        Some(format!(
            "Popisy postav od uživatele (drž je konzistentně v celém textu):\n{}",
            described.join("\n")
        ))
    }

    /// Kompletní pipeline generování obrázku: zkontroluje prostředí, případně
    /// doinstaluje ComfyUI, stáhne model podle stylu promptu, spustí server,
    /// vygeneruje, a nakonec server zastaví (uvolní VRAM — soubory modelů
    /// zůstávají na disku pro příště). Průběh hlásí přes ImageStage chunky.
    #[allow(clippy::too_many_arguments)]
    async fn handle_image(
        &self,
        prompt: String,
        reference_image_paths: Vec<String>,
        reference_preservation: Option<String>,
        translate_image_prompt: bool,
        init_image: Option<String>,
        pulid_weight: f32,
        face_detailer: bool,
        checkpoint_override: Option<String>,
        lora_override: Option<String>,
        stream_tx: mpsc::Sender<StreamChunk>,
    ) -> AppResult<()> {
        use crate::ports::comfy_installer_port::ComfyStatus;
        use crate::ports::llm_port::{ImageStage, ImageStageInfo};

        let original_reference_count = reference_image_paths.len();
        let reference_image_paths: Vec<String> = reference_image_paths
            .into_iter()
            .take(MAX_IMAGE_GENERATION_REFERENCES)
            .collect();
        if original_reference_count > reference_image_paths.len() {
            tracing::info!(
                original_reference_count,
                used_reference_count = reference_image_paths.len(),
                "Počet referenčních obrázků pro PuLID zkrácen"
            );
        }

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
        if matches!(
            self.comfy_installer.status().await?,
            ComfyStatus::NotInstalled | ComfyStatus::Broken
        ) {
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

        // 2. Model podle stylu promptu (stáhne se, jen když chybí). Při
        // zvoleném vlastním checkpointu se stylový nestahuje — nebyl by
        // použit a klidně by tahal 6+ GB zbytečně; existenci vlastního
        // souboru ohlídá preflight kontrola v comfyui klientu.
        if checkpoint_override.is_none() {
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

        // 2c. FaceDetailer (Impact Pack) — jen když si uživatel zapnul doladění
        // obličeje. Musí se doinstalovat PŘED spuštěním serveru, jinak by
        // ComfyUI nový uzel nenačetl. Když instalace selže, generuje se dál bez
        // něj (comfyui klient si dostupnost uzlu ověří a případně ho vypustí).
        if face_detailer {
            let (itx, irx) = mpsc::channel(64);
            let fwd = tokio::spawn(forward_install_progress(
                ImageStage::DownloadingModel,
                irx,
                stream_tx.clone(),
            ));
            let result = self.comfy_installer.ensure_face_detailer_assets(itx).await;
            let _ = fwd.await;
            if let Err(e) = result {
                tracing::warn!(
                    "Instalace FaceDetaileru selhala ({e}) — generuji bez doladění obličeje"
                );
            }
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
        let reference_preservation = reference_preservation
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let prompt_for_generator = if let Some(note) = reference_preservation.as_deref() {
            if reference_image_paths.is_empty() {
                prompt.clone()
            } else {
                format!(
                    "{prompt}\nReference preservation note: keep this from the reference images: {note}"
                )
            }
        } else {
            prompt.clone()
        };
        let (mut sd_prompt, lora_query) = if translate_image_prompt {
            self.enhance_image_prompt(&prompt_for_generator, !reference_image_paths.is_empty())
                .await
        } else {
            (prompt_for_generator.clone(), None)
        };
        sd_prompt = reinforce_known_subject_tags(&sd_prompt, &prompt_for_generator);
        let lora_query = detect_lora_query(&prompt_for_generator, &sd_prompt).or(lora_query);

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
        // Ruční volba LoRA (per konverzace) má přednost — automatické hledání
        // na CivitAI se pak vynechá. Trigger words si píše uživatel do promptu
        // sám (u ručně stažených LoRA je nemáme jak zjistit).
        let mut lora_file = lora_override;
        if lora_file.is_some() {
            tracing::info!(lora = ?lora_file, "Použita ručně zvolená LoRA");
        } else if let Some(query) = lora_query {
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
                Ok(None) => {
                    tracing::info!(%query, "Žádná vhodná LoRA nenalezena");
                    // Ať to uživatel vidí v průběhu, ne jen v logu — jinak to
                    // vypadá, že se hledání ani nepokusilo.
                    let _ = stream_tx
                        .send(StreamChunk::ImageStage(ImageStageInfo {
                            stage: ImageStage::DownloadingModel,
                            detail: Some(format!(
                                "LoRA pro '{query}' nenalezena — generuji bez ní"
                            )),
                            percent: None,
                        }))
                        .await;
                }
                Err(e) => tracing::warn!("Hledání LoRA selhalo ({e}) — generuji bez ní"),
            }
        }

        // 4. Generování
        let _ = stream_tx.send(stage(ImageStage::Generating)).await;

        // Formát podle záběru: celá postava → na výšku (do čtverce se figura
        // nevejde a ořízne se u stehen), jinak čtverec. Rozhoduje původní
        // dotaz i vylepšený prompt (ten „full body" doplňuje sám).
        let full_body = wants_full_body(&prompt) || wants_full_body(&sd_prompt);
        let wide_image = wants_wide_image(&prompt) || wants_wide_image(&sd_prompt);
        let uses_reference_images = !reference_image_paths.is_empty();
        let (width, height) = if wide_image {
            (1280, 720)
        } else if full_body && uses_reference_images {
            (768, 1152)
        } else if full_body {
            (832, 1216)
        } else if uses_reference_images {
            (896, 896)
        } else {
            (1024, 1024)
        };
        let mut negative_prompt = DEFAULT_NEGATIVE_PROMPT.to_string();
        if wide_image {
            sd_prompt.push_str(
                ", wide 16:9 landscape composition, horizontal frame, wallpaper composition",
            );
            negative_prompt.push_str(", portrait orientation, vertical image, close-up crop");
        }
        if full_body {
            // U celé postavy je obličej malý → tagy na oči + hi-res průchod,
            // aby oči nevycházely rozmazané/„divné".
            if wide_image {
                sd_prompt.push_str(
                    ", full body, entire figure in frame, visible feet, detailed face, \
                     detailed symmetric eyes",
                );
            } else {
                sd_prompt.push_str(
                    ", full body, entire figure in frame, visible feet, standing, detailed face, \
                     detailed symmetric eyes",
                );
            }
            negative_prompt.push_str(", cropped, out of frame, cut off, close-up");
        }

        // Uvolníme vestavěný LLM z VRAM — velký lokální model (např. 24B) a
        // SDXL generování se na 24GB kartě nevejdou najednou. Prompt už máme
        // připravený; model se líně načte při další textové zprávě. Cloud/HTTP
        // backendy jsou no-op.
        self.llm.unload().await;

        let (img_tx, mut img_rx) = mpsc::channel(32);
        let request = ImageRequest {
            original_prompt: Some(prompt.clone()),
            prompt: sd_prompt,
            negative_prompt: Some(negative_prompt),
            reference_preservation,
            width,
            height,
            steps: 20,
            cfg_scale: 7.0,
            seed: None,
            style_preset: style,
            reference_image_paths,
            lora_file,
            init_image_path: init_image,
            // Hi-res dolaďovací průchod hlavně kvůli obličeji u celé postavy.
            hires_fix: full_body && !uses_reference_images,
            // Síla PuLID podoby a doladění obličeje (per-konverzace, z posuvníků).
            pulid_weight,
            face_detailer,
            checkpoint_override,
        };

        // Generování běží v samostatné úloze a průběh vyprazdňujeme SOUBĚŽNĚ.
        // Kdyby se drainovalo až po návratu generate() (jako dřív), zaplnil by
        // se bufferovaný kanál a generate() by se na plném kanálu zaseklo u
        // delších workflow (PuLID + hi-res průchod = přes 32 zpráv). Obrázek
        // se stihl uložit do galerie, ale Done do chatu nikdy nedorazil a UI
        // viselo na „generuji".
        let image_gen = self.image_gen.clone();
        let gen_handle = tokio::spawn(async move { image_gen.generate(request, img_tx).await });

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
                ImageProgress::Status { detail, percent } => {
                    let _ = stream_tx
                        .send(StreamChunk::ImageStage(ImageStageInfo {
                            stage: ImageStage::Generating,
                            detail: Some(detail),
                            percent,
                        }))
                        .await;
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

        // Kanál se uzavřel = generate() skončilo (img_tx se dropnul). Vyzvedneme
        // jeho výsledek; při chybě uvolníme VRAM.
        match gen_handle.await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => {
                let _ = self.comfy_installer.stop_server().await;
                Err(e)
            }
            Err(join_err) => {
                let _ = self.comfy_installer.stop_server().await;
                Err(AppError::ComfyUi(format!(
                    "Generování obrázku spadlo: {join_err}"
                )))
            }
        }
    }

    /// Převede požadavek uživatele (typicky česky) na anglický Stable
    /// Diffusion prompt + volitelný koncept pro vyhledání LoRA. Při
    /// jakémkoli selhání LLM vrací původní text — horší prompt je lepší
    /// než spadlé generování.
    async fn enhance_image_prompt(
        &self,
        user_prompt: &str,
        has_reference_images: bool,
    ) -> (String, Option<String>) {
        // Hotový anglický SD prompt vezmeme rovnou — nemá cenu ho posílat LLM
        // (zbytečné přepisování a hlavně: cenzurované modely jako Mistral by
        // explicitní/odhalený prompt odmítly přeložit a generování by spadlo).
        if is_ready_english_prompt(user_prompt) {
            return (user_prompt.to_string(), None);
        }

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
            return (
                fallback_image_prompt(user_prompt, has_reference_images),
                None,
            );
        }
        let mut out = String::new();
        while let Some(chunk) = rx.recv().await {
            match chunk {
                StreamChunk::Token(t) => out.push_str(&t),
                StreamChunk::Error(e) => {
                    tracing::warn!("Vylepšení promptu selhalo ({e}) — použije se původní");
                    return (
                        fallback_image_prompt(user_prompt, has_reference_images),
                        None,
                    );
                }
                StreamChunk::Done(_) | StreamChunk::ImageStage(_) => {}
            }
        }
        parse_prompt_enhancement(
            &out,
            &fallback_image_prompt(user_prompt, has_reference_images),
        )
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
        llm.expect_unload().returning(|| Box::pin(async {}));
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

    /// Mock repozitáře postav: žádné uložené postavy (výchozí pro testy,
    /// kde postavy nejsou předmětem).
    fn default_subject_repo() -> crate::ports::subject_repository::MockSubjectRepository {
        let mut m = crate::ports::subject_repository::MockSubjectRepository::new();
        m.expect_list().returning(|| Box::pin(async { Ok(vec![]) }));
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
            Arc::new(default_subject_repo()),
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
            Arc::new(default_subject_repo()),
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
            None,
            true,
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
                    && req.reference_preservation.as_deref() == Some("preserve face shape")
                    && req.original_prompt.is_some()
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
            Some("preserve face shape".into()),
            true,
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
    async fn image_generation_does_not_deadlock_on_many_progress_messages() {
        let mut conv_repo = MockConversationRepository::new();
        conv_repo
            .expect_find_by_id()
            .returning(|_| Box::pin(async { Ok(Some(dummy_conversation())) }));
        let mut msg_repo = MockMessageRepository::new();
        msg_repo
            .expect_save()
            .returning(|_| Box::pin(async { Ok(()) }));

        let mut image_gen = MockImageGenPort::new();
        image_gen.expect_generate().returning(|_req, tx| {
            Box::pin(async move {
                // Víc průběhových zpráv, než je buffer kanálu (32) — dřív se
                // tu generate() zaseklo na plném kanálu (drain běžel až potom).
                for step in 1..=40u32 {
                    let _ = tx.send(ImageProgress::Progress { step, total: 40 }).await;
                }
                let _ = tx
                    .send(ImageProgress::Done {
                        output_path: "/gallery/x.png".into(),
                    })
                    .await;
                Ok(())
            })
        });

        let uc = make_full_uc(
            conv_repo,
            msg_repo,
            image_prompt_llm(),
            image_gen,
            MockAttachmentStorePort::new(),
        );
        let (tx, mut rx) = mpsc::channel(256);

        // Timeout: při deadlocku by test jinak visel donekonečna.
        let res = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            uc.execute(
                ConversationId::new(),
                "nakresli hrad".into(),
                vec![],
                vec![],
                None,
                true,
                tx,
            ),
        )
        .await;
        assert!(
            res.is_ok(),
            "handle_image se zaseklo (deadlock na plném kanálu)"
        );
        res.unwrap().unwrap();

        // Obrázek dorazil do streamu jako Token s markdownem.
        let mut got_image = false;
        while let Ok(chunk) = rx.try_recv() {
            if let StreamChunk::Token(t) = chunk {
                if t.contains("/gallery/x.png") {
                    got_image = true;
                }
            }
        }
        assert!(got_image, "obrázek se nedostal do streamu chatu");
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
            None,
            true,
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
                    ..Default::default()
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
            Arc::new(default_subject_repo()),
        );
        let (tx, _rx) = mpsc::channel(8);

        uc.execute(
            ConversationId::new(),
            "jak se máš?".into(),
            vec![],
            vec![],
            None,
            true,
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
            Arc::new(default_subject_repo()),
        );

        let (tx, mut rx) = mpsc::channel(64);
        uc.execute(
            ConversationId::new(),
            "nakresli hrad na skale".into(),
            vec![],
            vec![],
            None,
            true,
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
            Arc::new(default_subject_repo()),
        );

        let (tx, mut rx) = mpsc::channel(64);
        uc.execute(
            ConversationId::new(),
            "nakresli mě jako rytíře".into(),
            vec![],
            vec!["C:/fotky/ja.png".into()],
            None,
            true,
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

    /// Zapnutý FaceDetailer (per-konverzace) musí před generováním doinstalovat
    /// Impact Pack assety a propsat sílu PuLID i příznak do ImageRequestu.
    #[tokio::test]
    async fn face_detailer_setting_installs_assets_and_reaches_request() {
        let mut conv_repo = MockConversationRepository::new();
        conv_repo
            .expect_find_by_id()
            .returning(|_| Box::pin(async { Ok(Some(dummy_conversation())) }));

        let mut msg_repo = MockMessageRepository::new();
        msg_repo
            .expect_save()
            .returning(|_| Box::pin(async { Ok(()) }));

        // Per-konverzační nastavení: síla PuLID 0.7 + zapnutý FaceDetailer
        let mut gen_repo = MockGenerationSettingsRepository::new();
        gen_repo.expect_get().returning(|_| {
            Box::pin(async {
                Ok(weave_domain::generation_settings::GenerationSettings {
                    pulid_weight: Some(0.7),
                    face_detailer: Some(true),
                    ..Default::default()
                })
            })
        });

        let mut installer = MockComfyInstallerPort::new();
        installer
            .expect_status()
            .returning(|| Box::pin(async { Ok(ComfyStatus::Running) }));
        installer
            .expect_ensure_style_checkpoint()
            .returning(|_, _| Box::pin(async { Ok(()) }));
        // Klíčové: FaceDetailer assety se musí doinstalovat právě jednou
        installer
            .expect_ensure_face_detailer_assets()
            .times(1)
            .returning(|_| Box::pin(async { Ok(()) }));
        installer
            .expect_stop_server()
            .returning(|| Box::pin(async { Ok(()) }));

        let mut image_gen = MockImageGenPort::new();
        image_gen
            .expect_generate()
            .withf(|req: &ImageRequest, _tx| {
                req.face_detailer && (req.pulid_weight - 0.7).abs() < 1e-6
            })
            .returning(|_req, tx| {
                Box::pin(async move {
                    let _ = tx
                        .send(ImageProgress::Done {
                            output_path: "/gallery/face.png".into(),
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
            Arc::new(gen_repo),
            Arc::new(installer),
            Arc::new(default_lora_catalog()),
            Arc::new(default_subject_repo()),
        );

        let (tx, mut rx) = mpsc::channel(64);
        uc.execute(
            ConversationId::new(),
            "nakresli mi portrét".into(),
            vec![],
            vec![],
            None,
            true,
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

    /// Mock repozitáře postav s danými postavami.
    fn subject_repo_with(
        subjects: Vec<weave_domain::subject::Subject>,
    ) -> crate::ports::subject_repository::MockSubjectRepository {
        let mut m = crate::ports::subject_repository::MockSubjectRepository::new();
        m.expect_list().returning(move || {
            let subjects = subjects.clone();
            Box::pin(async move { Ok(subjects) })
        });
        m
    }

    fn subject(name: &str, notes: &str, photos: &[&str]) -> weave_domain::subject::Subject {
        weave_domain::subject::Subject {
            id: format!("subject:{name}"),
            name: name.into(),
            notes: notes.into(),
            images: photos
                .iter()
                .map(|p| weave_domain::subject::SubjectImage {
                    id: format!("img:{p}"),
                    path: (*p).into(),
                    mime: "image/png".into(),
                })
                .collect(),
        }
    }

    #[test]
    fn subject_name_matching_handles_case_diacritics_and_declension() {
        // Přímá shoda + diakritika/velikost písmen
        assert!(subject_name_mentioned("Nikol", "nakresli nikol na plazi"));
        assert!(subject_name_mentioned("Růženka", "kapitola o ruzenka"));
        // Skloňování ženských jmen na -a (Růženka → Růžence/Růženku)
        assert!(subject_name_mentioned(
            "Růženka",
            "napis kapitolu o ruzence"
        ));
        assert!(subject_name_mentioned("Růženka", "videl ruzenku v lese"));
        assert!(subject_name_mentioned("Adéla", "adelu potkal u reky"));
        // Neshoda a příliš krátká jména
        assert!(!subject_name_mentioned("Nikol", "nakresli kocku"));
        assert!(!subject_name_mentioned("Al", "alabastrova vaza"));
    }

    /// Zmíněná postava (i bez diakritiky/velkých písmen) → její poznámky
    /// jdou jako system kontext do textového generování.
    #[tokio::test]
    async fn mentioned_subject_notes_reach_text_context() {
        let conv = ConversationId::new();
        let mut conv_repo = MockConversationRepository::new();
        conv_repo
            .expect_find_by_id()
            .returning(|_| Box::pin(async { Ok(Some(dummy_conversation())) }));

        let mut msg_repo = MockMessageRepository::new();
        msg_repo
            .expect_save()
            .returning(|_| Box::pin(async { Ok(()) }));
        let conv_for_list = conv.clone();
        msg_repo.expect_list_by_conversation().returning(move |_| {
            let history = vec![Message::user(
                conv_for_list.clone(),
                "napiš kapitolu o ruzence v lese",
            )];
            Box::pin(async move { Ok(history) })
        });

        let mut llm = MockLlmPort::new();
        llm.expect_chat_stream()
            .withf(|req: &ChatRequest, _tx| {
                // System kontext s popisem postavy musí být v historii
                req.messages.iter().any(|m| {
                    m.role == weave_domain::message::Role::System
                        && m.content.contains("Růženka")
                        && m.content.contains("rusovláska se zelenýma očima")
                })
            })
            .returning(|_, tx| {
                Box::pin(async move {
                    let _ = tx.send(StreamChunk::Token("kapitola".into())).await;
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
            Arc::new(subject_repo_with(vec![subject(
                "Růženka",
                "rusovláska se zelenýma očima, 25 let",
                &[],
            )])),
        );

        let (tx, mut rx) = mpsc::channel(64);
        uc.execute(
            conv,
            "napiš kapitolu o ruzence v lese".into(),
            vec![],
            vec![],
            None,
            true,
            tx,
        )
        .await
        .unwrap();
        while rx.recv().await.is_some() {}
    }

    /// Obrázkový prompt zmiňující právě jednu postavu bez přiložených fotek
    /// → fotky postavy se přiloží automaticky jako PuLID reference.
    /// Při zmínce dvou postav se nepřikládá nic (průměrování identit).
    #[tokio::test]
    async fn image_prompt_auto_attaches_single_mentioned_subject_photos() {
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
                req.reference_image_paths == ["/appdata/subjects/nikol1.png"]
            })
            .returning(|_req, tx| {
                Box::pin(async move {
                    let _ = tx
                        .send(ImageProgress::Done {
                            output_path: "/gallery/nikol.png".into(),
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
            Arc::new(default_comfy_installer()),
            Arc::new(default_lora_catalog()),
            Arc::new(subject_repo_with(vec![subject(
                "Nikol",
                "",
                &["/appdata/subjects/nikol1.png"],
            )])),
        );

        let (tx, mut rx) = mpsc::channel(64);
        uc.execute(
            ConversationId::new(),
            "vygeneruj obrázek: Nikol na pláži".into(),
            vec![],
            vec![],
            None,
            true,
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

    #[tokio::test]
    async fn image_prompt_with_two_mentioned_subjects_attaches_nothing() {
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
            .withf(|req: &ImageRequest, _tx| req.reference_image_paths.is_empty())
            .returning(|_req, tx| {
                Box::pin(async move {
                    let _ = tx
                        .send(ImageProgress::Done {
                            output_path: "/gallery/dve.png".into(),
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
            Arc::new(default_comfy_installer()),
            Arc::new(default_lora_catalog()),
            Arc::new(subject_repo_with(vec![
                subject("Nikol", "", &["/a/nikol.png"]),
                subject("Adéla", "", &["/a/adela.png"]),
            ])),
        );

        let (tx, mut rx) = mpsc::channel(64);
        uc.execute(
            ConversationId::new(),
            "vygeneruj obrázek: Nikol a Adéla na pláži".into(),
            vec![],
            vec![],
            None,
            true,
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

    /// Zvolený vlastní checkpoint (per konverzace) musí doletět do
    /// ImageRequestu a stylový checkpoint se nesmí zbytečně stahovat.
    #[tokio::test]
    async fn image_checkpoint_override_reaches_request_and_skips_style_download() {
        let mut conv_repo = MockConversationRepository::new();
        conv_repo
            .expect_find_by_id()
            .returning(|_| Box::pin(async { Ok(Some(dummy_conversation())) }));
        let mut msg_repo = MockMessageRepository::new();
        msg_repo
            .expect_save()
            .returning(|_| Box::pin(async { Ok(()) }));

        let mut gen_repo = MockGenerationSettingsRepository::new();
        gen_repo.expect_get().returning(|_| {
            Box::pin(async {
                Ok(weave_domain::generation_settings::GenerationSettings {
                    image_checkpoint: Some("realvis_ultra.safetensors".into()),
                    ..Default::default()
                })
            })
        });

        let mut installer = MockComfyInstallerPort::new();
        installer
            .expect_status()
            .returning(|| Box::pin(async { Ok(ComfyStatus::Running) }));
        // Klíčové: stylový checkpoint se při overridu nestahuje
        installer.expect_ensure_style_checkpoint().never();
        installer
            .expect_stop_server()
            .returning(|| Box::pin(async { Ok(()) }));

        let mut image_gen = MockImageGenPort::new();
        image_gen
            .expect_generate()
            .withf(|req: &ImageRequest, _tx| {
                req.checkpoint_override.as_deref() == Some("realvis_ultra.safetensors")
            })
            .returning(|_req, tx| {
                Box::pin(async move {
                    let _ = tx
                        .send(ImageProgress::Done {
                            output_path: "/gallery/custom.png".into(),
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
            Arc::new(gen_repo),
            Arc::new(installer),
            Arc::new(default_lora_catalog()),
            Arc::new(default_subject_repo()),
        );

        let (tx, mut rx) = mpsc::channel(64);
        uc.execute(
            ConversationId::new(),
            "vygeneruj obrázek hradu".into(),
            vec![],
            vec![],
            None,
            true,
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

    /// Ručně zvolená LoRA (per konverzace) musí doletět do ImageRequestu
    /// a automatické hledání na CivitAI se nesmí spustit.
    #[tokio::test]
    async fn image_lora_override_reaches_request_and_skips_civitai_search() {
        let mut conv_repo = MockConversationRepository::new();
        conv_repo
            .expect_find_by_id()
            .returning(|_| Box::pin(async { Ok(Some(dummy_conversation())) }));
        let mut msg_repo = MockMessageRepository::new();
        msg_repo
            .expect_save()
            .returning(|_| Box::pin(async { Ok(()) }));

        let mut gen_repo = MockGenerationSettingsRepository::new();
        gen_repo.expect_get().returning(|_| {
            Box::pin(async {
                Ok(weave_domain::generation_settings::GenerationSettings {
                    image_lora: Some("nikol_v1.safetensors".into()),
                    ..Default::default()
                })
            })
        });

        // Klíčové: automatické hledání LoRA se při ruční volbě nevolá
        let mut catalog = MockLoraCatalogPort::new();
        catalog.expect_find_lora().never();

        let mut image_gen = MockImageGenPort::new();
        image_gen
            .expect_generate()
            .withf(|req: &ImageRequest, _tx| {
                req.lora_file.as_deref() == Some("nikol_v1.safetensors")
            })
            .returning(|_req, tx| {
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
            Arc::new(image_prompt_llm()),
            Arc::new(image_gen),
            Arc::new(MockWorkspaceRepository::new()),
            Arc::new(MockPersonaRepository::new()),
            Arc::new(MockAttachmentStorePort::new()),
            Arc::new(gen_repo),
            Arc::new(default_comfy_installer()),
            Arc::new(catalog),
            Arc::new(default_subject_repo()),
        );

        let (tx, mut rx) = mpsc::channel(64);
        uc.execute(
            ConversationId::new(),
            "vygeneruj obrázek portrétu".into(),
            vec![],
            vec![],
            None,
            true,
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
            Arc::new(default_subject_repo()),
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

    #[test]
    fn ready_english_prompt_is_detected() {
        // Hotový anglický SD prompt → projde rovnou (bez LLM)
        assert!(is_ready_english_prompt(
            "a young woman, full body, photorealistic, natural lighting, highly detailed"
        ));
        // Krátký / český / bez čárek → potřebuje překlad
        assert!(!is_ready_english_prompt("nahá žena v lese"));
        assert!(!is_ready_english_prompt("vygeneruj mi obrázek dívky"));
        assert!(!is_ready_english_prompt("cat"));
    }

    #[test]
    fn wants_full_body_detects_cz_and_en() {
        assert!(wants_full_body("full body shot, head to toe, a woman"));
        assert!(wants_full_body(
            "celá postava od hlavy k patě, žena v pralese"
        ));
        assert!(wants_full_body("full length portrait"));
        assert!(!wants_full_body("portrait of a woman, close-up"));
        assert!(!wants_full_body("fotka kočky na zahradě"));
    }

    #[test]
    fn wants_wide_image_detects_wallpaper_requests() {
        assert!(wants_wide_image("wide cinematic 16:9 desktop wallpaper"));
        assert!(wants_wide_image("na sirku, tapeta pro QHD monitor"));
        assert!(wants_wide_image("landscape horizontal composition"));
        assert!(!wants_wide_image("portrait of a woman, close-up"));
    }

    #[test]
    fn fallback_image_prompt_translates_common_czech_terms() {
        let prompt = fallback_image_prompt("nakresli hrad na skale v noci", false);
        assert!(prompt.contains("castle"));
        assert!(prompt.contains("rock cliff"));
        assert!(prompt.contains("night"));
        assert!(!prompt.contains("nakresli"));

        let ref_prompt = fallback_image_prompt("nakresli mě jako rytíře", true);
        assert!(ref_prompt.contains("knight"));
        assert!(ref_prompt.contains("same person as reference image"));
        assert!(ref_prompt.contains("consistent facial identity"));

        let cat_prompt =
            fallback_image_prompt("vygeneruj mi obr\u{00E1}zek ko\u{010D}ky v klobouku", false);
        assert!(cat_prompt.contains("cat"));
        assert!(cat_prompt.contains("hat"));
        assert!(!cat_prompt.contains("kocky"));
        assert!(!cat_prompt.contains("klobouku"));
    }

    #[test]
    fn fallback_image_prompt_preserves_ahsoka_beach_request() {
        let prompt = fallback_image_prompt(
            "vygeneruj Ahsoka Tano na plazi sama v bikinach, animovany styl",
            false,
        );

        assert!(prompt.contains("adult Ahsoka Tano-inspired character"));
        assert!(prompt.contains("beach"));
        assert!(prompt.contains("wearing a bikini"));
        assert!(prompt.contains("solo"));
        assert!(prompt.contains("stylized animated illustration"));
    }

    #[test]
    fn deterministic_lora_query_detects_known_character() {
        assert_eq!(
            detect_lora_query(
                "vygeneruj Ahsoka Tano na plazi sama v bikinach",
                "beach, solo, high quality"
            )
            .as_deref(),
            Some("Ahsoka Tano")
        );
        assert_eq!(
            detect_lora_query("generic beach, high quality", "sand, ocean").as_deref(),
            None
        );
    }

    #[test]
    fn known_character_tags_are_reinforced_when_llm_drops_subject() {
        let prompt = reinforce_known_subject_tags(
            "beach, solo, wearing a bikini, high quality",
            "Ahsoka Tano na plazi",
        );

        assert!(prompt.contains("adult Ahsoka Tano-inspired character"));
        assert!(prompt.contains("orange skin"));
        assert!(prompt.contains("montrals and lekku"));
        assert!(prompt.contains("beach"));
    }

    #[test]
    fn parse_enhancement_strips_chatml_garbage() {
        // Reálný zašuměný výstup malého lokálního modelu: za odpovědí
        // ukecal celou ChatML šablonu a zopakoval dotaz.
        let raw = "Ahsoka Tano, Star Wars: The Clone Wars season 5, highly detailed, \
             sharp focus <|im_start|>user vygeneruj obrázek<|im_end|> <|im_start|>assistant \
             Ahsoka Tano<|im_\nLORA: Ahsoka Tano<|im_end|>";
        let (prompt, lora) = parse_prompt_enhancement(raw, "fb");

        assert_eq!(
            prompt,
            "Ahsoka Tano, Star Wars: The Clone Wars season 5, highly detailed, sharp focus"
        );
        assert!(
            !prompt.contains("<|"),
            "prompt nesmí obsahovat ChatML tokeny"
        );
        assert!(!prompt.contains("vygeneruj"), "prompt nesmí opakovat dotaz");
        assert_eq!(lora.as_deref(), Some("Ahsoka Tano"));
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
        msg_repo
            .expect_list_by_conversation()
            .returning(|_| Box::pin(async { Ok(vec![]) }));

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
        llm.expect_unload().returning(|| Box::pin(async {}));

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
            Arc::new(default_subject_repo()),
        );

        let (tx, mut rx) = mpsc::channel(64);
        uc.execute(
            ConversationId::new(),
            "nakresli Nikol jako portrét".into(),
            vec![],
            vec![],
            None,
            true,
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
    async fn wide_wallpaper_prompt_uses_landscape_dimensions() {
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

        let mut llm = MockLlmPort::new();
        llm.expect_chat_stream().returning(|_, tx| {
            Box::pin(async move {
                let _ = tx
                    .send(StreamChunk::Token(
                        "wide cinematic 16:9 wallpaper, full body, lying on her side, beach\nLORA: none"
                            .into(),
                    ))
                    .await;
                let _ = tx.send(StreamChunk::Done(Default::default())).await;
                Ok(())
            })
        });
        llm.expect_unload().returning(|| Box::pin(async {}));

        let mut installer = MockComfyInstallerPort::new();
        installer
            .expect_status()
            .returning(|| Box::pin(async { Ok(ComfyStatus::Running) }));
        installer
            .expect_ensure_style_checkpoint()
            .returning(|_, _| Box::pin(async { Ok(()) }));
        installer
            .expect_stop_server()
            .returning(|| Box::pin(async { Ok(()) }));

        let mut image_gen = MockImageGenPort::new();
        image_gen.expect_generate().returning(|req, tx| {
            assert_eq!((req.width, req.height), (1280, 720));
            assert!(req.prompt.contains("wide 16:9 landscape composition"));
            assert!(!req.prompt.contains("standing"));
            assert!(req
                .negative_prompt
                .as_deref()
                .unwrap_or_default()
                .contains("portrait orientation"));
            Box::pin(async move {
                let _ = tx
                    .send(ImageProgress::Done {
                        output_path: "/gallery/wide.png".into(),
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
            Arc::new(default_lora_catalog()),
            Arc::new(default_subject_repo()),
        );

        let (tx, mut rx) = mpsc::channel(64);
        uc.execute(
            ConversationId::new(),
            "wide 16:9 wallpaper, full body, lying on her side on a beach".into(),
            vec![],
            vec![],
            None,
            true,
            tx,
        )
        .await
        .unwrap();

        while rx.recv().await.is_some() {}
    }

    #[tokio::test]
    async fn image_prompt_falls_back_to_english_tags_on_llm_error() {
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
        llm.expect_unload().returning(|| Box::pin(async {}));

        let mut image_gen = MockImageGenPort::new();
        image_gen.expect_generate().returning(|req, tx| {
            assert!(req.prompt.contains("castle"));
            assert!(req.prompt.contains("rock cliff"));
            assert!(!req.prompt.contains("nakresli"));
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
            Arc::new(default_subject_repo()),
        );

        let (tx, mut rx) = mpsc::channel(64);
        uc.execute(
            ConversationId::new(),
            "nakresli hrad na skale".into(),
            vec![],
            vec![],
            None,
            true,
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
