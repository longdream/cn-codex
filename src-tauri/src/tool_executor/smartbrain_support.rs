use super::*;

impl ToolExecutor {
    pub(crate) async fn exec_smartbrain_search(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        #[derive(Deserialize)]
        struct Args {
            query: String,
            #[serde(default)]
            top_k: Option<usize>,
            #[serde(default)]
            domain: Option<String>,
            #[serde(default)]
            concept_type: Option<String>,
            #[serde(default)]
            tags: Option<Vec<String>>,
            #[serde(default)]
            source_type: Option<String>,
            #[serde(default)]
            source_group: Option<String>,
            #[serde(default)]
            relative_path_prefix: Option<String>,
            #[serde(default)]
            source_file: Option<String>,
        }

        let args: Args = serde_json::from_str(arguments).map_err(|e| {
            crate::error::AppError::Custom(format!("Invalid smartbrain_search args: {e}"))
        })?;
        let query = args.query.trim();
        self.emit_tool_start(app_handle, thread_id, call_id, "smartbrain_search", query);

        if query.is_empty() {
            let msg = "Error: empty search query".to_string();
            self.emit_tool_end(
                app_handle,
                thread_id,
                call_id,
                "smartbrain_search",
                -1,
                &msg,
            );
            return Ok(msg);
        }

        if !self.smartbrain_is_active() {
            let msg =
                "Local Knowledge Base is disabled for this chat. Enable it in the composer (本地知识库) to use smartbrain_search."
                    .to_string();
            self.emit_tool_end(
                app_handle,
                thread_id,
                call_id,
                "smartbrain_search",
                -1,
                &msg,
            );
            return Ok(msg);
        }

        let top_k = args.top_k.unwrap_or(5).clamp(1, 20);
        let bm25_path = crate::smartbrain::bm25_index_path(&self.workspace_config_dir);
        let smartbrain_policy = ConfigToml::load(&self.workspace_config_dir.join("config.toml"))
            .map(|config| config.smartbrain_config())
            .unwrap_or_default();
        if !bm25_path.exists() {
            let msg = "Local Knowledge Base index is not ready yet. Upload knowledge files or run `smartbrain_rebuild_index`, then retry `smartbrain_search`.".to_string();
            self.emit_tool_end(
                app_handle,
                thread_id,
                call_id,
                "smartbrain_search",
                -1,
                &msg,
            );
            return Ok(msg);
        }
        let bm25 = crate::smartbrain::bm25_index::BM25Index::load(&bm25_path);
        if bm25.document_count() == 0 {
            let msg = "Local Knowledge Base index is empty. Upload knowledge content first, then run `smartbrain_rebuild_index` if needed.".to_string();
            self.emit_tool_end(
                app_handle,
                thread_id,
                call_id,
                "smartbrain_search",
                -1,
                &msg,
            );
            return Ok(msg);
        }
        let domain = args
            .domain
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
        let concept_type = args
            .concept_type
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
        let tags = args
            .tags
            .unwrap_or_default()
            .into_iter()
            .map(|tag| tag.trim().to_string())
            .filter(|tag| !tag.is_empty())
            .collect::<Vec<_>>();
        let source_group = args
            .source_group
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
        let relative_path_prefix = args
            .relative_path_prefix
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
        let source_file = args
            .source_file
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
        let source_type = args
            .source_type
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| match value.to_ascii_lowercase().as_str() {
                "experience" => crate::smartbrain::bm25_index::SourceType::Experience,
                _ => crate::smartbrain::bm25_index::SourceType::Knowledge,
            });
        let has_filter = concept_type.is_some()
            || !tags.is_empty()
            || source_type.is_some()
            || domain.is_some()
            || source_group.is_some()
            || relative_path_prefix.is_some()
            || source_file.is_some();
        let results = if has_filter {
            crate::smartbrain::search::unified_search_with_filter_with_policy(
                &bm25_path,
                query,
                top_k,
                crate::smartbrain::bm25_index::SearchFilter {
                    concept_type,
                    tags,
                    domain,
                    source_group,
                    relative_path_prefix,
                    source_file,
                    source_type,
                    timestamp_after: None,
                    timestamp_before: None,
                },
                smartbrain_policy.search_okf_prefilter_enabled,
                Some(&smartbrain_policy.search_okf_prefilter_order),
            )
        } else {
            crate::smartbrain::search::unified_search(&bm25_path, query, top_k)
        };

