use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GenerateParams {
    pub image_folder: String,
    pub output_pdf: String,
    pub logo_path: String,
    pub dpi: u32,
    pub card_w: f64,
    pub card_h: f64,
    pub gap: f64,
    pub show_crop: bool,
    pub include_back: bool,
    pub pdf_format: String,
    pub back_bleed: f64,
    pub selected_images: Vec<String>,
}

impl GenerateParams {
    /// Verifica che i parametri abbiano senso prima di avviare un lavoro
    /// potenzialmente lungo. Fallire subito con un messaggio chiaro è
    /// molto meglio di un crash a metà generazione o di un PDF corrotto:
    /// è lo stesso principio del "type-driven design" applicato ai dati
    /// che arrivano da fuori (il frontend), che il compilatore non può
    /// validare da solo perché arrivano come JSON.
    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.card_w <= 0.0 || self.card_h <= 0.0 {
            return Err("Le dimensioni della carta devono essere maggiori di zero.".into());
        }
        if self.dpi == 0 {
            return Err("Il DPI deve essere maggiore di zero.".into());
        }
        if self.output_pdf.trim().is_empty() {
            return Err("Specifica un percorso di output per il PDF.".into());
        }
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GridInfoParams {
    pub card_w: f64,
    pub card_h: f64,
    pub gap: f64,
    pub dpi: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GridInfoResult {
    pub cards_per_page: usize,
    pub card_w_px: u32,
    pub card_h_px: u32,
}