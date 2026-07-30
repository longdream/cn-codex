use super::*;

pub(crate) fn image_generate_display(args: &ImageGenerateArgs) -> String {
    let prompt = condense_whitespace(&args.prompt);
    if prompt.is_empty() {
        return "image_generate".to_string();
    }
    if prompt.chars().count() > 64 {
        format!("{}...", prompt.chars().take(64).collect::<String>())
    } else {
        prompt
    }
}


pub(crate) fn image_generation_api_key(settings_api_key: Option<&str>) -> Option<String> {
    settings_api_key
        .and_then(|value| non_empty_string(value.to_string()))
        .or_else(|| {
            std::env::var("CN_CODEX_IMAGE_API_KEY")
                .ok()
                .and_then(non_empty_string)
        })
        .or_else(|| {
            std::env::var("OPENAI_API_KEY")
                .ok()
                .and_then(non_empty_string)
        })
}


pub(crate) fn image_generation_model(model: Option<&str>, settings_model: Option<&str>) -> String {
    model
        .and_then(|value| non_empty_string(value.to_string()))
        .or_else(|| settings_model.and_then(|value| non_empty_string(value.to_string())))
        .or_else(|| {
            std::env::var("CN_CODEX_IMAGE_MODEL")
                .ok()
                .and_then(non_empty_string)
        })
        .unwrap_or_else(|| "gpt-image-2".to_string())
}


pub(crate) fn image_generation_api_url(base_url: Option<&str>, settings_base_url: Option<&str>) -> String {
    let base = base_url
        .and_then(|value| non_empty_string(value.to_string()))
        .or_else(|| settings_base_url.and_then(|value| non_empty_string(value.to_string())))
        .or_else(|| {
            std::env::var("CN_CODEX_IMAGE_BASE_URL")
                .ok()
                .and_then(non_empty_string)
        })
        .unwrap_or_else(|| "https://api.openai.com/v1".to_string());
    let trimmed = base.trim_end_matches('/');
    if trimmed.ends_with("/images/generations") {
        trimmed.to_string()
    } else {
        format!("{trimmed}/images/generations")
    }
}


pub(crate) fn image_generation_request_body(
    prompt: &str,
    model: &str,
    size: Option<&str>,
    quality: Option<&str>,
    background: Option<&str>,
    n: Option<u32>,
) -> serde_json::Value {
    let mut body = serde_json::Map::new();
    body.insert(
        "model".to_string(),
        serde_json::Value::String(model.to_string()),
    );
    body.insert(
        "prompt".to_string(),
        serde_json::Value::String(prompt.to_string()),
    );
    body.insert(
        "size".to_string(),
        serde_json::Value::String(
            size.and_then(|value| non_empty_string(value.to_string()))
                .unwrap_or_else(|| "1024x1024".to_string()),
        ),
    );
    if let Some(value) = quality.and_then(|value| non_empty_string(value.to_string())) {
        body.insert("quality".to_string(), serde_json::Value::String(value));
    }
    if let Some(value) = background.and_then(|value| non_empty_string(value.to_string())) {
        body.insert("background".to_string(), serde_json::Value::String(value));
    }
    body.insert(
        "n".to_string(),
        serde_json::Value::Number(serde_json::Number::from(image_generation_count(n))),
    );
    body.insert(
        "response_format".to_string(),
        serde_json::Value::String("b64_json".to_string()),
    );
    serde_json::Value::Object(body)
}


pub(crate) fn image_generation_count(n: Option<u32>) -> u32 {
    n.unwrap_or(1).clamp(1, 10)
}


pub(crate) fn decode_image_base64(input: &str) -> Result<Vec<u8>, String> {
    let payload = input
        .trim()
        .split_once(',')
        .map(|(_, payload)| payload)
        .unwrap_or_else(|| input.trim());
    let compact: String = payload.chars().filter(|ch| !ch.is_whitespace()).collect();
    general_purpose::STANDARD
        .decode(compact)
        .map_err(|e| format!("image_generate returned invalid base64 image data: {e}"))
}


