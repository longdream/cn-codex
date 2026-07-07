use crate::thread_store::ThreadMessage;
use serde::Deserialize;

/// Maximum characters to include from a session transcript for extraction.
const MAX_TRANSCRIPT_CHARS: usize = 60_000;

pub const EXTRACTION_SYSTEM_PROMPT: &str = "\
You are an experience extraction system. Analyze the following session transcript between a user and an AI coding assistant. Extract only long-term, stable experience that will remain useful in future sessions when SmartBrain is enabled.

Output a structured markdown document with these sections (omit empty sections):

## Lessons
Concrete bug fixes, error resolutions, or technical discoveries. Each item should be a self-contained insight.

## User Preferences
Coding style preferences, tool usage patterns, communication preferences, or workflow habits observed.

## Project Knowledge
Project structure, dependency information, build system details, naming conventions, or architectural patterns specific to the user's codebase.

## Workflow Patterns
Successful multi-step workflows, debugging strategies, or task decomposition approaches that worked well.

## Categories
A comma-separated list of category tags for this experience (e.g., debugging, react, rust, refactoring, testing).

## Summary Slug
A short kebab-case identifier for this experience (e.g., fix-react-hydration-mismatch, setup-docker-compose).

## Title
A short, human-readable title (10-30 characters) describing the core lesson or knowledge point. Write in the same language as the session.

## Summary
A 1-2 sentence durable summary of the reusable experience. This is long-term memory, not a recap of this specific session timeline. Write in the same language as the session.

Rules:
- Be concise: each item should be 1-3 sentences.
- Focus on reusable knowledge that can stand alone in a future session.
- Do NOT output recent-chat summaries, progress reports, next-step plans, or temporary task states.
- Do NOT include one-off conversation details that are not reusable.
- Skip trivial interactions (greetings, simple Q&A with no lasting value).
- If the session has no extractable experience, output exactly: NO_EXPERIENCE_FOUND";

pub const CONSOLIDATION_SYSTEM_PROMPT: &str = "\
You are a knowledge consolidation system. You will receive a collection of raw experience notes extracted from past coding sessions. Your job is to merge, deduplicate, and organize them into two outputs.

OUTPUT 1 — experience_summary.md:
A concise, well-organized summary (under {max_tokens} tokens) that captures the most important and frequently useful knowledge. This summary will be injected into every new session's system prompt, so it must be compact and high-signal. Group related items. Use short bullet points. Prioritize items that have been used more frequently (higher usage counts).

OUTPUT 2 — experience_handbook.md:
A comprehensive, searchable reference organized by category. Include all non-redundant knowledge. Use markdown headers for categories. This document will be available for the model to read on demand.

Format your response as:

---BEGIN experience_summary.md---
(content here)
---END experience_summary.md---

---BEGIN experience_handbook.md---
(content here)
---END experience_handbook.md---

Rules:
- Merge duplicate or near-duplicate entries into single, improved versions.
- Preserve concrete technical details (error messages, command syntax, file paths).
- Remove outdated or contradictory information (prefer the more recent entry).
- Keep the summary focused on actionable knowledge, not narrative.";

pub const SUMMARIZE_MERGE_SYSTEM_PROMPT: &str = "\
You are an experience consolidation system. You will receive a collection of raw experience notes extracted from past coding sessions. Your job is to categorize and merge similar/duplicate experiences into a SMALLER set of consolidated experience entries, reducing the total count while preserving all reusable knowledge.

You must produce AT MOST {target_count} merged entries, and strictly FEWER than the number of input experiences. Group experiences that share the same topic, technology, or lesson. Each merged entry must combine the knowledge of its members without losing concrete technical details (error messages, command syntax, file paths).

Format your response as:

---BEGIN merged_experiences.json---
[
  {
    \"title\": \"Short human-readable title (10-30 chars), same language as the input\",
    \"summary\": \"1-2 sentence durable summary of the reusable experience\",
    \"slug\": \"kebab-case-identifier\",
    \"categories\": [\"tag1\", \"tag2\"],
    \"content\": \"Full markdown body. Use ## sections (Lessons, User Preferences, Project Knowledge, Workflow Patterns) as needed. Merge and deduplicate the member experiences' content here.\"
  }
]
---END merged_experiences.json---

