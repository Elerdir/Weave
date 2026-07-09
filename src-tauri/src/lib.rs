use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager,
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};
use weave_shell::{commands, setup_state};

/// Musí odpovídat `identifier` v tauri.conf.json — logy míří do stejné app
/// data složky, kterou pak čte log viewer přes `AppState.log_dir`.
const APP_IDENTIFIER: &str = "dev.weave.app";

/// Drží worker vlákno file appenderu naživu po celou dobu běhu procesu —
/// bez toho by se logy do souboru nedopisovaly.
static LOG_GUARD: std::sync::OnceLock<tracing_appender::non_blocking::WorkerGuard> =
    std::sync::OnceLock::new();
static CLEANUP_STARTED: AtomicBool = AtomicBool::new(false);

/// Zapne logování: barevná konzole + soubor s denní rotací
/// (`<app data>/logs/weave.log.YYYY-MM-DD`) pro log viewer v aplikaci.
/// Výchozí úroveň INFO, přepsatelná přes RUST_LOG.
fn init_logging() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    let log_dir = dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join(APP_IDENTIFIER)
        .join("logs");
    let file_appender = tracing_appender::rolling::daily(&log_dir, "weave.log");
    let (file_writer, guard) = tracing_appender::non_blocking(file_appender);
    let _ = LOG_GUARD.set(guard);

    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer())
        .with(
            tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_writer(file_writer),
        )
        .init();
}

