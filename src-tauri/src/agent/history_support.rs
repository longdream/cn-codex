use super::*;

pub(crate) fn multimodal_user_content(text: &str, attachments: &[UserAttachment]) -> serde_json::Value {
    let mut combined_text = text.to_string();
    let mut image_parts: Vec<serde_json::Value> = Vec::new();

    for attachment in attachments {
        if attachment.mime_type.starts_with("image/") && attachment.data_url.starts_with("data:") {
            image_parts.push(serde_json::json!({
                "type": "image_url",
                "image_url": {
                    "url": attachment.data_url,
                    "detail": "high"
                }
            }));
        } else {
            match crate::document_parser::parse_document(
                &attachment.mime_type,
                &attachment.data_url,
            ) {
                Ok(extracted) => {
                    combined_text.push_str(&format!(
                        "\n\n[Attachment: {}]\n{}",
                        attachment.name, extracted
                    ));
                }
                Err(e) => {
                    warn!("Document parse failed for {}: {e}", attachment.name);
                    combined_text.push_str(&format!(
                        "\n\n[Attachment: {} ({}, {} bytes) - content extraction failed]",
                        attachment.name, attachment.mime_type, attachment.size
                    ));
                }
            }
        }
    }

    // 只有当存在图片时才使用 multimodal 数组格式，否则用纯文本（兼容不支持 multimodal 的 API）
    if image_parts.is_empty() {
        serde_json::Value::String(combined_text)
    } else {
        let mut parts = vec![serde_json::json!({
            "type": "text",
            "text": combined_text
        })];
        parts.extend(image_parts);
        serde_json::Value::Array(parts)
    }
}


pub(crate) fn normalize_tool_call_requests(calls: Vec<ToolCallRequest>) -> Vec<ToolCallRequest> {
    calls
        .into_iter()
        .enumerate()
        .filter_map(|(index, call)| {
            let name = call.name.trim().to_string();
            if name.is_empty() {
                return None;
            }

            let id = if call.id.trim().is_empty() {
                format!("call_{}_{}", sanitize_tool_name(&name), index)
            } else {
                call.id
            };

            Some(ToolCallRequest {
                id,
                name,
                arguments: call.arguments,
            })
        })
        .collect()
}


pub(crate) fn sanitize_tool_name(name: &str) -> String {
    let sanitized: String = name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = sanitized.trim_matches('_');
    if trimmed.is_empty() {
        "tool".to_string()
    } else {
        trimmed.to_string()
    }
}


pub(crate) fn reorder_history_system_messages_for_model<'a>(
    history: &'a [ThreadMessage],
) -> Vec<&'a ThreadMessage> {
    let mut system_messages = Vec::new();
    let mut non_system_messages = Vec::new();
    for msg in history {
        if msg.role == "system" {
            system_messages.push(msg);
        } else {
            non_system_messages.push(msg);
        }
    }
    system_messages.extend(non_system_messages);
    system_messages
}


pub(crate) fn uniquify_tool_call_ids(
    calls: Vec<ToolCallRequest>,
    issued_ids: &mut HashSet<String>,
) -> Vec<ToolCallRequest> {
    calls
        .into_iter()
        .map(|mut call| {
            let original = call.id.trim();
            let base = if original.is_empty() { "call" } else { original };
            if issued_ids.insert(base.to_string()) {
                call.id = base.to_string();
                return call;
            }

            let mut suffix = 2_usize;
            loop {
                let candidate = format!("{base}__{suffix}");
                if issued_ids.insert(candidate.clone()) {
                    warn!(
                        "Provider reused tool call ID `{base}`; persisted it as `{candidate}` to preserve its result"
                    );
                    call.id = candidate;
                    return call;
                }
                suffix = suffix.saturating_add(1);
            }
        })
        .collect()
}