pub(crate) fn resolve_image_generate_output_path(
    root: &Path,
    workspace_config_dir: &Path,
    input: Option<&str>,
    call_id: &str,
    prompt: &str,
    extension: &str,
) -> Result<PathBuf, String> {
    let path = if let Some(input) = input.map(str::trim).filter(|value| !value.is_empty()) {
        let raw = PathBuf::from(input);
        if raw.is_absolute() {
            raw
        } else {
            let mut resolved = root.to_path_buf();
            for component in raw.components() {
                match component {
                    Component::Normal(part) => resolved.push(part),
                    Component::CurDir => {}
                    Component::ParentDir => {
                        return Err(
                            "Error: relative image output paths must not contain '..'".to_string()
                        );
                    }
                    _ => return Err("Error: invalid image output path component".to_string()),
                }
            }
            resolved
        }
    } else {
        let safe_call_id = sanitize_tool_name_part(call_id, "image");
        let prompt_hash = stable_hash_hex(prompt);
        workspace_config_dir
            .join("images")
            .join("generated")
            .join(format!(
                "{}-{}.{}",
                safe_call_id,
                &prompt_hash[..8],
                extension
            ))
    };

    Ok(ensure_image_output_extension(path, extension))
}


#[allow(clippy::too_many_arguments)]
pub(crate) fn resolve_image_generate_output_path_for_index(
    root: &Path,
    workspace_config_dir: &Path,
    input: Option<&str>,
    call_id: &str,
    prompt: &str,
    extension: &str,
    index: usize,
    count: usize,
) -> Result<PathBuf, String> {
    let path = resolve_image_generate_output_path(
        root,
        workspace_config_dir,
        input,
        call_id,
        prompt,
        extension,
    )?;
    if count <= 1 {
        return Ok(path);
    }
    Ok(add_image_output_index_suffix(path, index + 1, extension))
}


pub(crate) fn add_image_output_index_suffix(mut path: PathBuf, index: usize, extension: &str) -> PathBuf {
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("image");
    let ext = path
        .extension()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or(extension);
    path.set_file_name(format!("{stem}-{index}.{ext}"));
    path
}


pub(crate) fn ensure_image_output_extension(mut path: PathBuf, extension: &str) -> PathBuf {
    if path.extension().is_none() {
        path.set_extension(extension);
    }
    path
}


pub(crate) fn image_extension_for_info(info: &ImageInfo) -> &'static str {
    match info.format {
        "JPEG" => "jpg",
        "GIF" => "gif",
        "WebP" => "webp",
        _ => "png",
    }
}


pub(crate) fn workspace_relative_display_path(root: &Path, path: &Path) -> Option<String> {
    let root = root.canonicalize().ok()?;
    let path = path.canonicalize().ok()?;
    path.strip_prefix(root)
        .ok()
        .map(|value| value.to_string_lossy().replace('\\', "/"))
}


pub(crate) fn format_image_generation_http_error(status: u16, body: &str) -> String {
    if let Ok(parsed) = serde_json::from_str::<ImageGenerationErrorResponse>(body) {
        if let Some(error) = parsed.error {
            if let Some(message) = error.message.map(|value| value.trim().to_string()) {
                if !message.is_empty() {
                    if let Some(code) = error.code {
                        return format!(
                            "image_generate request failed with HTTP {status}: {message} (code: {code})"
                        );
                    }
                    return format!("image_generate request failed with HTTP {status}: {message}");
                }
            }
        }
    }

    let body = truncate_output(body.trim(), 1000);
    if body.is_empty() {
        format!("image_generate request failed with HTTP {status}")
    } else {
        format!("image_generate request failed with HTTP {status}: {body}")
    }
}


pub(crate) fn non_empty_string(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}


