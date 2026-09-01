use rayon::prelude::*;
use tauri::State;

use crate::image_pipeline::{image_to_thumb_b64, list_image_files};
use crate::params::{GenerateParams, GridInfoParams, GridInfoResult};
use crate::pdf::{compute_grid_positions, make_pdf_internal, mm_to_px, PAGE_H_MM, PAGE_W_MM};
use crate::state::{AppState, CardsProgress, ProgressState};

#[tauri::command]
pub(crate) fn get_progress(state: State<AppState>) -> ProgressState {
    state.progress.lock().unwrap().clone()
}

#[tauri::command]
pub(crate) fn get_grid_info(params: GridInfoParams) -> GridInfoResult {
    let positions = compute_grid_positions(PAGE_W_MM, PAGE_H_MM, params.card_w, params.card_h, params.gap);
    GridInfoResult {
        cards_per_page: positions.len(),
        card_w_px: mm_to_px(params.card_w, params.dpi),
        card_h_px: mm_to_px(params.card_h, params.dpi),
    }
}

/// Avvia il caricamento delle carte di una cartella in un thread separato,
/// a piccoli lotti. Elaborare tutte le immagini in un colpo solo (come
/// faceva la vecchia `get_folder_images`) poteva tenere in RAM decine di
/// immagini a piena risoluzione contemporaneamente: con cartelle grandi
/// (80+ file) questo poteva esaurire la memoria e far crashare il processo.
/// Qui non teniamo mai in RAM più di CHUNK_SIZE immagini alla volta, e il
/// frontend può monitorare l'avanzamento reale con `get_cards_progress`.
#[tauri::command]
pub(crate) fn start_load_cards(folder: String, state: State<AppState>) {
    {
        let mut p = state.cards_progress.lock().unwrap();
        p.done = 0;
        p.total = 0;
        p.finished = false;
        p.error = None;
        p.cards = vec![];
    }

    let state_ptr = state.inner() as *const AppState as usize;
    std::thread::spawn(move || {
        let state_ref = unsafe { &*(state_ptr as *const AppState) };

        let images = list_image_files(&folder);
        let total = images.len();
        {
            let mut p = state_ref.cards_progress.lock().unwrap();
            p.total = total;
        }

        let n_cpus = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
        let n_threads = n_cpus.saturating_sub(1).max(1);

        let pool = match rayon::ThreadPoolBuilder::new().num_threads(n_threads).build() {
            Ok(p) => p,
            Err(e) => {
                let mut p = state_ref.cards_progress.lock().unwrap();
                p.finished = true;
                p.error = Some(format!("Errore thread pool: {}", e));
                return;
            }
        };

        const CHUNK_SIZE: usize = 8;
        for chunk in images.chunks(CHUNK_SIZE) {
            let results: Vec<serde_json::Value> = pool.install(|| {
                chunk
                    .par_iter()
                    .map(|path| {
                        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
                        let path_str = path.to_string_lossy().to_string();
                        let thumb = image_to_thumb_b64(path);
                        serde_json::json!({ "name": name, "path": path_str, "thumb": thumb })
                    })
                    .collect()
            });

            let mut p = state_ref.cards_progress.lock().unwrap();
            p.cards.extend(results);
            p.done = p.cards.len();
        }

        let mut p = state_ref.cards_progress.lock().unwrap();
        p.finished = true;
    });
}

#[tauri::command]
pub(crate) fn get_cards_progress(state: State<AppState>) -> CardsProgress {
    state.cards_progress.lock().unwrap().clone()
}

#[tauri::command]
pub(crate) fn generate_pdf(params: GenerateParams, state: State<AppState>) -> String {
    {
        let mut p = state.progress.lock().unwrap();
        p.value = 0.0;
        p.message = "Avvio...".to_string();
    }
    let state_ptr = state.inner() as *const AppState as usize;
    std::thread::spawn(move || {
        let state_ref = unsafe { &*(state_ptr as *const AppState) };
        let result = make_pdf_internal(&params, |value, message| {
            let mut p = state_ref.progress.lock().unwrap();
            p.value = value;
            p.message = message.to_string();
        });
        let mut p = state_ref.progress.lock().unwrap();
        match result {
            Ok(msg) => {
                p.value = 100.0;
                p.message = msg;
            }
            Err(e) => {
                p.value = -1.0;
                p.message = e;
            }
        }
    });
    "started".to_string()
}