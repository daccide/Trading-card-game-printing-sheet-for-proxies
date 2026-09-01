use std::collections::HashMap;
use std::path::{Path, PathBuf};

use lopdf::content::{Content, Operation};
use lopdf::{dictionary, Document, Object, Stream, StringFormat};
use rayon::prelude::*;

use crate::image_pipeline::{list_image_files, load_image_optimized, EncodedImage};
use crate::params::GenerateParams;

pub(crate) const PAGE_W_MM: f64 = 210.0;
pub(crate) const PAGE_H_MM: f64 = 297.0;
const MM_TO_PT: f64 = 2.834645669;

// Profilo ICC sRGB reale, preso da Windows (System32\spool\drivers\color)
// e copiato in src-tauri/assets/sRGB.icc prima della compilazione. Non è
// possibile "inventare" byte ICC validi: un profilo leggermente sbagliato
// romperebbe la conformità invece di garantirla, quindi qui incorporiamo
// un file vero, non generato a mano.
const SRGB_ICC_PROFILE: &[u8] = include_bytes!("D:/card-printer-updated/card_printer_professional/src-tauri/src-tauri/assets/sRGB.icc");

pub(crate) fn mm_to_pt(mm: f64) -> f64 {
    mm * MM_TO_PT
}

pub(crate) fn mm_to_px(mm: f64, dpi: u32) -> u32 {
    (mm / 25.4 * dpi as f64) as u32
}

pub(crate) fn compute_grid_positions(
    page_w: f64,
    page_h: f64,
    card_w: f64,
    card_h: f64,
    gap: f64,
) -> Vec<(f64, f64)> {
    let cols = ((page_w + gap) / (card_w + gap)).floor() as usize;
    let rows = ((page_h + gap) / (card_h + gap)).floor() as usize;
    let cols = cols.max(1);
    let rows = rows.max(1);
    let grid_w = cols as f64 * card_w + (cols - 1) as f64 * gap;
    let grid_h = rows as f64 * card_h + (rows - 1) as f64 * gap;
    let x_start = (page_w - grid_w) / 2.0;
    let y_start = (page_h - grid_h) / 2.0;
    let mut positions = Vec::new();
    for r in 0..rows {
        for c in 0..cols {
            positions.push((
                x_start + c as f64 * (card_w + gap),
                y_start + r as f64 * (card_h + gap),
            ));
        }
    }
    positions
}

/// Aggiunge in coda i nuovi operatori al content stream di una pagina,
/// creando l'array "Contents" se non esiste ancora.
fn append_content(doc: &mut Document, page_id: lopdf::ObjectId, ops: Vec<Operation>) {
    let content_data = Content { operations: ops }.encode().unwrap_or_default();
    let content_id = doc.add_object(Stream::new(dictionary! {}, content_data));

    let page = doc.get_object_mut(page_id).unwrap();
    if let Ok(dict) = page.as_dict_mut() {
        match dict.get(b"Contents") {
            Ok(Object::Array(arr)) => {
                let mut arr = arr.clone();
                arr.push(Object::Reference(content_id));
                dict.set("Contents", Object::Array(arr));
            }
            Ok(Object::Reference(r)) => {
                let r = *r;
                dict.set(
                    "Contents",
                    Object::Array(vec![Object::Reference(r), Object::Reference(content_id)]),
                );
            }
            _ => {
                dict.set("Contents", Object::Reference(content_id));
            }
        }
    }
}

/// Incorpora un'immagine nel documento come XObject e ne restituisce l'id.
/// Se la stessa immagine va disegnata più volte (logo del retro, carte con
/// quantità > 1), chiama questa funzione UNA SOLA VOLTA e riusa l'id con
/// `draw_registered_image` per ogni posizione.
fn register_image_xobject(doc: &mut Document, encoded: &EncodedImage) -> lopdf::ObjectId {
    let smask_id = encoded.alpha_flate.as_ref().map(|alpha_data| {
        let alpha_stream = Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => encoded.w as i64,
                "Height" => encoded.h as i64,
                "ColorSpace" => "DeviceGray",
                "BitsPerComponent" => 8i64,
                "Filter" => "FlateDecode",
                "DecodeParms" => dictionary! {
                    "Predictor" => 15i64,
                    "Colors" => 1i64,
                    "BitsPerComponent" => 8i64,
                    "Columns" => encoded.w as i64,
                },
            },
            alpha_data.clone(),
        );
        doc.add_object(alpha_stream)
    });

    let mut img_dict = dictionary! {
        "Type" => "XObject",
        "Subtype" => "Image",
        "Width" => encoded.w as i64,
        "Height" => encoded.h as i64,
        "ColorSpace" => "DeviceRGB",
        "BitsPerComponent" => 8i64,
        "Filter" => "FlateDecode",
        "DecodeParms" => dictionary! {
            "Predictor" => 15i64,
            "Colors" => 3i64,
            "BitsPerComponent" => 8i64,
            "Columns" => encoded.w as i64,
        },
    };
    if let Some(sid) = smask_id {
        img_dict.set("SMask", Object::Reference(sid));
    }
    let img_stream = Stream::new(img_dict, encoded.rgb_flate.clone());
    doc.add_object(img_stream)
}