pub(crate) fn resolve_view_image_path(root: &Path, input: &str) -> Result<PathBuf, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("Error: image path must not be empty".to_string());
    }

    let path = PathBuf::from(trimmed);
    if path.is_absolute() {
        return Ok(path);
    }

    let mut resolved = root.to_path_buf();
    for component in path.components() {
        match component {
            Component::Normal(part) => resolved.push(part),
            Component::CurDir => {}
            Component::ParentDir => {
                return Err("Error: relative image paths must not contain '..'".to_string());
            }
            _ => return Err("Error: invalid image path component".to_string()),
        }
    }
    Ok(resolved)
}


pub(crate) fn inspect_image_bytes(bytes: &[u8]) -> Result<ImageInfo, String> {
    inspect_png(bytes)
        .or_else(|| inspect_gif(bytes))
        .or_else(|| inspect_jpeg(bytes))
        .or_else(|| inspect_webp(bytes))
        .ok_or_else(|| {
            "unsupported or invalid image format; supported formats are PNG, JPEG, GIF, and WebP"
                .to_string()
        })
}


pub(crate) fn inspect_png(bytes: &[u8]) -> Option<ImageInfo> {
    if bytes.len() < 24 {
        return None;
    }
    if &bytes[0..8] != b"\x89PNG\r\n\x1A\n" || &bytes[12..16] != b"IHDR" {
        return None;
    }

    Some(ImageInfo {
        format: "PNG",
        mime: "image/png",
        width: u32::from_be_bytes(bytes[16..20].try_into().ok()?),
        height: u32::from_be_bytes(bytes[20..24].try_into().ok()?),
    })
}


pub(crate) fn inspect_gif(bytes: &[u8]) -> Option<ImageInfo> {
    if bytes.len() < 10 {
        return None;
    }
    if &bytes[0..6] != b"GIF87a" && &bytes[0..6] != b"GIF89a" {
        return None;
    }

    Some(ImageInfo {
        format: "GIF",
        mime: "image/gif",
        width: u16::from_le_bytes(bytes[6..8].try_into().ok()?) as u32,
        height: u16::from_le_bytes(bytes[8..10].try_into().ok()?) as u32,
    })
}


pub(crate) fn inspect_jpeg(bytes: &[u8]) -> Option<ImageInfo> {
    if bytes.len() < 4 || bytes[0] != 0xFF || bytes[1] != 0xD8 {
        return None;
    }

    let mut i = 2usize;
    while i + 1 < bytes.len() {
        while i < bytes.len() && bytes[i] != 0xFF {
            i += 1;
        }
        while i < bytes.len() && bytes[i] == 0xFF {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }

        let marker = bytes[i];
        i += 1;

        if marker == 0xD9 || marker == 0xDA {
            break;
        }
        if marker == 0x01 || (0xD0..=0xD7).contains(&marker) {
            continue;
        }
        if i + 2 > bytes.len() {
            break;
        }

        let segment_len = u16::from_be_bytes(bytes[i..i + 2].try_into().ok()?) as usize;
        if segment_len < 2 {
            break;
        }
        let data_start = i + 2;
        let data_end = i + segment_len;
        if data_end > bytes.len() {
            break;
        }

        if is_jpeg_sof_marker(marker) && data_start + 5 <= data_end {
            return Some(ImageInfo {
                format: "JPEG",
                mime: "image/jpeg",
                height: u16::from_be_bytes(bytes[data_start + 1..data_start + 3].try_into().ok()?)
                    as u32,
                width: u16::from_be_bytes(bytes[data_start + 3..data_start + 5].try_into().ok()?)
                    as u32,
            });
        }

        i = data_end;
    }

    None
}


pub(crate) fn is_jpeg_sof_marker(marker: u8) -> bool {
    matches!(
        marker,
        0xC0 | 0xC1 | 0xC2 | 0xC3 | 0xC5 | 0xC6 | 0xC7 | 0xC9 | 0xCA | 0xCB | 0xCD | 0xCE | 0xCF
    )
}


