use serde::Serialize;
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
}

fn gallery_dir(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("weave")
        .join("gallery"))
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
            let (prompt, negative_prompt) = if path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("png"))
            {
                weave_infrastructure::image_stamp::read_prompt_metadata(&path)
            } else {
                (None, None)
            };
            Some(GalleryImage {
                path: path.to_string_lossy().into_owned(),
                file_name: entry.file_name().to_string_lossy().into_owned(),
                size_bytes: meta.len(),
                modified_at,
                prompt,
                negative_prompt,
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