pub(crate) fn sanitize_history_for_model(history: &[ThreadMessage]) -> Vec<ThreadMessage> {
    let mut seen_tool_call_ids: HashSet<String> = HashSet::new();
    let mut pending_result_ids: BTreeMap<String, VecDeque<String>> = BTreeMap::new();
    let mut sanitized = Vec::with_capacity(history.len());

    for msg in history {
        let filtered_tool_calls = msg.tool_calls.as_ref().map(|tool_calls| {
            tool_calls
                .iter()
                .filter_map(|call| {
                    let original_id = call.id.trim();
                    if original_id.is_empty() {
                        return None;
                    }
                    let unique_id =
                        unique_history_tool_call_id(original_id, &mut seen_tool_call_ids);
                    pending_result_ids
                        .entry(original_id.to_string())
                        .or_default()
                        .push_back(unique_id.clone());
                    let mut sanitized_call = call.clone();
                    sanitized_call.id = unique_id;
                    Some(sanitized_call)
                })
                .collect::<Vec<_>>()
        });

        if msg.role == "assistant"
            && msg.content.trim().is_empty()
            && filtered_tool_calls
                .as_ref()
                .is_some_and(|tool_calls| tool_calls.is_empty())
        {
            continue;
        }

        if msg.role == "tool" {
            let Some(tool_call_id) = msg
                .tool_call_id
                .as_ref()
                .map(|id| id.trim())
                .filter(|id| !id.is_empty())
            else {
                continue;
            };

            let Some(remapped_id) = pending_result_ids
                .get_mut(tool_call_id)
                .and_then(VecDeque::pop_front)
            else {
                continue;
            };

            let mut sanitized_msg = msg.clone();
            sanitized_msg.tool_call_id = Some(remapped_id);
            sanitized_msg.tool_calls = None;
            sanitized.push(sanitized_msg);
            continue;
        }

        let mut sanitized_msg = msg.clone();
        sanitized_msg.tool_calls = filtered_tool_calls.filter(|tool_calls| !tool_calls.is_empty());
        sanitized.push(sanitized_msg);
    }

    let recorded_result_ids: HashSet<String> = sanitized
        .iter()
        .filter(|message| message.role == "tool")
        .filter_map(|message| message.tool_call_id.clone())
        .collect();

    // A crash or cancellation can persist the assistant tool call before its
    // result. Both Responses and Chat APIs reject that dangling pair on the
    // next request, so add a stable prompt-only aborted result.
    let mut index = 0_usize;
    while index < sanitized.len() {
        let missing_calls = sanitized[index]
            .tool_calls
            .as_ref()
            .map(|calls| {
                calls
                    .iter()
                    .filter(|call| !recorded_result_ids.contains(&call.id))
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if missing_calls.is_empty() {
            index += 1;
            continue;
        }

        let timestamp = sanitized[index].timestamp;
        let mut insert_at = index + 1;
        while insert_at < sanitized.len() && sanitized[insert_at].role == "tool" {
            insert_at += 1;
        }
        let synthetic_results = missing_calls.into_iter().map(|call| ThreadMessage {
            id: format!("synthetic-tool-result-{}", call.id),
            role: "tool".to_string(),
            content: "Tool execution aborted before a result was recorded.".to_string(),
            timestamp,
            tool_call_id: Some(call.id),
            tool_name: Some(call.name),
            tool_calls: None,
            attachments: Vec::new(),
        });
        let inserted = synthetic_results.len();
        sanitized.splice(insert_at..insert_at, synthetic_results);
        index = insert_at + inserted;
    }

    sanitized
}


pub(crate) fn unique_history_tool_call_id(original_id: &str, seen_ids: &mut HashSet<String>) -> String {
    if seen_ids.insert(original_id.to_string()) {
        return original_id.to_string();
    }

    let mut suffix = 2_usize;
    loop {
        let candidate = format!("{original_id}__{suffix}");
        if seen_ids.insert(candidate.clone()) {
            return candidate;
        }
        suffix = suffix.saturating_add(1);
    }
}


