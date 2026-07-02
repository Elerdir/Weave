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

pub fn run() {
    init_logging();

    tauri::Builder::default()
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
            build_tray(app.handle())?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::conversation::list_conversations,
            commands::conversation::create_conversation,
            commands::conversation::delete_conversation,
            commands::conversation::rename_conversation,
            commands::conversation::set_conversation_pinned,
            commands::conversation::suggest_export_filename,
            commands::conversation::export_conversation,
            commands::message::list_messages,
            commands::message::send_message,
            commands::message::regenerate_response,
            commands::message::stop_generation,
            commands::message::compact_conversation,
            commands::message::get_conversation_settings,
            commands::message::set_conversation_settings,
            commands::settings::get_api_key_status,
            commands::settings::store_api_key,
            commands::settings::delete_api_key,
            commands::settings::get_masked_api_key,
            commands::settings::get_app_setting,
            commands::settings::set_app_setting,
            commands::settings::test_comfyui_connection,
            commands::comfy_installer::get_comfyui_status,
            commands::comfy_installer::install_comfyui,
            commands::comfy_installer::start_comfyui_server,
            commands::comfy_installer::stop_comfyui_server,
            commands::comfy_installer::list_image_models,
            commands::comfy_installer::delete_image_model,
            commands::comfy_installer::save_file_copy,
            commands::settings::test_local_llm_connection,
            commands::logs::get_app_logs,
            commands::models::list_local_models,
            commands::models::list_recommended_models,
            commands::models::detect_gpu,
            commands::models::download_model,
            commands::models::download_recommended_model,
            commands::models::delete_model,
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
        ])
        .run(tauri::generate_context!())
        .expect("Chyba spuštění Tauri aplikace");
}