/// Disegna un'immagine già registrata in una posizione della pagina.
fn draw_registered_image(
    doc: &mut Document,
    page_id: lopdf::ObjectId,
    xobject_id: lopdf::ObjectId,
    x_mm: f64,
    y_mm: f64,
    w_mm: f64,
    h_mm: f64,
    img_name: &str,
) {
    let x_pt = mm_to_pt(x_mm);
    let y_pt = mm_to_pt(y_mm);
    let w_pt = mm_to_pt(w_mm);
    let h_pt = mm_to_pt(h_mm);

    {
        let page = doc.get_object_mut(page_id).unwrap();
        if let Ok(dict) = page.as_dict_mut() {
            let resources = dict.get_mut(b"Resources").unwrap().as_dict_mut().unwrap();
            let xobjects = resources.get_mut(b"XObject").unwrap().as_dict_mut().unwrap();
            xobjects.set(img_name, Object::Reference(xobject_id));
        }
    }

    let ops = vec![
        Operation::new("q", vec![]),
        Operation::new(
            "cm",
            vec![w_pt.into(), 0.into(), 0.into(), h_pt.into(), x_pt.into(), y_pt.into()],
        ),
        Operation::new("Do", vec![Object::Name(img_name.as_bytes().to_vec())]),
        Operation::new("Q", vec![]),
    ];
    append_content(doc, page_id, ops);
}

fn add_crop_marks(
    doc: &mut Document,
    page_id: lopdf::ObjectId,
    x_mm: f64,
    y_mm: f64,
    w_mm: f64,
    h_mm: f64,
    mark_len_mm: f64,
) {
    let x = mm_to_pt(x_mm);
    let y = mm_to_pt(y_mm);
    let w = mm_to_pt(w_mm);
    let h = mm_to_pt(h_mm);
    let ml = mm_to_pt(mark_len_mm);
    let mut ops = vec![Operation::new("q", vec![]), Operation::new("w", vec![Object::Real(0.5)])];
    for (x1, y1, x2, y2) in [
        (x, y, x + ml, y),
        (x, y, x, y + ml),
        (x + w, y, x + w - ml, y),
        (x + w, y, x + w, y + ml),
        (x, y + h, x + ml, y + h),
        (x, y + h, x, y + h - ml),
        (x + w, y + h, x + w - ml, y + h),
        (x + w, y + h, x + w, y + h - ml),
    ] {
        ops.push(Operation::new("m", vec![x1.into(), y1.into()]));
        ops.push(Operation::new("l", vec![x2.into(), y2.into()]));
        ops.push(Operation::new("S", vec![]));
    }
    ops.push(Operation::new("Q", vec![]));
    append_content(doc, page_id, ops);
}

fn new_page(doc: &mut Document, pages_id: lopdf::ObjectId) -> lopdf::ObjectId {
    let pw = mm_to_pt(PAGE_W_MM) as f32;
    let ph = mm_to_pt(PAGE_H_MM) as f32;
    let media_box = vec![Object::Integer(0), Object::Integer(0), Object::Real(pw), Object::Real(ph)];
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages_id,
        "MediaBox" => media_box.clone(),
        // TrimBox/BleedBox esplicitamente dichiarati: per un foglio con più
        // carte imposte, il "trim" ufficiale è l'intero foglio — sono i
        // segni di taglio già disegnati a indicare dove tagliare le singole
        // carte, esattamente come avviene nella prassi prepress reale.
        "TrimBox" => media_box.clone(),
        "BleedBox" => media_box,
        "Resources" => dictionary! { "XObject" => dictionary! {} },
    });
    if let Ok(pages) = doc.get_object_mut(pages_id) {
        if let Ok(dict) = pages.as_dict_mut() {
            if let Ok(Object::Array(kids)) = dict.get_mut(b"Kids") {
                kids.push(Object::Reference(page_id));
            }
            if let Ok(Object::Integer(n)) = dict.get_mut(b"Count") {
                *n += 1;
            }
        }
    }
    page_id
}

