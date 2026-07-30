use super::*;

pub(crate) fn web_search_provider_order() -> [WebSearchProvider; 3] {
    [
        WebSearchProvider::BingBrowser,
        WebSearchProvider::DuckDuckGoApi,
        WebSearchProvider::DuckDuckGoBrowser,
    ]
}


pub(crate) fn web_search_output_has_results(output: &str) -> bool {
    !output.starts_with("No web search results found")
}


pub(crate) fn take_first_lines(lines: &[String], count: usize) -> Vec<String> {
    lines.iter().take(count).cloned().collect()
}


pub(crate) fn take_last_lines(lines: &[String], count: usize) -> Vec<String> {
    if lines.len() <= count {
        return lines.to_vec();
    }
    lines[lines.len() - count..].to_vec()
}


pub(crate) fn format_duckduckgo_results(
    query: &str,
    response: DuckDuckGoResponse,
    max_results: usize,
) -> String {
    let mut seen_urls = BTreeSet::new();
    let mut results = Vec::new();

    if !response.abstract_text.trim().is_empty() {
        let title = if response.heading.trim().is_empty() {
            query.to_string()
        } else {
            response.heading.trim().to_string()
        };
        if !response.abstract_url.trim().is_empty() {
            seen_urls.insert(response.abstract_url.clone());
        }
        results.push(WebSearchResult {
            title,
            url: response.abstract_url,
            snippet: response.abstract_text,
        });
    }

    collect_duckduckgo_topics(
        &response.related_topics,
        max_results,
        &mut seen_urls,
        &mut results,
    );

    if results.is_empty() {
        return format!("No web search results found for: {query}");
    }

    let mut output = format!("Web search results for \"{query}\":\n");
    for (idx, result) in results.into_iter().take(max_results).enumerate() {
        output.push_str(&format!("\n{}. {}", idx + 1, result.title));
        if !result.url.trim().is_empty() {
            output.push_str(&format!("\n   URL: {}", result.url.trim()));
        }
        if !result.snippet.trim().is_empty() {
            output.push_str(&format!(
                "\n   Snippet: {}",
                truncate_output(&condense_whitespace(&result.snippet), 600)
            ));
        }
        output.push('\n');
    }

    output
}


pub(crate) fn collect_duckduckgo_topics(
    topics: &[DuckDuckGoTopic],
    max_results: usize,
    seen_urls: &mut BTreeSet<String>,
    results: &mut Vec<WebSearchResult>,
) {
    for topic in topics {
        if results.len() >= max_results {
            return;
        }

        if !topic.topics.is_empty() {
            collect_duckduckgo_topics(&topic.topics, max_results, seen_urls, results);
            continue;
        }

        let text = topic.text.trim();
        let url = topic.first_url.trim();
        if text.is_empty() || url.is_empty() || !seen_urls.insert(url.to_string()) {
            continue;
        }

        let (title, snippet) = split_search_text(text);
        results.push(WebSearchResult {
            title,
            url: url.to_string(),
            snippet,
        });
    }
}


pub(crate) fn split_search_text(text: &str) -> (String, String) {
    if let Some((title, snippet)) = text.split_once(" - ") {
        (title.trim().to_string(), snippet.trim().to_string())
    } else {
        (text.trim().to_string(), String::new())
    }
}


pub(crate) fn parse_browser_search_results(
    browser_output: &str,
    max_results: usize,
) -> Option<Vec<WebSearchResult>> {
    let parsed: serde_json::Value = extract_browser_output_json(browser_output)?;
    let actions = parsed.get("actions").and_then(|v| v.as_array())?;

    for action in actions {
        if action.get("type").and_then(|v| v.as_str()) != Some("eval") {
            continue;
        }
        let value_str = action.get("value").and_then(|v| v.as_str()).or_else(|| {
            action
                .get("value")
                .and_then(|v| serde_json::to_string(v).ok())
                .as_deref()
                .map(|_| "")
        })?;

        let items: Vec<serde_json::Value> = serde_json::from_str(value_str).ok()?;
        let mut results = Vec::new();
        for item in items.into_iter().take(max_results) {
            let title = item
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            let url = item
                .get("url")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            let snippet = item
                .get("snippet")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            if !title.is_empty() {
                results.push(WebSearchResult {
                    title,
                    url,
                    snippet,
                });
            }
        }
        if !results.is_empty() {
            return Some(results);
        }
    }
    None
}