        let output = if results.is_empty() {
            "No matching documents found in Local Knowledge Base.".to_string()
        } else {
            let mut lines = Vec::new();
            let mut context_blocks_added = 0usize;
            lines.push(format!(
                "Found {} results for \"{}\":\n",
                results.len(),
                query
            ));
            for (i, r) in results.iter().enumerate() {
                lines.push(format!(
                    "{}. [{}] {} (score: {:.3})\n   Path: {}\n   Use `memory_read` with path \"{}\" and small windows (for example `line_offset: 1`, `max_lines: 120`) to read relevant sections. For chunk hits, context bridge below keeps prev/current/next continuity with >=30-line overlap.",
                    i + 1, r.source_type, r.title, r.score, r.file_path, r.file_path
                ));
                if let Some(result_domain) = r.domain.as_deref().filter(|value| !value.is_empty()) {
                    lines.push(format!("   Domain: {result_domain}"));
                }
                if let Some(result_group) =
                    r.source_group.as_deref().filter(|value| !value.is_empty())
                {
                    lines.push(format!("   Source Group: {result_group}"));
                }
                if let Some(relative_path) =
                    r.relative_path.as_deref().filter(|value| !value.is_empty())
                {
                    lines.push(format!("   Relative Path: {relative_path}"));
                }
                if let Some(source_file) =
                    r.source_file.as_deref().filter(|value| !value.is_empty())
                {
                    lines.push(format!("   Source File: {source_file}"));
                }
                if r.is_chunk {
                    let chunk_index = r.chunk_index.unwrap_or(1);
                    let chunk_total = r.chunk_total.unwrap_or(chunk_index);
                    lines.push(format!("   Chunk: {chunk_index}/{chunk_total}"));
                    if let Some(parent_doc_id) =
                        r.parent_doc_id.as_deref().filter(|value| !value.is_empty())
                    {
                        lines.push(format!("   Parent Doc: {parent_doc_id}"));
                    }
                    if context_blocks_added < SMARTBRAIN_CONTEXT_RESULT_LIMIT {
                        if let Some(context_block) =
                            self.render_chunk_context_bridge(r, SMARTBRAIN_CONTEXT_OVERLAP_LINES)
                        {
                            lines.push(context_block);
                            context_blocks_added += 1;
                        }
                    }
                }
            }
            lines.join("\n")
        };

