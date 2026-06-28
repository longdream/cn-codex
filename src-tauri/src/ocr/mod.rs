use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use base64::{Engine as _, engine::general_purpose};
use image::DynamicImage;
use image::imageops::FilterType;
use oar_ocr::domain::tasks::TextDetectionConfig;
use oar_ocr::oarocr::OAROCRBuilder;
use oar_ocr::processors::LimitType;
use oar_ocr::utils::load_image;

use crate::error::{AppError, AppResult};

const DET_MODEL_FILENAME: &str = "ppocrv5_mobile_det.onnx";
const REC_MODEL_FILENAME: &str = "ppocrv5_mobile_rec.onnx";
const VOCAB_FILENAME: &str = "ppocrv5_mobile_vocab.txt";
const RESOURCE_SUBDIR: &str = "ocr";
const MAX_OCR_IMAGE_SIDE: u32 = 1600;
#[cfg(target_os = "windows")]
const ORT_DYLIB_FILENAME: &str = "onnxruntime.dll";
#[cfg(target_os = "linux")]
const ORT_DYLIB_FILENAME: &str = "libonnxruntime.so";
#[cfg(target_os = "macos")]
const ORT_DYLIB_FILENAME: &str = "libonnxruntime.dylib";
static ORT_INIT: OnceLock<Result<(), String>> = OnceLock::new();

#[derive(Debug, Clone)]
pub struct OcrImageInput {
    pub name: String,
    pub mime_type: String,
    pub data_url: String,
}

#[derive(Debug, Clone)]
pub struct OcrImageResult {
    pub name: String,
    pub text: String,
}

pub fn extract_text_from_data_urls(
    project_root: &Path,
    images: &[OcrImageInput],
) -> AppResult<Vec<OcrImageResult>> {
    let runtime = PpOcrRuntime::load(project_root)?;
    let mut results = Vec::new();
    for image in images {
        if !image.mime_type.starts_with("image/") {
            continue;
        }
        let bytes = decode_data_url(&image.data_url)?;
        let text = runtime.infer_text_from_bytes(&bytes)?;
        if text.trim().is_empty() {
            continue;
        }
        results.push(OcrImageResult {
            name: image.name.clone(),
            text,
        });
    }
    Ok(results)
}

pub fn extract_text_from_image_file(project_root: &Path, image_path: &Path) -> AppResult<String> {
    let runtime = PpOcrRuntime::load(project_root)?;
    runtime.infer_text_from_image_path(image_path)
}

fn resolve_ocr_resource_dir(project_root: &Path) -> AppResult<PathBuf> {
    if let Ok(explicit) = std::env::var("CN_CODEX_OCR_RESOURCE_DIR") {
        let explicit_path = PathBuf::from(explicit);
        if explicit_path.exists() {
            return Ok(explicit_path);
        }
    }

    let mut candidates = vec![
        project_root
            .join("src-tauri")
            .join("resources")
            .join(RESOURCE_SUBDIR),
        project_root.join("resources").join(RESOURCE_SUBDIR),
    ];

    if let Ok(exe_path) = std::env::current_exe()
        && let Some(exe_dir) = exe_path.parent()
    {
        candidates.push(exe_dir.join("resources").join(RESOURCE_SUBDIR));
        candidates.push(
            exe_dir
                .parent()
                .unwrap_or(exe_dir)
                .join("resources")
                .join(RESOURCE_SUBDIR),
        );
    }

    for candidate in candidates {
        if candidate.is_dir() {
            return Ok(candidate);
        }
    }

    Err(AppError::Custom(
        "PP-OCRv5 mobile resources not found. Expected an ocr resource directory.".to_string(),
    ))
}

fn decode_data_url(data_url: &str) -> AppResult<Vec<u8>> {
    let (_, payload) = data_url
        .split_once(',')
        .ok_or_else(|| AppError::Custom("Invalid data URL".to_string()))?;
    general_purpose::STANDARD
        .decode(payload.as_bytes())
        .map_err(|e| AppError::Custom(format!("Failed to decode data URL: {e}")))
}

fn ensure_onnx_runtime_loaded(resource_dir: &Path) -> AppResult<()> {
    let result = ORT_INIT.get_or_init(|| {
        let dylib_path = resource_dir.join(ORT_DYLIB_FILENAME);
        if dylib_path.exists() {
            let _ = ort::init_from(&dylib_path)
                .map_err(|err| format!("ONNX Runtime init failed: {err}"))?
                .commit();
            return Ok::<(), String>(());
        }
        let _ = ort::init().commit();
        Ok::<(), String>(())
    });
    if let Err(err) = result {
        return Err(AppError::Custom(err.clone()));
    }
    Ok(())
}

fn collect_result_text(result: &oar_ocr::oarocr::OAROCRResult) -> String {
    let mut lines = Vec::new();
    for region in &result.text_regions {
        if let Some((text, confidence)) = region.text_with_confidence() {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                continue;
            }
            if confidence < 0.15 {
                continue;
            }
            lines.push(trimmed.to_string());
            continue;
        }
        if let Some(text) = region.text.as_deref() {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                lines.push(trimmed.to_string());
            }
        }
    }
    lines.join("\n")
}

struct PpOcrRuntime {
    ocr: oar_ocr::oarocr::OAROCR,
}

