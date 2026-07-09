use serde::Serialize;
use std::hash::{Hash, Hasher};
use tauri::Manager;

/// Jeden obrázek v galerii vygenerovaných obrázků.
#[derive(Debug, Clone, Serialize)]
pub struct GalleryImage {
    pub path: String,
    pub file_name: String,
    pub size_bytes: u64,
    /// Unix timestamp poslední změny — pro řazení od nejnovějších.
    pub modified_at: u64,
    /// Pozitivní prompt použitý při generování (z PNG metadat), pokud je.
    pub prompt: Option<String>,
    /// Negativní prompt (z PNG metadat), pokud byl použit.
    pub negative_prompt: Option<String>,
    pub original_prompt: Option<String>,
    pub reference_preservation: Option<String>,
    pub ai_stamped: bool,
}

fn gallery_dir(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("weave")
        .join("gallery"))
}

fn encode_query_component(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char);
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

fn stable_detail_label(path: &str) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut hasher);
    format!("gallery-detail-{:x}", hasher.finish())
}

/// Vypíše vygenerované obrázky (nejnovější první). Neexistující složka
/// = prázdná galerie, žádná chyba.
#[tauri::command]
pub async fn list_gallery_images(app: tauri::AppHandle) -> Result<Vec<GalleryImage>, String> {
    let dir = gallery_dir(&app)?;
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Ok(Vec::new());
    };

    let mut images: Vec<GalleryImage> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let is_image = path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
                matches!(
                    e.to_ascii_lowercase().as_str(),
                    "png" | "jpg" | "jpeg" | "webp"
                )
            });
            if !is_image {
                return None;
            }
            let meta = entry.metadata().ok()?;
            let modified_at = meta
                .modified()
                .ok()?
                .duration_since(std::time::UNIX_EPOCH)
                .ok()?
                .as_secs();
            // Prompt čteme jen z PNG (jiné formáty tEXt chunky nemají).
            let is_png = path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("png"));
            let prompt_meta = if is_png {
                weave_infrastructure::image_stamp::read_prompt_metadata_extended(&path)
            } else {
                Default::default()
            };
            let ai_stamped = is_png && weave_infrastructure::image_stamp::is_ai_stamped(&path);
            Some(GalleryImage {
                path: path.to_string_lossy().into_owned(),
                file_name: entry.file_name().to_string_lossy().into_owned(),
                size_bytes: meta.len(),
                modified_at,
                prompt: prompt_meta.prompt,
                negative_prompt: prompt_meta.negative_prompt,
                original_prompt: prompt_meta.original_prompt,
                reference_preservation: prompt_meta.reference_preservation,
                ai_stamped,
            })
        })
        .collect();

    images.sort_by_key(|img| std::cmp::Reverse(img.modified_at));
    Ok(images)
}

/// Smaže obrázek z galerie (jen podle názvu souboru — path traversal guard).
#[tauri::command]
pub async fn delete_gallery_image(file_name: String, app: tauri::AppHandle) -> Result<(), String> {
    if file_name.contains('/') || file_name.contains('\\') || file_name.contains("..") {
        return Err(format!("Neplatný název souboru: {file_name}"));
    }
    let path = gallery_dir(&app)?.join(&file_name);
    std::fs::remove_file(&path).map_err(|e| format!("Smazání obrázku selhalo: {e}"))
}

/// Otevře obrázek v systémovém prohlížeči fotek (výchozí aplikace OS pro
/// daný typ). `shell open` z frontendu na tohle nestačí — jeho scope povoluje
/// jen URL, ne souborové cesty.
#[tauri::command]
pub async fn export_gallery_image_metadata(
    file_name: String,
    dest: String,
    app: tauri::AppHandle,
) -> Result<(), String> {
    if file_name.contains('/') || file_name.contains('\\') || file_name.contains("..") {
        return Err(format!("NeplatnĂ˝ nĂˇzev souboru: {file_name}"));
    }
    let image = list_gallery_images(app)
        .await?
        .into_iter()
        .find(|img| img.file_name == file_name)
        .ok_or_else(|| format!("ObrĂˇzek v galerii nenalezen: {file_name}"))?;
    let json = serde_json::to_string_pretty(&image).map_err(|e| e.to_string())?;
    std::fs::write(&dest, json).map_err(|e| format!("Export metadat selhal: {e}"))
}

#[tauri::command]
pub fn open_image_external(path: String) -> Result<(), String> {
    let result = {
        #[cfg(target_os = "windows")]
        {
            // `cmd /C start "" "<cesta>"` — první "" je (prázdný) titulek okna.
            std::process::Command::new("cmd")
                .args(["/C", "start", "", &path])
                .spawn()
        }
        #[cfg(target_os = "macos")]
        {
            std::process::Command::new("open").arg(&path).spawn()
        }
        #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
        {
            std::process::Command::new("xdg-open").arg(&path).spawn()
        }
    };
    result
        .map(|_| ())
        .map_err(|e| format!("Otevření obrázku selhalo: {e}"))
}

/// Otevře galerii v samostatném okně (nebo zaostří už otevřené).
#[tauri::command]
pub async fn open_gallery_window(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(win) = app.get_webview_window("gallery") {
        win.show().map_err(|e| e.to_string())?;
        win.unminimize().map_err(|e| e.to_string())?;
        win.set_focus().map_err(|e| e.to_string())?;
        return Ok(());
    }

    tauri::WebviewWindowBuilder::new(
        &app,
        "gallery",
        tauri::WebviewUrl::App("index.html?view=gallery".into()),
    )
    .title("Weave — Galerie")
    .inner_size(1100.0, 800.0)
    .min_inner_size(640.0, 480.0)
    .build()
    .map_err(|e| format!("Otevření galerie selhalo: {e}"))?;
    Ok(())
}

/// Otevře detail jednoho obrázku v samostatném okně.
#[tauri::command]
pub async fn open_gallery_detail_window(
    app: tauri::AppHandle,
    path: String,
) -> Result<(), String> {
    let label = stable_detail_label(&path);
    if let Some(win) = app.get_webview_window(&label) {
        win.show().map_err(|e| e.to_string())?;
        win.unminimize().map_err(|e| e.to_string())?;
        win.set_focus().map_err(|e| e.to_string())?;
        return Ok(());
    }

    let url = format!(
        "index.html?view=gallery-detail&path={}",
        encode_query_component(&path)
    );
    tauri::WebviewWindowBuilder::new(&app, label, tauri::WebviewUrl::App(url.into()))
        .title("Weave - Detail obrazku")
        .inner_size(1220.0, 840.0)
        .min_inner_size(860.0, 620.0)
        .build()
        .map_err(|e| format!("Otevreni detailu obrazku selhalo: {e}"))?;
    Ok(())
}