        let output = truncate_output(&output, TOOL_OUTPUT_SMARTBRAIN_MAX_CHARS);
        self.emit_tool_end(
            app_handle,
            thread_id,
            call_id,
            "smartbrain_search",
            0,
            &output,
        );
        Ok(output)
    }


    pub(crate) async fn exec_smartbrain_sql_query(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        #[derive(Deserialize)]
        struct Args {
            #[serde(default)]
            database: Option<String>,
            sql: String,
            #[serde(default)]
            row_limit: Option<usize>,
            #[serde(default)]
            timeout_sec: Option<u64>,
        }

        let args: Args = serde_json::from_str(arguments).map_err(|e| {
            crate::error::AppError::Custom(format!("Invalid smartbrain_sql_query args: {e}"))
        })?;
        let sql = args.sql.trim();
        let database = args
            .database
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let display = match database {
            Some(name) => format!("{name} :: {}", truncate_output(sql, 120)),
            None => truncate_output(sql, 160),
        };
        self.emit_tool_start(
            app_handle,
            thread_id,
            call_id,
            "smartbrain_sql_query",
            &display,
        );

        if !self.smartbrain_is_active() {
            let msg = "Local Knowledge Base is disabled for this chat. Enable it in the composer (本地知识库) to use smartbrain_sql_query."
                .to_string();
            self.emit_tool_end(
                app_handle,
                thread_id,
                call_id,
                "smartbrain_sql_query",
                -1,
                &msg,
            );
            return Ok(msg);
        }
        if sql.is_empty() {
            let msg = "Error: empty SQL".to_string();
            self.emit_tool_end(
                app_handle,
                thread_id,
                call_id,
                "smartbrain_sql_query",
                -1,
                &msg,
            );
            return Ok(msg);
        }

        match crate::smartbrain::db_query::execute_smartbrain_sql_query(
            &self.workspace_config_dir,
            database,
            sql,
            args.row_limit,
            args.timeout_sec,
        )
        .await
        {
            Ok(result) => {
                let output = truncate_output(
                    &crate::smartbrain::db_query::format_sql_query_result(&result),
                    24_000,
                );
                self.emit_tool_end(
                    app_handle,
                    thread_id,
                    call_id,
                    "smartbrain_sql_query",
                    0,
                    &output,
                );
                Ok(output)
            }
            Err(error) => {
                let msg = format!("smartbrain_sql_query failed: {error}");
                self.emit_tool_end(
                    app_handle,
                    thread_id,
                    call_id,
                    "smartbrain_sql_query",
                    -1,
                    &msg,
                );
                Ok(msg)
            }
        }
    }


    pub(crate) fn render_chunk_context_bridge(
        &self,
        result: &crate::smartbrain::search::SmartBrainSearchResult,
        overlap_lines: usize,
    ) -> Option<String> {
        if !result.is_chunk {
            return None;
        }
        let parent_doc_id = result.parent_doc_id.as_deref()?.trim();
        if parent_doc_id.is_empty() {
            return None;
        }
        let chunk_index = result.chunk_index?;
        let chunk_total = result.chunk_total.unwrap_or(chunk_index).max(chunk_index);
        let overlap_lines = overlap_lines.max(SMARTBRAIN_CONTEXT_OVERLAP_LINES);

        let current_path = self.resolve_memory_path(&result.file_path).ok()?;
        let current_lines = read_okf_body_lines(&current_path)?;
        if current_lines.is_empty() {
            return None;
        }

        let docs_dir = self.memories_dir().join("knowledge").join("docs");
        let previous_lines = if chunk_index > 1 {
            let previous_file =
                crate::smartbrain::knowledge::chunk_file_name(parent_doc_id, chunk_index - 1);
            read_okf_body_lines(&docs_dir.join(previous_file))
        } else {
            None
        };
        let next_lines = if chunk_index < chunk_total {
            let next_file =
                crate::smartbrain::knowledge::chunk_file_name(parent_doc_id, chunk_index + 1);
            read_okf_body_lines(&docs_dir.join(next_file))
        } else {
            None
        };

        if previous_lines.is_none() && next_lines.is_none() {
            return None;
        }

        let mut lines = vec![format!(
            "   Context Bridge (上下文补齐, overlap >= {overlap_lines} lines):"
        )];
        if let Some(prev) = previous_lines {
            lines.push(format!(
                "   - Prev chunk {}/{} tail + current head overlap:",
                chunk_index.saturating_sub(1),
                chunk_total
            ));
            for line in take_last_lines(&prev, overlap_lines) {
                lines.push(format!("     {line}"));
            }
            for line in take_first_lines(&current_lines, overlap_lines) {
                lines.push(format!("     {line}"));
            }
        }
        lines.push(format!("   - Current chunk {chunk_index}/{chunk_total}:"));
        for line in &current_lines {
            lines.push(format!("     {line}"));
        }
        if let Some(next) = next_lines {
            lines.push(format!(
                "   - Current tail overlap + next chunk {}/{} head:",
                chunk_index + 1,
                chunk_total
            ));
            for line in take_last_lines(&current_lines, overlap_lines) {
                lines.push(format!("     {line}"));
            }
            for line in take_first_lines(&next, overlap_lines) {
                lines.push(format!("     {line}"));
            }
        }
        Some(lines.join("\n"))
    }


    pub(crate) fn smartbrain_is_active(&self) -> bool {
        if let Some(enabled) = self.smartbrain_enabled_override {
            return enabled;
        }
        let config_path = self.workspace_config_dir.join("config.toml");
        ConfigToml::load(&config_path)
            .map(|config| config.smartbrain_config().knowledge_is_active())
            .unwrap_or(false)
    }

}
