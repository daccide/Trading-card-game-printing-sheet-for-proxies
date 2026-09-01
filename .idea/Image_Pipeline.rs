use std::path::{Path, PathBuf};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use fast_image_resize::images::Image as FirImage;
use fast_image_resize::{PixelType, ResizeAlg, ResizeOptions, Resizer};
use jpeg_encoder::{ColorType, Encoder};
use walkdir::WalkDir;

// ===================== PIPELINE IMMAGINI (lossless, RGBA) =====================
//
// - non fa mai `.to_rgb8()` (che buttava via l'alpha subito) ma `.to_rgba8()`
// - se serve resize, usa Lanczos3 invece di Bilinear (molto più nitido sul testo)
// - non ricomprime in JPEG: incapsula RGB e alpha come PNG per ottenere lo
//   stream deflate già filtrato (predictor Paeth incluso), poi lo riusa TALE
//   E QUALE come stream FlateDecode nel PDF, senza ricomprimere nulla a mano.

pub(crate) struct EncodedImage {
    pub(crate) rgb_flate: Vec<u8>,
    pub(crate) alpha_flate: Option<Vec<u8>>, // None se l'immagine è totalmente opaca
    pub(crate) w: u32,
    pub(crate) h: u32,
}

pub(crate) fn list_image_files(folder: &str) -> Vec<PathBuf> {
    let exts = ["png", "jpg", "jpeg", "bmp", "tiff", "tif"];
    let mut files: Vec<PathBuf> = WalkDir::new(folder)
        .max_depth(1)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter(|e| {
            e.path()
                .extension()
                .and_then(|s| s.to_str())
                .map(|s| exts.contains(&s.to_lowercase().as_str()))
                .unwrap_or(false)
        })
        .map(|e| e.path().to_path_buf())
        .collect();
    files.sort();
    files
}

/// Estrae e concatena i chunk IDAT da un PNG in memoria: è già lo stream
/// zlib/deflate con predictor PNG applicato, pronto per essere reincapsulato
/// in un XObject PDF con /Filter /FlateDecode + /DecodeParms {Predictor 15}.
fn extract_idat(png_bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 8usize; // salta la firma PNG da 8 byte
    while i + 8 <= png_bytes.len() {
        let len = u32::from_be_bytes(png_bytes[i..i + 4].try_into().unwrap()) as usize;
        let kind = &png_bytes[i + 4..i + 8];
        let data_start = i + 8;
        let data_end = data_start + len;
        if data_end + 4 > png_bytes.len() {
            break;
        }
        if kind == b"IDAT" {
            out.extend_from_slice(&png_bytes[data_start..data_end]);
        }
        i = data_end + 4; // salta il CRC
    }
    out
}

fn encode_channel_png(width: u32, height: u32, data: &[u8], color: ::image::ColorType) -> Vec<u8> {
    let mut buf = Vec::new();
    {
        let mut cursor = std::io::Cursor::new(&mut buf);
        let encoder = ::image::codecs::png::PngEncoder::new_with_quality(
            &mut cursor,
            ::image::codecs::png::CompressionType::Fast,
            ::image::codecs::png::FilterType::Adaptive,
        );
        let _ = ::image::ImageEncoder::write_image(encoder, data, width, height, color.into());
    }
    buf
}

/// Decodifica un'immagine e la prepara per la stampa alla risoluzione target,
/// mantenendo il canale alpha. Usata per fronte, retro (logo) e qualsiasi
/// immagine che finirà davvero nel PDF stampabile.
pub(crate) fn load_image_optimized(path: &Path, target_w: u32, target_h: u32) -> Option<EncodedImage> {
    let img = image::open(path).ok()?;
    let rgba = img.to_rgba8(); // <-- mantiene l'alpha, a differenza di to_rgb8()
    let (src_w, src_h) = rgba.dimensions();

    let (raw, fw, fh) = if src_w <= target_w && src_h <= target_h {
        (rgba.into_raw(), src_w, src_h)
    } else {
        let src = FirImage::from_vec_u8(src_w, src_h, rgba.into_raw(), PixelType::U8x4).ok()?;
        let mut dst = FirImage::new(target_w, target_h, PixelType::U8x4);
        let mut resizer = Resizer::new();
        resizer
            .resize(
                &src,
                &mut dst,
                &ResizeOptions::new()
                    .resize_alg(ResizeAlg::Convolution(fast_image_resize::FilterType::Lanczos3)),
            )
            .ok()?;
        (dst.into_vec(), target_w, target_h)
    };

    // Separa RGB e alpha, e controlla se l'alpha è uniformemente opaca
    // (caso comune per il fronte carta senza maschera): se sì, evitiamo
    // di generare/incorporare l'SMask, risparmiando tempo e byte.
    let px_count = (fw * fh) as usize;
    let mut rgb = Vec::with_capacity(px_count * 3);
    let mut alpha = Vec::with_capacity(px_count);
    let mut fully_opaque = true;
    for px in raw.chunks_exact(4) {
        rgb.extend_from_slice(&px[0..3]);
        if px[3] != 255 {
            fully_opaque = false;
        }
        alpha.push(px[3]);
    }

    let rgb_png = encode_channel_png(fw, fh, &rgb, ::image::ColorType::Rgb8);
    let rgb_flate = extract_idat(&rgb_png);

    let alpha_flate = if fully_opaque {
        None
    } else {
        let alpha_png = encode_channel_png(fw, fh, &alpha, ::image::ColorType::L8);
        Some(extract_idat(&alpha_png))
    };

    Some(EncodedImage { rgb_flate, alpha_flate, w: fw, h: fh })
}

/// Genera una miniatura JPEG compatta per la sola anteprima UI: qui la
/// qualità non conta (non finisce mai nel PDF stampato), conta la velocità.
pub(crate) fn image_to_thumb_b64(path: &Path) -> String {
    let img = match image::open(path) {
        Ok(i) => i,
        Err(_) => return String::new(),
    };
    let thumb = img.resize(120, 170, ::image::imageops::FilterType::Triangle);
    let rgb = thumb.to_rgb8();
    let (w, h) = rgb.dimensions();
    let mut buf = Vec::new();
    let enc = Encoder::new(&mut buf, 70);
    let _ = enc.encode(rgb.as_raw(), w as u16, h as u16, ColorType::Rgb);
    format!("data:image/jpeg;base64,{}", BASE64.encode(&buf))
}