pub(crate) fn extract_browser_output_json(browser_output: &str) -> Option<serde_json::Value> {
    if let Ok(parsed) = serde_json::from_str(browser_output) {
        return Some(parsed);
    }

    let start = browser_output.find('{')?;
    let end = browser_output.rfind('}')?;
    if end <= start {
        return None;
    }

    serde_json::from_str(&browser_output[start..=end]).ok()
}


pub(crate) fn format_browser_search_results(query: &str, results: &[WebSearchResult]) -> String {
    let mut output = format!("Web search results for \"{query}\" (via browser):\n");
    for (idx, result) in results.iter().enumerate() {
        output.push_str(&format!("\n{}. {}", idx + 1, result.title));
        if !result.url.is_empty() {
            output.push_str(&format!("\n   URL: {}", result.url));
        }
        if !result.snippet.is_empty() {
            output.push_str(&format!(
                "\n   Snippet: {}",
                truncate_output(&condense_whitespace(&result.snippet), 600)
            ));
        }
        output.push('\n');
    }
    output
}


pub(crate) fn extract_html_title(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let open = lower.find("<title")?;
    let after_open = &html[open..];
    let tag_end = after_open.find('>')?;
    let content_start = open + tag_end + 1;
    let close_rel = lower[content_start..].find("</title>")?;
    let raw_title = &html[content_start..content_start + close_rel];
    let title = condense_whitespace(&decode_html_entities(raw_title));
    (!title.is_empty()).then_some(title)
}


pub(crate) fn html_to_text(html: &str) -> String {
    let without_scripts = strip_block_tag(html, "script");
    let without_styles = strip_block_tag(&without_scripts, "style");
    let without_svg = strip_block_tag(&without_styles, "svg");

    let mut output = String::new();
    let mut in_tag = false;
    let mut last_space = false;

    for ch in without_svg.chars() {
        match ch {
            '<' => {
                in_tag = true;
                push_single_space(&mut output, &mut last_space);
            }
            '>' => {
                in_tag = false;
                push_single_space(&mut output, &mut last_space);
            }
            _ if in_tag => {}
            _ if ch.is_whitespace() => push_single_space(&mut output, &mut last_space),
            _ => {
                output.push(ch);
                last_space = false;
            }
        }
    }

    condense_whitespace(&decode_html_entities(&output))
}


pub(crate) fn strip_block_tag(input: &str, tag: &str) -> String {
    let open_pattern = format!("<{tag}");
    let close_pattern = format!("</{tag}>");
    let mut rest = input;
    let mut output = String::new();

    loop {
        let lower = rest.to_ascii_lowercase();
        let Some(start) = lower.find(&open_pattern) else {
            output.push_str(rest);
            break;
        };

        output.push_str(&rest[..start]);
        let after_start = &rest[start..];
        let after_lower = after_start.to_ascii_lowercase();
        let Some(end_rel) = after_lower.find(&close_pattern) else {
            break;
        };

        rest = &after_start[end_rel + close_pattern.len()..];
    }

    output
}


pub(crate) fn push_single_space(output: &mut String, last_space: &mut bool) {
    if !output.is_empty() && !*last_space {
        output.push(' ');
        *last_space = true;
    }
}


pub(crate) fn decode_html_entities(input: &str) -> String {
    input
        .replace("&nbsp;", " ")
        .replace("&#160;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
}


pub(crate) fn condense_whitespace(input: &str) -> String {
    input.split_whitespace().collect::<Vec<_>>().join(" ")
}


pub(crate) fn encode_query_component(input: &str) -> String {
    let mut output = String::new();
    for byte in input.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                output.push(*byte as char)
            }
            _ => output.push_str(&format!("%{byte:02X}")),
        }
    }
    output
}


