// studio:database-begin
mod db;

// studio:database-end
// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![greet])
        // studio:database-begin
        // The database commands join `greet`; this replaces the handler above.
        .invoke_handler(tauri::generate_handler![
            greet,
            db::get_setting,
            db::set_setting
        ])
        .setup(|app| {
            use tauri::Manager as _;
            app.manage(db::open(&app.path().app_data_dir()?)?);
            Ok(())
        })
        // studio:database-end
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