/// Aggiunge al documento l'OutputIntent richiesto da PDF/X-4: incorpora il
/// profilo ICC sRGB reale e lo referenzia dal Catalog. Senza questo, la
/// scelta "PDF/X-4" nell'interfaccia non avrebbe alcun effetto sul file.
fn add_output_intent_x4(doc: &mut Document, catalog_id: lopdf::ObjectId) {
    let icc_stream = Stream::new(
        dictionary! {
            "N" => 3i64,
            "Alternate" => "DeviceRGB",
        },
        SRGB_ICC_PROFILE.to_vec(),
    );
    let icc_id = doc.add_object(icc_stream);

    let output_intent = dictionary! {
        "Type" => "OutputIntent",
        "S" => "GTS_PDFX",
        "OutputConditionIdentifier" => Object::String(b"sRGB IEC61966-2.1".to_vec(), StringFormat::Literal),
        "RegistryName" => Object::String(b"http://www.color.org".to_vec(), StringFormat::Literal),
        "Info" => Object::String(b"sRGB IEC61966-2.1".to_vec(), StringFormat::Literal),
        "DestOutputProfile" => Object::Reference(icc_id),
    };
    let oi_id = doc.add_object(Object::Dictionary(output_intent));

    if let Ok(cat) = doc.get_object_mut(catalog_id) {
        if let Ok(dict) = cat.as_dict_mut() {
            dict.set("OutputIntents", Object::Array(vec![Object::Reference(oi_id)]));
        }
    }
}

/// Aggiunge il pacchetto XMP minimale che identifica il documento come
/// PDF/X-4: è questo (non solo l'OutputIntent) il pezzo che un validatore
/// controlla per riconoscere formalmente la conformità.
fn add_xmp_metadata_x4(doc: &mut Document, catalog_id: lopdf::ObjectId) {
    let xmp: &[u8] = b"<?xpacket begin=\"\xEF\xBB\xBF\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n\
<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\n\
 <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n\
  <rdf:Description rdf:about=\"\" xmlns:pdfxid=\"http://www.npes.org/pdfx/ns/id/\">\n\
   <pdfxid:GTS_PDFXVersion>PDF/X-4</pdfxid:GTS_PDFXVersion>\n\
  </rdf:Description>\n\
 </rdf:RDF>\n\
</x:xmpmeta>\n\
<?xpacket end=\"w\"?>";

    let xmp_stream = Stream::new(dictionary! { "Type" => "Metadata", "Subtype" => "XML" }, xmp.to_vec());
    let xmp_id = doc.add_object(xmp_stream);

    if let Ok(cat) = doc.get_object_mut(catalog_id) {
        if let Ok(dict) = cat.as_dict_mut() {
            dict.set("Metadata", Object::Reference(xmp_id));
        }
    }
}

