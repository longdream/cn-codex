use crate::thread_store::ThreadMessage;

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

pub const KNOWLEDGE_ORGANIZE_SYSTEM_PROMPT: &str = "\
You are a knowledge organization system. You will receive raw text extracted from a document. Your job is to:

1. Organize the content into a clear hierarchical structure with categories.
2. Produce a clean markdown document with the organized knowledge.
3. Produce a JSON category hierarchy for indexing.

Format your response as:

---BEGIN organized_knowledge.md---
(organized markdown content here, using ## headers for categories and ### for sub-categories)
---END organized_knowledge.md---

---BEGIN hierarchy.json---
{\"categories\": [{\"name\": \"Category Name\", \"summary\": \"Brief summary\"}]}
---END hierarchy.json---

Rules:
- Group related information under clear category headers.
- Preserve all technical details (code snippets, command syntax, configuration).
- Use concise bullet points within each category.
- Remove redundant or repeated information.
- Categories should be descriptive and specific.";

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

pub fn parse_knowledge_organize_output(output: &str) -> (String, Option<serde_json::Value>) {
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
    (organized, hierarchy)
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
";
        let (organized, hierarchy) = parse_knowledge_organize_output(output);
        assert!(organized.contains("API Reference"));
        assert!(hierarchy.is_some());
    }
}