impl ToolExecutor {
    pub(crate) async fn exec_web_search(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        #[derive(Deserialize)]
        struct SearchArgs {
            query: String,
            #[serde(default)]
            max_results: Option<usize>,
        }

        let args: SearchArgs = serde_json::from_str(arguments)
            .map_err(|e| crate::error::AppError::Custom(format!("Invalid web_search args: {e}")))?;
        let query = args.query.trim();
        let max_results = args.max_results.unwrap_or(5).clamp(1, 10);

        self.emit_tool_start(app_handle, thread_id, call_id, "web_search", query);

        if query.is_empty() {
            let msg = "Error: empty search query".to_string();
            self.emit_tool_end(app_handle, thread_id, call_id, "web_search", -1, &msg);
            return Ok(msg);
        }

        info!("Searching web: {query}");
        let mut fallback_output =
            format!("No web search results found for: {query} (tried Bing and DuckDuckGo)");

        for provider in web_search_provider_order() {
            match provider {
                WebSearchProvider::BingBrowser => {
                    if let Some(output) = self
                        .search_bing_browser(query, max_results, call_id, app_handle, thread_id)
                        .await
                    {
                        self.emit_tool_end(
                            app_handle,
                            thread_id,
                            call_id,
                            "web_search",
                            0,
                            &output,
                        );
                        return Ok(output);
                    }
                    info!("Bing browser returned no results, falling back to DuckDuckGo");
                }
                WebSearchProvider::DuckDuckGoApi => {
                    let output = match self.search_duckduckgo_api(query, max_results).await {
                        Ok(output) => output,
                        Err(err) => {
                            warn!("DuckDuckGo API search failed, trying browser fallback: {err}");
                            continue;
                        }
                    };
                    if web_search_output_has_results(&output) {
                        self.emit_tool_end(
                            app_handle,
                            thread_id,
                            call_id,
                            "web_search",
                            0,
                            &output,
                        );
                        return Ok(output);
                    }
                    info!("DuckDuckGo API returned no results, trying browser fallback");
                }
                WebSearchProvider::DuckDuckGoBrowser => {
                    let output = self
                        .fallback_browser_search(query, max_results, call_id, app_handle, thread_id)
                        .await;
                    if web_search_output_has_results(&output) {
                        self.emit_tool_end(
                            app_handle,
                            thread_id,
                            call_id,
                            "web_search",
                            0,
                            &output,
                        );
                        return Ok(output);
                    }
                    fallback_output = output;
                }
            }
        }

        self.emit_tool_end(
            app_handle,
            thread_id,
            call_id,
            "web_search",
            0,
            &fallback_output,
        );
        Ok(fallback_output)
    }