impl PpOcrRuntime {
    fn load(project_root: &Path) -> AppResult<Self> {
        let resource_dir = resolve_ocr_resource_dir(project_root)?;
        ensure_onnx_runtime_loaded(&resource_dir)?;
        let det_path = resource_dir.join(DET_MODEL_FILENAME);
        if !det_path.exists() {
            return Err(AppError::Custom(format!(
                "PP-OCRv5 det model not found: {}",
                det_path.display()
            )));
        }
        let rec_path = resource_dir.join(REC_MODEL_FILENAME);
        if !rec_path.exists() {
            return Err(AppError::Custom(format!(
                "PP-OCRv5 rec model not found: {}",
                rec_path.display()
            )));
        }
        let vocab_path = resource_dir.join(VOCAB_FILENAME);
        if !vocab_path.exists() {
            return Err(AppError::Custom(format!(
                "PP-OCRv5 vocab not found: {}",
                vocab_path.display()
            )));
        }

        let det_config = TextDetectionConfig {
            score_threshold: 0.5,
            box_threshold: 0.7,
            unclip_ratio: 1.8,
            max_candidates: 40,
            limit_side_len: Some(960),
            limit_type: Some(LimitType::Max),
            max_side_len: Some(2000),
        };

        let ocr = OAROCRBuilder::new(&det_path, &rec_path, &vocab_path)
            .text_detection_config(det_config)
            .image_batch_size(1)
            .region_batch_size(64)
            .build()
            .map_err(|err| AppError::Custom(format!("Failed to build PP-OCRv5 pipeline: {err}")))?;
        Ok(Self { ocr })
    }

    fn infer_text_from_image_path(&self, image_path: &Path) -> AppResult<String> {
        let image = load_image(image_path)
            .map_err(|err| AppError::Custom(format!("Failed to load image for OCR: {err}")))?;
        let image = downscale_large_image(image);
        let results = self
            .ocr
            .predict(vec![image])
            .map_err(|err| AppError::Custom(format!("PP-OCRv5 inference failed: {err}")))?;
        let text = results.first().map(collect_result_text).unwrap_or_default();
        Ok(text)
    }

    fn infer_text_from_bytes(&self, image_bytes: &[u8]) -> AppResult<String> {
        let image = image::load_from_memory(image_bytes)
            .map_err(|err| AppError::Custom(format!("Invalid image bytes: {err}")))?;
        let tmp = tempfile::tempdir()
            .map_err(|err| AppError::Custom(format!("Failed to create temp dir for OCR: {err}")))?;
        let image_path = tmp.path().join("ocr-input.png");
        save_as_png(&image, &image_path)?;
        self.infer_text_from_image_path(&image_path)
    }
}

fn downscale_large_image(image: image::RgbImage) -> image::RgbImage {
    let width = image.width();
    let height = image.height();
    let max_side = width.max(height);
    if max_side <= MAX_OCR_IMAGE_SIDE {
        return image;
    }
    let scale = MAX_OCR_IMAGE_SIDE as f32 / max_side as f32;
    let target_width = ((width as f32) * scale).round().max(1.0) as u32;
    let target_height = ((height as f32) * scale).round().max(1.0) as u32;
    image::imageops::resize(&image, target_width, target_height, FilterType::Triangle)
}

fn save_as_png(image: &DynamicImage, image_path: &Path) -> AppResult<()> {
    image
        .save(image_path)
        .map_err(|err| AppError::Custom(format!("Failed to encode temporary PNG for OCR: {err}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgb;
    use image::RgbImage;

    #[test]
    fn local_ocr_runtime_smoke_with_optional_resources() {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let project_root = manifest_dir
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or(manifest_dir);
        let ocr_dir = project_root
            .join("src-tauri")
            .join("resources")
            .join(RESOURCE_SUBDIR);
        if !ocr_dir.join(DET_MODEL_FILENAME).exists()
            || !ocr_dir.join(REC_MODEL_FILENAME).exists()
            || !ocr_dir.join(VOCAB_FILENAME).exists()
        {
            return;
        }

        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let image_path = temp_dir.path().join("blank.png");
        let image = RgbImage::from_pixel(1024, 768, Rgb([255, 255, 255]));
        image.save(&image_path).expect("write blank image");

        let result = extract_text_from_image_file(&project_root, &image_path);
        assert!(
            result.is_ok(),
            "expected local OCR inference to run, got: {result:?}"
        );
    }

    #[test]
    fn local_ocr_debug_image_from_env_path() {
        let image_path = match std::env::var("CN_CODEX_OCR_TEST_IMAGE") {
            Ok(value) if !value.trim().is_empty() => PathBuf::from(value),
            _ => return,
        };
        if !image_path.exists() {
            return;
        }

        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let project_root = manifest_dir
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or(manifest_dir);
        let ocr_dir = project_root
            .join("src-tauri")
            .join("resources")
            .join(RESOURCE_SUBDIR);
        if !ocr_dir.join(DET_MODEL_FILENAME).exists()
            || !ocr_dir.join(REC_MODEL_FILENAME).exists()
            || !ocr_dir.join(VOCAB_FILENAME).exists()
        {
            return;
        }

        let result = extract_text_from_image_file(&project_root, &image_path);
        println!("[debug-ocr] image: {}", image_path.display());
        match result {
            Ok(text) => {
                println!("[debug-ocr] text:\n{text}");
                if text.is_empty() {
                    println!("[debug-ocr] warning: OCR returned empty text for debug image");
                }
            }
            Err(err) => panic!("OCR failed for debug image: {err}"),
        }
    }
}