pub(crate) fn inspect_webp(bytes: &[u8]) -> Option<ImageInfo> {
    if bytes.len() < 20 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WEBP" {
        return None;
    }

    let mut i = 12usize;
    while i + 8 <= bytes.len() {
        let fourcc = &bytes[i..i + 4];
        let chunk_size = u32::from_le_bytes(bytes[i + 4..i + 8].try_into().ok()?) as usize;
        let payload = i + 8;
        let end = payload.checked_add(chunk_size)?;
        if end > bytes.len() {
            break;
        }

        if fourcc == b"VP8X" && chunk_size >= 10 {
            return Some(ImageInfo {
                format: "WebP",
                mime: "image/webp",
                width: read_u24_le(&bytes[payload + 4..payload + 7])? + 1,
                height: read_u24_le(&bytes[payload + 7..payload + 10])? + 1,
            });
        }

        if fourcc == b"VP8 "
            && chunk_size >= 10
            && &bytes[payload + 3..payload + 6] == b"\x9D\x01\x2A"
        {
            return Some(ImageInfo {
                format: "WebP",
                mime: "image/webp",
                width: (u16::from_le_bytes(bytes[payload + 6..payload + 8].try_into().ok()?)
                    & 0x3FFF) as u32,
                height: (u16::from_le_bytes(bytes[payload + 8..payload + 10].try_into().ok()?)
                    & 0x3FFF) as u32,
            });
        }

        if fourcc == b"VP8L" && chunk_size >= 5 && bytes[payload] == 0x2F {
            let b1 = bytes[payload + 1] as u32;
            let b2 = bytes[payload + 2] as u32;
            let b3 = bytes[payload + 3] as u32;
            let b4 = bytes[payload + 4] as u32;
            return Some(ImageInfo {
                format: "WebP",
                mime: "image/webp",
                width: 1 + b1 + ((b2 & 0x3F) << 8),
                height: 1 + ((b2 >> 6) | (b3 << 2) | ((b4 & 0x0F) << 10)),
            });
        }

        i = end + (chunk_size % 2);
    }

    None
}


pub(crate) fn read_u24_le(bytes: &[u8]) -> Option<u32> {
    if bytes.len() != 3 {
        return None;
    }
    Some((bytes[0] as u32) | ((bytes[1] as u32) << 8) | ((bytes[2] as u32) << 16))
}


impl ToolExecutor {
    pub(crate) async fn exec_view_image(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        #[derive(Deserialize)]
        struct ViewImageArgs {
            path: String,
        }

        let args: ViewImageArgs = serde_json::from_str(arguments)
            .map_err(|e| crate::error::AppError::Custom(format!("Invalid view_image args: {e}")))?;
        self.emit_tool_start(app_handle, thread_id, call_id, "view_image", &args.path);

        let full_path = match resolve_view_image_path(&self.cwd, &args.path) {
            Ok(path) => path,
            Err(msg) => {
                self.emit_tool_end(app_handle, thread_id, call_id, "view_image", -1, &msg);
                return Ok(msg);
            }
        };

        let bytes = match tokio::fs::read(&full_path).await {
            Ok(bytes) => bytes,
            Err(e) => {
                let msg = format!("Error reading image {}: {e}", args.path);
                self.emit_tool_end(app_handle, thread_id, call_id, "view_image", -1, &msg);
                return Ok(msg);
            }
        };

        let info = match inspect_image_bytes(&bytes) {
            Ok(info) => info,
            Err(msg) => {
                let msg = format!("Error inspecting image {}: {msg}", args.path);
                self.emit_tool_end(app_handle, thread_id, call_id, "view_image", -1, &msg);
                return Ok(msg);
            }
        };

        let absolute_path = full_path.canonicalize().unwrap_or(full_path);
        let output = format!(
            "Viewed image: {}\nAbsolute path: {}\nFormat: {}\nMIME: {}\nDimensions: {}x{}\nSize: {} bytes",
            args.path,
            absolute_path.display(),
            info.format,
            info.mime,
            info.width,
            info.height,
            bytes.len()
        );
        self.emit_tool_end(app_handle, thread_id, call_id, "view_image", 0, &output);
        Ok(output)
    }