pub(crate) fn make_pdf_internal(
    params: &GenerateParams,
    progress_cb: impl Fn(f64, &str),
) -> Result<String, String> {
    params.validate()?;

    let images: Vec<PathBuf> = if params.selected_images.is_empty() {
        list_image_files(&params.image_folder)
    } else {
        params.selected_images.iter().map(PathBuf::from).collect()
    };

    if images.is_empty() {
        return Err("Nessuna immagine trovata!".to_string());
    }

    let is_x4 = params.pdf_format == "x4";

    let card_w_px = mm_to_px(params.card_w, params.dpi);
    let card_h_px = mm_to_px(params.card_h, params.dpi);
    let positions = compute_grid_positions(PAGE_W_MM, PAGE_H_MM, params.card_w, params.card_h, params.gap);
    let slots = positions.len();
    let total = images.len();

    // Deduplica per percorso: con quantità multiple lo stesso file compare
    // più volte in `images`. Lavoro pesante (decode/resize/compressione)
    // fatto una sola volta per immagine unica, riusato per ogni copia.
    let mut unique_paths: Vec<PathBuf> = Vec::new();
    let mut path_to_unique_idx: HashMap<&Path, usize> = HashMap::new();
    for p in &images {
        if !path_to_unique_idx.contains_key(p.as_path()) {
            path_to_unique_idx.insert(p.as_path(), unique_paths.len());
            unique_paths.push(p.clone());
        }
    }

    progress_cb(
        0.0,
        &format!("Conversione {} immagini uniche (di {} totali)...", unique_paths.len(), total),
    );

    let unique_encoded: Vec<Option<EncodedImage>> = unique_paths
        .par_iter()
        .map(|path| load_image_optimized(path, card_w_px, card_h_px))
        .collect();

    let image_idx_per_slot: Vec<usize> = images
        .iter()
        .map(|p| path_to_unique_idx[p.as_path()])
        .collect();

    progress_cb(50.0, "Generazione PDF...");

    let back_encoded: Option<EncodedImage> = if params.include_back && !params.logo_path.is_empty() {
        let bw = mm_to_px(params.card_w + params.back_bleed * 2.0, params.dpi);
        let bh = mm_to_px(params.card_h + params.back_bleed * 2.0, params.dpi);
        load_image_optimized(Path::new(&params.logo_path), bw, bh)
    } else {
        None
    };

    // PDF/X-4 richiede PDF 1.6 come minimo; per lo standard restiamo a 1.4.
    let mut doc = Document::with_version(if is_x4 { "1.6" } else { "1.4" });
    let pages_id = doc.new_object_id();
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Kids" => Object::Array(vec![]), "Count" => Object::Integer(0),
        }),
    );
    let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog_id);

    if is_x4 {
        add_output_intent_x4(&mut doc, catalog_id);
        add_xmp_metadata_x4(&mut doc, catalog_id);
    }

    let back_xobject_id = back_encoded.as_ref().map(|enc| register_image_xobject(&mut doc, enc));

    let chunks: Vec<&[usize]> = image_idx_per_slot.chunks(slots).collect();
    let total_chunks = chunks.len();

    let mut front_xobj_cache: HashMap<usize, lopdf::ObjectId> = HashMap::new();

    for (ci, chunk) in chunks.iter().enumerate() {
        if params.include_back {
            let back_page_id = new_page(&mut doc, pages_id);
            if let Some(xobj_id) = back_xobject_id {
                let bleed = params.back_bleed;
                let cw = params.card_w + bleed * 2.0;
                let ch = params.card_h + bleed * 2.0;
                for (si, pos) in positions.iter().enumerate() {
                    if si >= chunk.len() {
                        break;
                    }
                    let xb = PAGE_W_MM - pos.0 - params.card_w - bleed;
                    let yb = pos.1 - bleed;
                    draw_registered_image(&mut doc, back_page_id, xobj_id, xb, yb, cw, ch, &format!("Ib{}", si));
                }
            }
            progress_cb(
                50.0 + ci as f64 / total_chunks as f64 * 25.0,
                &format!("Retro {}/{}", ci + 1, total_chunks),
            );

            let front_page_id = new_page(&mut doc, pages_id);
            for (si, pos) in positions.iter().enumerate() {
                if si >= chunk.len() {
                    break;
                }
                let uidx = chunk[si];
                if let Some(ref enc) = unique_encoded[uidx] {
                    let xobj_id = *front_xobj_cache
                        .entry(uidx)
                        .or_insert_with(|| register_image_xobject(&mut doc, enc));
                    draw_registered_image(
                        &mut doc,
                        front_page_id,
                        xobj_id,
                        pos.0,
                        pos.1,
                        params.card_w,
                        params.card_h,
                        &format!("If{}", si),
                    );
                }
                if params.show_crop {
                    add_crop_marks(&mut doc, front_page_id, pos.0, pos.1, params.card_w, params.card_h, 3.0);
                }
            }
            progress_cb(
                75.0 + ci as f64 / total_chunks as f64 * 23.0,
                &format!("Fronte {}/{}", ci + 1, total_chunks),
            );
        } else {
            let page_id = new_page(&mut doc, pages_id);
            for (si, pos) in positions.iter().enumerate() {
                if si >= chunk.len() {
                    break;
                }
                let uidx = chunk[si];
                if let Some(ref enc) = unique_encoded[uidx] {
                    let xobj_id = *front_xobj_cache
                        .entry(uidx)
                        .or_insert_with(|| register_image_xobject(&mut doc, enc));
                    draw_registered_image(
                        &mut doc,
                        page_id,
                        xobj_id,
                        pos.0,
                        pos.1,
                        params.card_w,
                        params.card_h,
                        &format!("If{}", si),
                    );
                }
                if params.show_crop {
                    add_crop_marks(&mut doc, page_id, pos.0, pos.1, params.card_w, params.card_h, 3.0);
                }
            }
            progress_cb(
                50.0 + ci as f64 / total_chunks as f64 * 48.0,
                &format!("Pagina {}/{}", ci + 1, total_chunks),
            );
        }
    }

    progress_cb(95.0, "Salvataggio PDF...");
    doc.save(&params.output_pdf).map_err(|e| format!("Errore salvataggio: {}", e))?;

    let mode = if params.include_back { "duplex" } else { "solo fronte" };
    let pages = total_chunks * if params.include_back { 2 } else { 1 };
    Ok(format!("PDF creato ({}, {} carte, {} pagine)", mode, total, pages))
}