    pub(crate) async fn fallback_browser_search(
        &self,
        query: &str,
        max_results: usize,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> String {
        let ddg_extract_script = r#"
            (() => {
                const results = [];
                document.querySelectorAll('.result, .nrn-react-div').forEach(el => {
                    const a = el.querySelector('.result__a, a.result-link');
                    const snippet = el.querySelector('.result__snippet, .result__body');
                    if (a) {
                        results.push({
                            title: a.innerText.trim(),
                            url: a.href || '',
                            snippet: snippet ? snippet.innerText.trim() : ''
                        });
                    }
                });
                return JSON.stringify(results);
            })()
        "#;

        let ddg_url = format!(
            "https://duckduckgo.com/?q={}",
            encode_query_component(query)
        );
        let browser_args_ddg = serde_json::json!({
            "engine": "webview-js-injection",
            "url": ddg_url,
            "waitUntil": "networkidle",
            "use_visible_browser": true,
            "actions": [
                { "type": "wait_for_timeout", "ms": 3000 },
                { "type": "eval", "script": ddg_extract_script },
                { "type": "title" },
                { "type": "url" }
            ]
        });

        let ddg_output = self
            .exec_browser_run(
                &browser_args_ddg.to_string(),
                call_id,
                app_handle,
                thread_id,
            )
            .await;

        if let Ok(ref raw) = ddg_output {
            if let Some(results) = parse_browser_search_results(raw, max_results) {
                if !results.is_empty() {
                    return format_browser_search_results(query, &results);
                }
            }
        }

        format!("No web search results found for: {query} (tried Bing and DuckDuckGo)")
    }


    pub(crate) async fn search_bing_browser(
        &self,
        query: &str,
        max_results: usize,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> Option<String> {
        let bing_extract_script = r#"
            (() => {
                const results = [];
                document.querySelectorAll('li.b_algo').forEach(el => {
                    const a = el.querySelector('h2 a');
                    const snippet = el.querySelector('.b_caption p, .b_lineclamp2, .b_algoSlug');
                    if (a) {
                        results.push({
                            title: a.innerText.trim(),
                            url: a.href || '',
                            snippet: snippet ? snippet.innerText.trim() : ''
                        });
                    }
                });
                return JSON.stringify(results);
            })()
        "#;

        let bing_url = format!(
            "https://cn.bing.com/search?q={}",
            encode_query_component(query)
        );
        let browser_args_bing = serde_json::json!({
            "engine": "webview-js-injection",
            "url": bing_url,
            "waitUntil": "networkidle",
            "use_visible_browser": true,
            "actions": [
                { "type": "wait_for_timeout", "ms": 2500 },
                { "type": "eval", "script": bing_extract_script },
                { "type": "title" },
                { "type": "url" }
            ]
        });

        let bing_output = self
            .exec_browser_run(
                &browser_args_bing.to_string(),
                call_id,
                app_handle,
                thread_id,
            )
            .await
            .ok()?;

        let results = parse_browser_search_results(&bing_output, max_results)?;
        if results.is_empty() {
            return None;
        }
        Some(format_browser_search_results(query, &results))
    }


    pub(crate) async fn search_duckduckgo_api(
        &self,
        query: &str,
        max_results: usize,
    ) -> Result<String, String> {
        let search_url = format!(
            "https://api.duckduckgo.com/?q={}&format=json&no_html=1&skip_disambig=1",
            encode_query_component(query)
        );
        let response = self
            .http
            .get(search_url)
            .send()
            .await
            .map_err(|e| format!("Web search request failed: {e}"))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(format!(
                "Web search HTTP error {status}: {}",
                truncate_output(&body, 1000)
            ));
        }

        let parsed = response
            .json::<DuckDuckGoResponse>()
            .await
            .map_err(|e| format!("Web search response parse failed: {e}"))?;
        Ok(format_duckduckgo_results(query, parsed, max_results))
    }


    pub(crate) async fn exec_web_fetch(
        &self,
        arguments: &str,
        call_id: &str,
        app_handle: &AppHandle,
        thread_id: &str,
    ) -> AppResult<String> {
        #[derive(Deserialize)]
        struct FetchArgs {
            url: String,
            #[serde(default)]
            max_chars: Option<usize>,
        }

        let args: FetchArgs = serde_json::from_str(arguments)
            .map_err(|e| crate::error::AppError::Custom(format!("Invalid web_fetch args: {e}")))?;
        let url = args.url.trim();
        let max_chars = args.max_chars.unwrap_or(8000).clamp(1000, 20_000);

        self.emit_tool_start(app_handle, thread_id, call_id, "web_fetch", url);

        if !(url.starts_with("http://") || url.starts_with("https://")) {
            let msg = "Error: web_fetch only supports http:// and https:// URLs".to_string();
            self.emit_tool_end(app_handle, thread_id, call_id, "web_fetch", -1, &msg);
            return Ok(msg);
        }

        info!("Fetching web page: {url}");

        let response = match self.http.get(url).send().await {
            Ok(response) => response,
            Err(e) => {
                let msg = format!("Web fetch request failed: {e}");
                self.emit_tool_end(app_handle, thread_id, call_id, "web_fetch", -1, &msg);
                return Ok(msg);
            }
        };

        let status = response.status();
        let final_url = response.url().to_string();
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .to_string();

        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            let msg = format!(
                "Web fetch HTTP error {status} for {final_url}: {}",
                truncate_output(&body, 1000)
            );
            self.emit_tool_end(app_handle, thread_id, call_id, "web_fetch", -1, &msg);
            return Ok(msg);
        }

        let raw = match response.text().await {
            Ok(raw) => raw,
            Err(e) => {
                let msg = format!("Web fetch body read failed: {e}");
                self.emit_tool_end(app_handle, thread_id, call_id, "web_fetch", -1, &msg);
                return Ok(msg);
            }
        };

        let looks_html = content_type.to_ascii_lowercase().contains("text/html")
            || raw
                .chars()
                .take(500)
                .collect::<String>()
                .to_ascii_lowercase()
                .contains("<html");
        let title = if looks_html {
            extract_html_title(&raw).unwrap_or_default()
        } else {
            String::new()
        };
        let body = if looks_html {
            html_to_text(&raw)
        } else {
            condense_whitespace(&raw)
        };
        let body = truncate_output(&body, max_chars);

        let mut output = format!("URL: {final_url}\nStatus: {status}\n");
        if !content_type.is_empty() {
            output.push_str(&format!("Content-Type: {content_type}\n"));
        }
        if !title.is_empty() {
            output.push_str(&format!("Title: {title}\n"));
        }
        output.push('\n');
        output.push_str(&body);

        self.emit_tool_end(app_handle, thread_id, call_id, "web_fetch", 0, &output);
        Ok(output)
    }

}