    pub(crate) async fn exec_ocr_image(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        #[derive(Deserialize)]
        struct OcrImageArgs {
            path: String,
        }

        let args: OcrImageArgs = match serde_json::from_str(arguments) {
            Ok(args) => args,
            Err(e) => {
                let msg = format!("Invalid ocr_image args: {e}");
                self.emit_tool_start(app_handle, thread_id, call_id, "ocr_image", "invalid");
                self.emit_tool_end(app_handle, thread_id, call_id, "ocr_image", -1, &msg);
                return Ok(msg);
            }
        };
        self.emit_tool_start(app_handle, thread_id, call_id, "ocr_image", &args.path);

        let full_path = match resolve_view_image_path(&self.cwd, &args.path) {
            Ok(path) => path,
            Err(msg) => {
                self.emit_tool_end(app_handle, thread_id, call_id, "ocr_image", -1, &msg);
                return Ok(msg);
            }
        };

        let absolute_path = full_path.canonicalize().unwrap_or(full_path.clone());
        let output = match crate::ocr::extract_text_from_image_file(&self.cwd, &full_path) {
            Ok(text) if !text.trim().is_empty() => format!(
                "OCR image: {}\nAbsolute path: {}\nEngine: PP-OCRv5 mobile (ONNX Runtime)\nText:\n{}",
                args.path,
                absolute_path.display(),
                text.trim()
            ),
            Ok(_) => format!(
                "OCR image: {}\nAbsolute path: {}\nEngine: PP-OCRv5 mobile (ONNX Runtime)\nText:\n<empty>",
                args.path,
                absolute_path.display()
            ),
            Err(err) => {
                let msg = format!("OCR image failed for {}: {}", absolute_path.display(), err);
                self.emit_tool_end(app_handle, thread_id, call_id, "ocr_image", -1, &msg);
                return Ok(msg);
            }
        };

        self.emit_tool_end(app_handle, thread_id, call_id, "ocr_image", 0, &output);
        Ok(output)
    }


    pub(crate) async fn exec_image_generate(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        let args: ImageGenerateArgs = match serde_json::from_str(arguments) {
            Ok(args) => args,
            Err(e) => {
                let msg = format!("Invalid image_generate args: {e}");
                self.emit_tool_start(app_handle, thread_id, call_id, "image_generate", "invalid");
                self.emit_tool_end(app_handle, thread_id, call_id, "image_generate", -1, &msg);
                return Ok(msg);
            }
        };

        let display = image_generate_display(&args);
        self.emit_tool_start(app_handle, thread_id, call_id, "image_generate", &display);

        let prompt = args.prompt.trim();
        if prompt.is_empty() {
            let msg = "image_generate prompt must not be empty".to_string();
            self.emit_tool_end(app_handle, thread_id, call_id, "image_generate", -1, &msg);
            return Ok(msg);
        }

        let image_defaults = self.read_image_generation_settings();
        if !image_defaults.enabled {
            let msg = "image_generate is disabled in Settings > 文生图. Enable the 文生图开关 to use this tool.".to_string();
            self.emit_tool_end(app_handle, thread_id, call_id, "image_generate", -1, &msg);
            return Ok(msg);
        }
        let api_key = match image_generation_api_key(image_defaults.api_key.as_deref()) {
            Some(value) => value,
            None => {
                let msg = "image_generate requires an API key. Configure it in Settings > 文生图, or set CN_CODEX_IMAGE_API_KEY / OPENAI_API_KEY.".to_string();
                self.emit_tool_end(app_handle, thread_id, call_id, "image_generate", -1, &msg);
                return Ok(msg);
            }
        };

        let model = image_generation_model(args.model.as_deref(), image_defaults.model.as_deref());
        let api_url =
            image_generation_api_url(args.base_url.as_deref(), image_defaults.base_url.as_deref());
        let body = image_generation_request_body(
            prompt,
            &model,
            args.size.as_deref(),
            args.quality.as_deref(),
            args.background.as_deref(),
            args.n,
        );

        let response = match self
            .http
            .post(&api_url)
            .bearer_auth(api_key)
            .json(&body)
            .send()
            .await
        {
            Ok(response) => response,
            Err(e) => {
                let msg = format!("image_generate request failed: {e}");
                self.emit_tool_end(app_handle, thread_id, call_id, "image_generate", -1, &msg);
                return Ok(msg);
            }
        };

        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            let msg = format_image_generation_http_error(status.as_u16(), &text);
            self.emit_tool_end(app_handle, thread_id, call_id, "image_generate", -1, &msg);
            return Ok(msg);
        }