Rules:
- Produce fewer entries than the input. Merge aggressively when experiences overlap.
- Never invent new knowledge not present in the inputs.
- Preserve concrete technical details from the source experiences.
- `categories` should be lowercase tags merged from the members (2-8 tags).
- `content` must be self-contained markdown usable as a standalone experience document.
- Write titles and summaries in the same language as the source experiences.
- If all inputs are trivial or empty, output an empty array: []";

pub const KNOWLEDGE_ORGANIZE_SYSTEM_PROMPT: &str = "\
You are a knowledge organization system. You will receive raw text extracted from a document. Your job is to:

1. Organize the content into a clear hierarchical structure with categories.
2. Produce a clean markdown document with the organized knowledge.
3. Produce a JSON category hierarchy for indexing.
4. Extract normalized metadata for indexing and retrieval.

Format your response as:

---BEGIN organized_knowledge.md---
(organized markdown content here, using ## headers for categories and ### for sub-categories)
---END organized_knowledge.md---

---BEGIN hierarchy.json---
{\"categories\": [{\"name\": \"Category Name\", \"summary\": \"Brief summary\"}]}
---END hierarchy.json---

---BEGIN metadata.json---
{\"title\": \"Suggested title\", \"description\": \"Brief summary\", \"domain\": \"topic\", \"tags\": [\"tag1\", \"tag2\"]}
---END metadata.json---

Rules:
- Group related information under clear category headers.
- Preserve all technical details (code snippets, command syntax, configuration).
- Use concise bullet points within each category.
- Remove redundant or repeated information.
- Categories should be descriptive and specific.
- metadata.title and metadata.description must be concise and searchable.
- metadata.tags should contain 2-8 concrete tags when possible.
- metadata.domain should be a short domain label (for example: backend, frontend, database, devops).";

#[derive(Debug, Clone, Default, Deserialize)]
pub struct KnowledgeOrganizeMetadata {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub domain: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

/// A single merged experience entry produced by the summarize-merge pipeline.
#[derive(Debug, Clone, Deserialize)]
pub struct MergedExperience {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub slug: Option<String>,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SummarizeMergeParseReason {
    EmptyArray,
    MissingMarkers,
    InvalidJson(String),
    AllEmptyContent,
}

impl std::fmt::Display for SummarizeMergeParseReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyArray => f.write_str("empty_array"),
            Self::MissingMarkers => f.write_str("missing_markers"),
            Self::InvalidJson(error) => write!(f, "invalid_json: {error}"),
            Self::AllEmptyContent => f.write_str("all_empty_content"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ParsedKnowledgeOrganizeOutput {
    pub organized_markdown: String,
    pub hierarchy: Option<serde_json::Value>,
    pub metadata: Option<KnowledgeOrganizeMetadata>,
}

pub fn build_extraction_messages(history: &[ThreadMessage]) -> Vec<(String, String)> {
    let mut transcript = String::new();
    for msg in history {
        if msg.role == "system" {
            continue;
        }
        if msg.role == "user" && crate::compaction::is_summary_message(&msg.content) {
            continue;
        }
        let role_label = match msg.role.as_str() {
            "user" => "USER",
            "assistant" => "ASSISTANT",
            "tool" => "TOOL_RESULT",
            _ => &msg.role,
        };

        let content = if msg.role == "assistant" && msg.content.is_empty() {
            if let Some(ref tool_calls) = msg.tool_calls {
                let calls: Vec<String> = tool_calls
                    .iter()
                    .map(|tc| format!("{}({})", tc.name, truncate_str(&tc.arguments, 200)))
                    .collect();
                format!("[tool calls: {}]", calls.join(", "))
            } else {
                continue;
            }
        } else {
            truncate_str(&msg.content, 2000)
        };

        transcript.push_str(&format!("{role_label}: {content}\n\n"));

        if transcript.len() > MAX_TRANSCRIPT_CHARS {
            transcript.push_str("... (transcript truncated) ...\n");
            break;
        }
    }

    vec![
        ("system".to_string(), EXTRACTION_SYSTEM_PROMPT.to_string()),
        (
            "user".to_string(),
            format!(
                "Here is the session transcript to analyze:\n\n---\n{transcript}\n---\n\nExtract the durable experience from this session."
            ),
        ),
    ]
}

pub fn build_consolidation_messages(
    raw_experiences: &[(String, String, u32)],
    max_summary_tokens: usize,
) -> Vec<(String, String)> {
    let system =
        CONSOLIDATION_SYSTEM_PROMPT.replace("{max_tokens}", &max_summary_tokens.to_string());

    let mut user_content = String::from(
        "Here are the raw experience notes to consolidate. Each entry includes the thread ID, usage count, and content:\n\n",
    );

    for (thread_id, content, usage_count) in raw_experiences {
        user_content.push_str(&format!(
            "--- Experience from thread {thread_id} (used {usage_count} times) ---\n{content}\n\n"
        ));
    }

    user_content.push_str(
        "Please consolidate these into experience_summary.md and experience_handbook.md.",
    );

    vec![
        ("system".to_string(), system),
        ("user".to_string(), user_content),
    ]
}

pub fn build_summarize_merge_messages(
    raw_experiences: &[(String, String, u32)],
    target_count: usize,
) -> Vec<(String, String)> {
    let system = SUMMARIZE_MERGE_SYSTEM_PROMPT.replace("{target_count}", &target_count.to_string());

    let mut user_content = String::from(
        "Here are the raw experience notes to categorize and merge. Each entry includes the thread ID, usage count, and content:\n\n",
    );

    for (thread_id, content, usage_count) in raw_experiences {
        user_content.push_str(&format!(
            "--- Experience from thread {thread_id} (used {usage_count} times) ---\n{content}\n\n"
        ));
    }

    user_content.push_str(&format!(
        "Please categorize and merge these into AT MOST {target_count} consolidated experience entries (fewer than the {count} inputs).",
        count = raw_experiences.len()
    ));

    vec![
        ("system".to_string(), system),
        ("user".to_string(), user_content),
    ]
}

pub fn build_knowledge_organize_messages(
    raw_text: &str,
    source_name: &str,
) -> Vec<(String, String)> {
    vec![
        (
            "system".to_string(),
            KNOWLEDGE_ORGANIZE_SYSTEM_PROMPT.to_string(),
        ),
        (
            "user".to_string(),
            format!(
                "Here is the raw text from document \"{source_name}\" to organize:\n\n---\n{}\n---\n\nPlease organize this into structured knowledge.",
                truncate_str(raw_text, MAX_TRANSCRIPT_CHARS)
            ),
        ),
    ]
}

pub fn parse_extraction_output(output: &str) -> Option<ParsedExtraction> {
    if output.contains("NO_EXPERIENCE_FOUND") {
        return None;
    }

    let slug = extract_section(output, "## Summary Slug")
        .map(|s| s.trim().trim_matches('`').to_string())
        .filter(|s| !s.is_empty());

    let title = extract_section(output, "## Title")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let summary = extract_section(output, "## Summary")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let categories = extract_section(output, "## Categories")
        .map(|s| {
            s.split(',')
                .map(|c| c.trim().to_lowercase())
                .filter(|c| !c.is_empty())
                .collect()
        })
        .unwrap_or_default();

    Some(ParsedExtraction {
        full_text: output.to_string(),
        slug,
        title,
        summary,
        categories,
    })
}

pub fn parse_consolidation_output(output: &str) -> (String, String) {
    let summary = extract_delimited(
        output,
        "---BEGIN experience_summary.md---",
        "---END experience_summary.md---",
    )
    .unwrap_or_default();
    let handbook = extract_delimited(
        output,
        "---BEGIN experience_handbook.md---",
        "---END experience_handbook.md---",
    )
    .unwrap_or_default();
    (summary, handbook)
}

pub fn parse_summarize_merge_output(
    output: &str,
) -> Result<Vec<MergedExperience>, SummarizeMergeParseReason> {
    let mut candidates = Vec::new();
    if let Some(text) = extract_delimited(
        output,
        "---BEGIN merged_experiences.json---",
        "---END merged_experiences.json---",
    ) {
        candidates.push(text);
    }

    if candidates.is_empty() {
        candidates = extract_json_array_candidates(output);
    }

    if candidates.is_empty() {
        if output.contains('[') {
            return Err(SummarizeMergeParseReason::InvalidJson(
                "could not extract a complete JSON array".to_string(),
            ));
        }
        return Err(SummarizeMergeParseReason::MissingMarkers);
    }

    let mut first_json_error: Option<String> = None;
    let mut saw_empty_array = false;
    let mut saw_all_empty_content = false;

    for candidate in candidates {
        let json_text = candidate.trim();
        if json_text.is_empty() {
            continue;
        }

        match serde_json::from_str::<Vec<MergedExperience>>(json_text) {
            Ok(parsed) => {
                if parsed.is_empty() {
                    saw_empty_array = true;
                    continue;
                }

                let filtered: Vec<MergedExperience> = parsed
                    .into_iter()
                    .filter(|entry| !entry.content.trim().is_empty())
                    .collect();
                if filtered.is_empty() {
                    saw_all_empty_content = true;
                    continue;
                }
                return Ok(filtered);
            }
            Err(error) => {
                if first_json_error.is_none() {
                    first_json_error = Some(error.to_string());
                }
            }
        }
    }

    if saw_all_empty_content {
        return Err(SummarizeMergeParseReason::AllEmptyContent);
    }
    if saw_empty_array {
        return Err(SummarizeMergeParseReason::EmptyArray);
    }
    Err(SummarizeMergeParseReason::InvalidJson(
        first_json_error.unwrap_or_else(|| "unknown JSON parse error".to_string()),
    ))
}

pub fn parse_knowledge_organize_output(output: &str) -> ParsedKnowledgeOrganizeOutput {
    let organized = extract_delimited(
        output,
        "---BEGIN organized_knowledge.md---",
        "---END organized_knowledge.md---",
    )
    .unwrap_or_else(|| output.to_string());
    let hierarchy = extract_delimited(
        output,
        "---BEGIN hierarchy.json---",
        "---END hierarchy.json---",
    )
    .and_then(|s| serde_json::from_str(&s).ok());
    let metadata = extract_delimited(
        output,
        "---BEGIN metadata.json---",
        "---END metadata.json---",
    )
    .and_then(|s| serde_json::from_str::<KnowledgeOrganizeMetadata>(&s).ok());
    ParsedKnowledgeOrganizeOutput {
        organized_markdown: organized,
        hierarchy,
        metadata,
    }
}

pub struct ParsedExtraction {
    pub full_text: String,
    pub slug: Option<String>,
    pub title: Option<String>,
    pub summary: Option<String>,
    pub categories: Vec<String>,
}

fn extract_section(text: &str, header: &str) -> Option<String> {
    let start = text.find(header)?;
    let content_start = start + header.len();
    let rest = &text[content_start..];

    let end = rest
        .find("\n## ")
        .or_else(|| rest.find("\n---"))
        .unwrap_or(rest.len());

    let section = rest[..end].trim();
    if section.is_empty() {
        None
    } else {
        Some(section.to_string())
    }
}

fn extract_delimited(text: &str, begin_marker: &str, end_marker: &str) -> Option<String> {
    let start = text.find(begin_marker)? + begin_marker.len();
    let end = text[start..].find(end_marker)? + start;
    let content = text[start..end].trim();
    if content.is_empty() {
        None
    } else {
        Some(content.to_string())
    }
}

fn extract_json_array_candidates(text: &str) -> Vec<String> {
    let mut arrays = Vec::new();
    let mut start_idx: Option<usize> = None;
    let mut depth: usize = 0;
    let mut in_string = false;
    let mut escaped = false;

    for (idx, ch) in text.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
                continue;
            }
            if ch == '\\' {
                escaped = true;
                continue;
            }
            if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '[' => {
                if depth == 0 {
                    start_idx = Some(idx);
                }
                depth += 1;
            }
            ']' => {
                if depth == 0 {
                    continue;
                }
                depth -= 1;
                if depth == 0 {
                    if let Some(start) = start_idx.take() {
                        let end = idx + ch.len_utf8();
                        let slice = text[start..end].trim();
                        if !slice.is_empty()
                            && !arrays
                                .iter()
                                .any(|existing: &String| existing.as_str() == slice)
                        {
                            arrays.push(slice.to_string());
                        }
                    }
                }
            }
            _ => {}
        }
    }

    arrays.sort_by_key(|candidate| std::cmp::Reverse(candidate.len()));
    arrays
}

fn truncate_str(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_chars).collect();
        format!("{truncated}...")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_message(role: &str, content: &str) -> ThreadMessage {
        ThreadMessage {
            id: format!("{role}-message"),
            role: role.to_string(),
            content: content.to_string(),
            timestamp: 0,
            tool_call_id: None,
            tool_name: None,
            tool_calls: None,
            attachments: Vec::new(),
        }
    }

    #[test]
    fn parse_extraction_no_experience() {
        assert!(parse_extraction_output("NO_EXPERIENCE_FOUND").is_none());
    }

    #[test]
    fn build_extraction_messages_filters_compaction_summary_user_message() {
        let summary_message = format!(
            "{}\n{}",
            crate::compaction::SUMMARY_PREFIX,
            "summarized progress that should not become durable memory"
        );
        let history = vec![
            test_message("user", "Please fix the build error."),
            test_message("assistant", "I will inspect cargo errors."),
            test_message("user", &summary_message),
            test_message("tool", "cargo check failed: unresolved import"),
        ];

        let messages = build_extraction_messages(&history);
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].0, "system");
        assert_eq!(messages[1].0, "user");
        assert!(messages[1].1.contains("USER: Please fix the build error."));
        assert!(
            messages[1]
                .1
                .contains("ASSISTANT: I will inspect cargo errors.")
        );
        assert!(
            messages[1]
                .1
                .contains("TOOL_RESULT: cargo check failed: unresolved import")
        );
        assert!(!messages[1].1.contains("summarized progress"));
        assert!(!messages[1].1.contains(crate::compaction::SUMMARY_PREFIX));
    }

    #[test]
    fn parse_extraction_with_content() {
        let output = "\
## Lessons
- Fixed a hydration mismatch by using useEffect.

## Categories
debugging, react, nextjs

## Summary Slug
fix-react-hydration-mismatch
";
        let parsed = parse_extraction_output(output).unwrap();
        assert_eq!(parsed.slug.as_deref(), Some("fix-react-hydration-mismatch"));
        assert_eq!(parsed.categories, vec!["debugging", "react", "nextjs"]);
        assert!(parsed.full_text.contains("hydration mismatch"));
    }

    #[test]
    fn parse_consolidation_output_splits_correctly() {
        let output = "\
Some preamble text.

---BEGIN experience_summary.md---
- Key insight 1
- Key insight 2
---END experience_summary.md---

---BEGIN experience_handbook.md---
# Debugging
- Detail about debugging
---END experience_handbook.md---
";
        let (summary, handbook) = parse_consolidation_output(output);
        assert!(summary.contains("Key insight 1"));
        assert!(handbook.contains("# Debugging"));
    }

    #[test]
    fn parse_consolidation_output_handles_missing() {
        let output = "Some random text without markers";
        let (summary, handbook) = parse_consolidation_output(output);
        assert!(summary.is_empty());
        assert!(handbook.is_empty());
    }

    #[test]
    fn parse_knowledge_organize_output_works() {
        let output = "\
---BEGIN organized_knowledge.md---
## API Reference
- endpoint /api/v1/users
---END organized_knowledge.md---

---BEGIN hierarchy.json---
{\"categories\": [{\"name\": \"API Reference\", \"summary\": \"REST API endpoints\"}]}
---END hierarchy.json---

---BEGIN metadata.json---
{\"title\": \"API Guide\", \"description\": \"Core REST endpoints\", \"domain\": \"backend\", \"tags\": [\"api\", \"rest\"]}
---END metadata.json---
";
        let parsed = parse_knowledge_organize_output(output);
        assert!(parsed.organized_markdown.contains("API Reference"));
        assert!(parsed.hierarchy.is_some());
        assert_eq!(
            parsed
                .metadata
                .as_ref()
                .and_then(|meta| meta.title.as_deref()),
            Some("API Guide")
        );
        assert_eq!(
            parsed.metadata.as_ref().map(|meta| meta.tags.clone()),
            Some(vec!["api".to_string(), "rest".to_string()])
        );
    }

    #[test]
    fn parse_knowledge_organize_output_tolerates_missing_metadata_block() {
        let output = "\
---BEGIN organized_knowledge.md---
## Notes
- item
---END organized_knowledge.md---

---BEGIN hierarchy.json---
{\"categories\": [{\"name\": \"Notes\", \"summary\": \"Simple notes\"}]}
---END hierarchy.json---
";
        let parsed = parse_knowledge_organize_output(output);
        assert!(parsed.organized_markdown.contains("## Notes"));
        assert!(parsed.hierarchy.is_some());
        assert!(parsed.metadata.is_none());
    }

    #[test]
    fn parse_summarize_merge_output_parses_delimited_json() {
        let output = "\
Some preamble.

---BEGIN merged_experiences.json---
[
  {
    \"title\": \"React debugging\",
    \"summary\": \"Fix common React issues.\",
    \"slug\": \"react-debugging\",
    \"categories\": [\"debugging\", \"react\"],
    \"content\": \"## Lessons\\n- Use useEffect for hydration.\"
  },
  {
    \"title\": \"Rust build\",
    \"summary\": \"Cargo build tips.\",
    \"slug\": \"rust-build\",
    \"categories\": [\"rust\", \"build\"],
    \"content\": \"## Lessons\\n- Run cargo check.\"
  }
]
---END merged_experiences.json---
";
        let merged = parse_summarize_merge_output(output).expect("should parse merged experiences");
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].title.as_deref(), Some("React debugging"));
        assert_eq!(merged[0].categories, vec!["debugging", "react"]);
        assert!(merged[0].content.contains("useEffect"));
        assert_eq!(merged[1].slug.as_deref(), Some("rust-build"));
    }

    #[test]
    fn parse_summarize_merge_output_tolerates_bare_json_array() {
        let output = "[{\"title\":\"t\",\"summary\":\"s\",\"slug\":\"sl\",\"categories\":[\"a\"],\"content\":\"body\"}]";
        let merged = parse_summarize_merge_output(output).expect("should parse bare array");
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].content, "body");
    }

    #[test]
    fn parse_summarize_merge_output_drops_empty_content_entries() {
        let output = "\
---BEGIN merged_experiences.json---
[
  {\"title\":\"keep\",\"content\":\"real content\"},
  {\"title\":\"drop\",\"content\":\"   \"}
]
---END merged_experiences.json---
";
        let merged = parse_summarize_merge_output(output).expect("should keep non-empty content");
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].title.as_deref(), Some("keep"));
    }

    #[test]
    fn parse_summarize_merge_output_handles_empty_array() {
        let output = "---BEGIN merged_experiences.json---\n[]\n---END merged_experiences.json---";
        let reason = parse_summarize_merge_output(output).expect_err("should report empty array");
        assert_eq!(reason, SummarizeMergeParseReason::EmptyArray);
    }

    #[test]
    fn parse_summarize_merge_output_supports_wrapped_json_array() {
        let output = "\
Here is the merged result:

[
  {
    \"title\": \"Wrapped\",
    \"summary\": \"Works with wrapper text\",
    \"slug\": \"wrapped\",
    \"categories\": [\"test\"],
    \"content\": \"## Lessons\\n- Keep durable content.\"
  }
]

Done.";
        let merged = parse_summarize_merge_output(output).expect("should parse wrapped array");
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].slug.as_deref(), Some("wrapped"));
    }

    #[test]
    fn parse_summarize_merge_output_supports_markdown_code_fence() {
        let output = "\
```json
[
  {
    \"title\": \"Fence\",
    \"summary\": \"Code fence\",
    \"slug\": \"fence\",
    \"categories\": [\"test\"],
    \"content\": \"## Lessons\\n- Parse fenced JSON.\"
  }
]
```";
        let merged = parse_summarize_merge_output(output).expect("should parse fenced JSON");
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].slug.as_deref(), Some("fence"));
    }

    #[test]
    fn parse_summarize_merge_output_reports_invalid_json() {
        let output = "\
---BEGIN merged_experiences.json---
[
  {\"title\": \"bad\", \"content\": \"oops\",
]
---END merged_experiences.json---";
        let reason =
            parse_summarize_merge_output(output).expect_err("should return invalid json reason");
        assert!(matches!(reason, SummarizeMergeParseReason::InvalidJson(_)));
    }

    #[test]
    fn parse_summarize_merge_output_reports_all_empty_content() {
        let output = "\
---BEGIN merged_experiences.json---
[
  {\"title\": \"empty-a\", \"content\": \"   \"},
  {\"title\": \"empty-b\"}
]
---END merged_experiences.json---";
        let reason =
            parse_summarize_merge_output(output).expect_err("should report empty content entries");
        assert_eq!(reason, SummarizeMergeParseReason::AllEmptyContent);
    }

    #[test]
    fn parse_summarize_merge_output_reports_missing_markers() {
        let output = "No JSON array here.";
        let reason =
            parse_summarize_merge_output(output).expect_err("should report missing markers");
        assert_eq!(reason, SummarizeMergeParseReason::MissingMarkers);
    }
}
