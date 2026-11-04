mod commands;
mod domain;
mod persistence;
mod platform;
mod security;
mod services;

pub use platform::graphics::apply_nvidia_workaround;

use commands::AppState;
use persistence::Db;
use platform::paths::StudioPaths;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let specta = commands::specta_builder();
    tauri::Builder::default()
        // First, so a second launch exits before `setup` runs: two instances share one
        // database, and a second one's startup reconcile would mark this one's live jobs
        // interrupted. The second launch focuses this window instead.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .invoke_handler(specta.invoke_handler())
        .setup(move |app| {
            specta.mount_events(app);
            let paths = StudioPaths::from_process_env()?;
            std::fs::create_dir_all(&paths.data_dir)?;
            let db = Db::open(&paths.data_dir.join("database.sqlite"))?;
            let template_dir = app.path().resource_dir()?.join("templates/tauri-vue");
            let projects_dir = paths.default_projects_dir.clone();
            let state = AppState::new(db, paths, template_dir);

            if let Err(e) = state.projects().reconcile_on_startup() {
                eprintln!("startup reconcile failed: {e}");
            }
            state.projects().remove_stale_clone_temps(&projects_dir);
            // Warm the doctor in the background; the dashboard never waits for it.
            let warm = state.clone();
            tauri::async_runtime::spawn(async move {
                warm.doctor_report(false).await;
            });

            commands::forward_events(app.handle().clone(), state.jobs().subscribe());
            commands::forward_preview_events(app.handle().clone(), state.preview().subscribe());
            commands::forward_build_events(app.handle().clone(), state.builds().subscribe());
            state.builds().cleanup_on_startup();
            if let Err(e) = state.jobs().reconcile_on_startup() {
                eprintln!("agent job reconcile failed: {e}");
            }
            app.manage(state);
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            // Kills every claude process group; their jobs end as interrupted.
            if let tauri::RunEvent::Exit = event {
                let state = app.state::<AppState>();
                tauri::async_runtime::block_on(state.jobs().shutdown());
            }
        });
}
