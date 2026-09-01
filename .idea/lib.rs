mod commands;
mod image_pipeline;
mod params;
mod pdf;
mod state;

use std::sync::Mutex;

use commands::*;
use state::{AppState, CardsProgress, ProgressState};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState {
            progress: Mutex::new(ProgressState { value: 0.0, message: "Pronto".to_string() }),
            cards_progress: Mutex::new(CardsProgress {
                done: 0,
                total: 0,
                finished: true,
                error: None,
                cards: vec![],
            }),
        })
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_fs::init())
        .invoke_handler(tauri::generate_handler![
            get_progress,
            get_grid_info,
            start_load_cards,
            get_cards_progress,
            generate_pdf,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}