/// Zobrazí a zaostří hlavní okno.
fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// Sestaví systray ikonu s menu (Zobrazit / Ukončit).
fn build_tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Zobrazit Weave", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Ukončit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;

    TrayIconBuilder::with_id("weave-tray")
        .icon(app.default_window_icon().unwrap().clone())
        .tooltip("Weave")
        .menu(&menu)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main_window(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

fn cleanup_runtime(app: &tauri::AppHandle) {
    if CLEANUP_STARTED.swap(true, Ordering::SeqCst) {
        return;
    }

    tracing::info!("Ukoncuji Weave: uvolnuji lokalni model, zastavuji ComfyUI a OpenVINO server");
    let state = app.state::<weave_shell::state::AppState>();
    let embedded = {
        state
            .embedded_llm
            .lock()
            .expect("embedded_llm mutex poisoned")
            .take()
            .map(|(_, client)| client)
    };
    let comfy_installer = state.comfy_installer.clone();

    tauri::async_runtime::block_on(async move {
        if let Some(client) = embedded {
            client.unload().await;
            tracing::info!("Lokalni model uvolnen pri ukonceni aplikace");
        }
        if let Err(e) = comfy_installer.stop_server().await {
            tracing::warn!("Zastaveni ComfyUI pri ukonceni selhalo: {e}");
        } else {
            tracing::info!("ComfyUI zastaveno pri ukonceni aplikace");
        }
        if let Err(e) = commands::openvino_installer::stop_managed_server().await {
            tracing::warn!("Zastaveni OpenVINO serveru pri ukonceni selhalo: {e}");
        } else {
            tracing::info!("OpenVINO server zastaven pri ukonceni aplikace");
        }
    });
}

pub fn run() {
    init_logging();

    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let app_handle = app.handle().clone();
            tauri::async_runtime::block_on(async move {
                setup_state(&app_handle)
                    .await
                    .expect("Chyba inicializace stavu");
            });

            // Předehřátí LLM na pozadí: u vestavěné inference se model začne
            // nahrávat do VRAM hned při startu, ne až s první zprávou —
            // resolve_llm ho uloží do cache v AppState. U API backendů no-op.
            let warmup_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let state = warmup_handle.state::<weave_shell::state::AppState>();
                let _ = weave_shell::commands::settings::resolve_llm(state.inner()).await;
                tracing::info!("LLM backend připraven (předehřátí při startu)");
            });

            build_tray(app.handle())?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::conversation::list_conversations,
            commands::conversation::search_conversations,
            commands::conversation::create_conversation,
            commands::conversation::delete_conversation,
            commands::conversation::rename_conversation,
            commands::conversation::set_conversation_pinned,
            commands::conversation::suggest_export_filename,
            commands::conversation::export_conversation,
            commands::message::list_messages,
            commands::message::send_message,
            commands::message::regenerate_response,
            commands::message::resend_message,
            commands::message::truncate_conversation_from,
            commands::message::stop_generation,
            commands::message::compact_conversation,
            commands::message::auto_title_conversation,
            commands::message::edit_image_message,
            commands::message::get_conversation_settings,
            commands::message::set_conversation_settings,
            commands::settings::get_api_key_status,
            commands::settings::open_settings_window,
            commands::settings::store_api_key,
            commands::settings::delete_api_key,
            commands::settings::get_masked_api_key,
            commands::settings::get_app_setting,
            commands::settings::set_app_setting,
            commands::settings::test_comfyui_connection,
            commands::settings::test_openvino_npu_connection,
            commands::settings::detect_npu,
            commands::settings::restart_runtime,
            commands::openvino_installer::get_openvino_runtime_status,
            commands::openvino_installer::list_openvino_model_profiles,
            commands::openvino_installer::install_openvino_runtime,
            commands::openvino_installer::uninstall_openvino_runtime,
            commands::openvino_installer::start_openvino_runtime_server,
            commands::openvino_installer::stop_openvino_runtime_server,
            commands::openvino_installer::download_openvino_recommended_model,
            commands::openvino_installer::download_openvino_model_profile,
            commands::comfy_installer::get_comfyui_status,
            commands::comfy_installer::diagnose_comfyui,
            commands::comfy_installer::install_comfyui,
            commands::comfy_installer::uninstall_comfyui,
            commands::comfy_installer::start_comfyui_server,
            commands::comfy_installer::stop_comfyui_server,
            commands::comfy_installer::list_image_models,
            commands::comfy_installer::delete_image_model,
            commands::comfy_installer::save_file_copy,
            commands::settings::test_local_llm_connection,
            commands::logs::get_app_logs,
            commands::logs::open_log_window,
            commands::gallery::list_gallery_images,
            commands::gallery::delete_gallery_image,
            commands::gallery::export_gallery_image_metadata,
            commands::gallery::open_image_external,
            commands::gallery::open_gallery_window,
            commands::gallery::open_gallery_detail_window,
            commands::subjects::list_subjects,
            commands::subjects::create_subject,
            commands::subjects::rename_subject,
            commands::subjects::set_subject_notes,
            commands::subjects::delete_subject,
            commands::subjects::add_subject_image,
            commands::subjects::remove_subject_image,
            commands::subjects::open_subjects_window,
            commands::catalog::search_model_catalog,
            commands::catalog::list_catalog_gguf_files,
            commands::models::list_local_models,
            commands::models::list_recommended_models,
            commands::models::detect_gpu,
            commands::models::get_download_segments,
            commands::models::set_download_segments,
            commands::models::download_model,
            commands::models::download_recommended_model,
            commands::models::delete_model,
            commands::models::get_models_dir,
            commands::models::set_models_dir,
            commands::models::recommend_gpu_layers_for_path,
            commands::settings::unload_embedded_model,
            commands::personas::list_personas,
            commands::personas::create_persona,
            commands::personas::delete_persona,
            commands::personas::set_conversation_persona,
            commands::workspace::set_workspace,
            commands::workspace::get_workspace,
            commands::workspace::index_workspace,
            commands::workspace::list_workspace_children,
            commands::workspace::read_workspace_file,
            commands::workspace::write_workspace_file,
            commands::workspace::create_workspace_entry,
            commands::workspace::delete_workspace_entry,
            commands::workspace::rename_workspace_entry,
            commands::workspace::search_workspace,
        ]);

    let app = builder
        .build(tauri::generate_context!())
        .expect("Chyba sestaveni Tauri aplikace");

    app.run(|app_handle, event| match event {
        tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit => {
            cleanup_runtime(app_handle);
        }
        _ => {}
    });
}