        let parsed: ImageGenerationResponse = match response.json().await {
            Ok(parsed) => parsed,
            Err(e) => {
                let msg = format!("image_generate returned invalid JSON: {e}");
                self.emit_tool_end(app_handle, thread_id, call_id, "image_generate", -1, &msg);
                return Ok(msg);
            }
        };

        let items = parsed.data;
        if items.is_empty() {
            let msg = "image_generate response did not contain any images".to_string();
            self.emit_tool_end(app_handle, thread_id, call_id, "image_generate", -1, &msg);
            return Ok(msg);
        }

        let image_count = items.len();
        let mut saved_images = Vec::with_capacity(image_count);

        for (index, item) in items.into_iter().enumerate() {
            let bytes = if let Some(b64) = item
                .b64_json
                .as_deref()
                .filter(|value| !value.trim().is_empty())
            {
                match decode_image_base64(b64) {
                    Ok(bytes) => bytes,
                    Err(msg) => {
                        self.emit_tool_end(
                            app_handle,
                            thread_id,
                            call_id,
                            "image_generate",
                            -1,
                            &msg,
                        );
                        return Ok(msg);
                    }
                }
            } else if let Some(url) = item.url.as_deref().filter(|value| !value.trim().is_empty()) {
                match self.fetch_generated_image_url(url, &api_url).await {
                    Ok(bytes) => bytes,
                    Err(msg) => {
                        self.emit_tool_end(
                            app_handle,
                            thread_id,
                            call_id,
                            "image_generate",
                            -1,
                            &msg,
                        );
                        return Ok(msg);
                    }
                }
            } else {
                let msg = format!(
                    "image_generate response item {} did not include b64_json or url",
                    index + 1
                );
                self.emit_tool_end(app_handle, thread_id, call_id, "image_generate", -1, &msg);
                return Ok(msg);
            };

            let info = match inspect_image_bytes(&bytes) {
                Ok(info) => info,
                Err(msg) => {
                    let msg = format!("image_generate returned an invalid image: {msg}");
                    self.emit_tool_end(app_handle, thread_id, call_id, "image_generate", -1, &msg);
                    return Ok(msg);
                }
            };

            let extension = image_extension_for_info(&info);
            let output_path = match resolve_image_generate_output_path_for_index(
                &self.cwd,
                &self.workspace_config_dir,
                args.output_path.as_deref(),
                call_id,
                prompt,
                extension,
                index,
                image_count,
            ) {
                Ok(path) => path,
                Err(msg) => {
                    self.emit_tool_end(app_handle, thread_id, call_id, "image_generate", -1, &msg);
                    return Ok(msg);
                }
            };

            if let Some(parent) = output_path.parent() {
                if let Err(e) = tokio::fs::create_dir_all(parent).await {
                    let msg = format!(
                        "Error creating image output directory {}: {e}",
                        parent.display()
                    );
                    self.emit_tool_end(app_handle, thread_id, call_id, "image_generate", -1, &msg);
                    return Ok(msg);
                }
            }

            if let Err(e) = tokio::fs::write(&output_path, &bytes).await {
                let msg = format!(
                    "Error writing generated image {}: {e}",
                    output_path.display()
                );
                self.emit_tool_end(app_handle, thread_id, call_id, "image_generate", -1, &msg);
                return Ok(msg);
            }

            let absolute_path = output_path.canonicalize().unwrap_or(output_path.clone());
            let display_path = workspace_relative_display_path(&self.cwd, &absolute_path)
                .unwrap_or_else(|| absolute_path.to_string_lossy().to_string());
            let revised_prompt = item
                .revised_prompt
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned);
            saved_images.push(GeneratedImageOutput {
                index: index + 1,
                output_path: display_path,
                absolute_path: absolute_path.to_string_lossy().to_string(),
                format: info.format.to_string(),
                mime: info.mime.to_string(),
                dimensions: format!("{}x{}", info.width, info.height),
                width: info.width,
                height: info.height,
                size: format!("{} bytes", bytes.len()),
                size_bytes: bytes.len(),
                revised_prompt,
            });
        }

        let first = saved_images.first().expect("checked non-empty image data");
        let mut output = format!(
            "Generated {}\nPrompt: {}\nModel: {}\nOutput path: {}\nAbsolute path: {}\nFormat: {}\nMIME: {}\nDimensions: {}\nSize: {}",
            if saved_images.len() == 1 {
                "image".to_string()
            } else {
                format!("{} images", saved_images.len())
            },
            prompt,
            model,
            first.output_path,
            first.absolute_path,
            first.format,
            first.mime,
            first.dimensions,
            first.size
        );
        if let Some(revised_prompt) = first.revised_prompt.as_deref() {
            output.push_str("\nRevised prompt: ");
            output.push_str(revised_prompt);
        }
        output.push_str("\nImages JSON:\n");
        output.push_str(
            &serde_json::to_string_pretty(&serde_json::json!({ "images": saved_images }))
                .unwrap_or_else(|_| "{\"images\":[]}".to_string()),
        );

        self.emit_tool_end(app_handle, thread_id, call_id, "image_generate", 0, &output);
        Ok(output)
    }


    pub(crate) async fn fetch_generated_image_url(
        &self,
        url: &str,
        reference_endpoint: &str,
    ) -> Result<Vec<u8>, String> {
        let resolved_url = Self::resolve_generated_image_url(url, reference_endpoint)?;
        let response = self.http.get(&resolved_url).send().await.map_err(|e| {
            format!("image_generate could not fetch generated image URL {resolved_url}: {e}")
        })?;
        let status = response.status();
        if !status.is_success() {
            return Err(format!(
                "image_generate image URL fetch failed with HTTP {} ({resolved_url})",
                status.as_u16(),
            ));
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|e| format!("image_generate could not read generated image bytes: {e}"))?;
        Ok(bytes.to_vec())
    }


    pub(crate) fn resolve_generated_image_url(url: &str, reference_endpoint: &str) -> Result<String, String> {
        let trimmed = url.trim();
        if trimmed.is_empty() {
            return Err("image_generate response contained an empty image URL".to_string());
        }

        if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
            return Ok(trimmed.to_string());
        }

        let base = reqwest::Url::parse(reference_endpoint).map_err(|e| {
            format!(
                "image_generate could not parse API endpoint '{reference_endpoint}' while resolving image URL '{trimmed}': {e}"
            )
        })?;

        base.join(trimmed).map(|value| value.to_string()).map_err(|e| {
            format!(
                "image_generate returned relative image URL '{trimmed}' that could not be resolved from '{reference_endpoint}': {e}"
            )
        })
    }


    pub(crate) fn image_generation_is_enabled(&self) -> bool {
        self.read_image_generation_settings().enabled
    }


    pub(crate) fn read_image_generation_settings(&self) -> ImageGenerationSettingsResolved {
        let config_path = self.workspace_config_dir.join("config.toml");
        let config = match ConfigToml::load(&config_path) {
            Ok(config) => config,
            Err(_) => return ImageGenerationSettingsResolved::default(),
        };
        let image_generation = config.image_generation_config();
        ImageGenerationSettingsResolved {
            enabled: image_generation.is_enabled(),
            model: image_generation.model.and_then(non_empty_string),
            base_url: image_generation.base_url.and_then(non_empty_string),
            api_key: image_generation.api_key.and_then(non_empty_string),
        }
    }

}
