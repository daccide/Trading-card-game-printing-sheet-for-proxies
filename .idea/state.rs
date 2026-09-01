use std::sync::Mutex;

use serde::{Deserialize, Serialize};

/// Stato di avanzamento della generazione del PDF, letto dal frontend
/// tramite polling (`get_progress`) mentre `generate_pdf` lavora su un
/// thread separato.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProgressState {
    pub value: f64,
    pub message: String,
}

/// Stato di avanzamento del caricamento delle carte da una cartella.
/// Il caricamento avviene a piccoli lotti (vedi `commands::start_load_cards`)
/// così la memoria di picco resta bassa anche con centinaia di immagini,
/// e il frontend può mostrare un contatore reale invece di restare in
/// attesa silenziosa di un'unica risposta enorme.
#[derive(Debug, Clone, Serialize)]
pub struct CardsProgress {
    pub done: usize,
    pub total: usize,
    pub finished: bool,
    pub error: Option<String>,
    pub cards: Vec<serde_json::Value>,
}

pub struct AppState {
    pub progress: Mutex<ProgressState>,
    pub cards_progress: Mutex<CardsProgress>,
}