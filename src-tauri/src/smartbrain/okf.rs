use std::collections::HashMap;
use std::path::Path;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Represents the YAML frontmatter of an OKF concept document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OkfFrontmatter {
    #[serde(rename = "type")]
    pub concept_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
    #[serde(flatten)]
    pub extensions: HashMap<String, serde_json::Value>,
}

/// A fully parsed OKF concept document: frontmatter + body.
#[derive(Debug, Clone)]
pub struct OkfDocument {
    pub frontmatter: OkfFrontmatter,
    pub body: String,
}

impl OkfFrontmatter {
    pub fn new(concept_type: impl Into<String>) -> Self {
        Self {
            concept_type: concept_type.into(),
            title: None,
            description: None,
            tags: Vec::new(),
            timestamp: None,
            extensions: HashMap::new(),
        }
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = tags;
        self
    }

    pub fn with_timestamp_now(mut self) -> Self {
        self.timestamp = Some(Utc::now().to_rfc3339());
        self
    }

    pub fn with_timestamp(mut self, ts: i64) -> Self {
        if let Some(dt) = DateTime::from_timestamp(ts, 0) {
            self.timestamp = Some(dt.to_rfc3339());
        }
        self
    }

    pub fn with_extension(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.extensions.insert(key.into(), value);
        self
    }

    /// Parse the timestamp field back into a unix timestamp (seconds).
    pub fn timestamp_secs(&self) -> Option<i64> {
        self.timestamp.as_ref().and_then(|ts| {
            DateTime::parse_from_rfc3339(ts)
                .ok()
                .map(|dt| dt.timestamp())
        })
    }
}

impl OkfDocument {
    pub fn new(frontmatter: OkfFrontmatter, body: impl Into<String>) -> Self {
        Self {
            frontmatter,
            body: body.into(),
        }
    }

    /// Serialize the document to a string with YAML frontmatter and markdown body.
    pub fn to_string(&self) -> Result<String, String> {
        let yaml = serde_yaml::to_string(&self.frontmatter)
            .map_err(|e| format!("Failed to serialize OKF frontmatter: {e}"))?;
        let yaml = yaml.trim_end();
        Ok(format!("---\n{yaml}\n---\n\n{}", self.body))
    }

    /// Write the document to a file path.
    pub fn write_to(&self, path: &Path) -> Result<(), String> {
        let content = self.to_string()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create directory: {e}"))?;
        }
        std::fs::write(path, content).map_err(|e| format!("Failed to write OKF document: {e}"))
    }
}

/// Parse an OKF markdown file into frontmatter + body.
/// Returns None if the file has no valid frontmatter.
pub fn parse_document(content: &str) -> Option<OkfDocument> {
    let content = content.trim_start_matches('\u{FEFF}');

    if !content.starts_with("---") {
        return None;
    }

    let after_first = &content[3..];
    let newline_pos = after_first.find('\n')?;
    let after_first_line = &after_first[newline_pos + 1..];

    let end_marker = find_closing_fence(after_first_line)?;
    let yaml_str = &after_first_line[..end_marker];
    let body_start = end_marker + 4; // "---\n"
    let body = if body_start <= after_first_line.len() {
        after_first_line[body_start..].trim_start().to_string()
    } else {
        String::new()
    };

    let frontmatter: OkfFrontmatter = serde_yaml::from_str(yaml_str).ok()?;

    Some(OkfDocument { frontmatter, body })
}

/// Parse frontmatter only, returning None if invalid.
pub fn parse_frontmatter(content: &str) -> Option<OkfFrontmatter> {
    parse_document(content).map(|doc| doc.frontmatter)
}

/// Extract the body content only (stripping frontmatter).
pub fn extract_body(content: &str) -> String {
    match parse_document(content) {
        Some(doc) => doc.body,
        None => content.to_string(),
    }
}

/// Check if a markdown file has OKF-conformant frontmatter.
pub fn has_frontmatter(content: &str) -> bool {
    parse_frontmatter(content).is_some()
}

fn find_closing_fence(content: &str) -> Option<usize> {
    for (i, line) in content.lines().enumerate() {
        if line.trim() == "---" {
            let byte_offset: usize = content.lines().take(i).map(|l| l.len() + 1).sum();
            return Some(byte_offset);
        }
    }
    None
}

/// Generate an OKF `index.md` file content from a list of (relative_path, title, description) entries.
pub fn generate_index_md(
    section_title: &str,
    entries: &[(String, String, Option<String>)],
    okf_version: Option<&str>,
) -> String {
    let mut output = String::new();

    if let Some(version) = okf_version {
        output.push_str(&format!("---\nokf_version: \"{version}\"\n---\n\n"));
    }

    output.push_str(&format!("# {section_title}\n\n"));

    for (path, title, description) in entries {
        if let Some(desc) = description {
            output.push_str(&format!("* [{title}]({path}) - {desc}\n"));
        } else {
            output.push_str(&format!("* [{title}]({path})\n"));
        }
    }

    output
}

/// Generate or append to an OKF `log.md` file.
pub fn append_log_entry(existing_log: &str, action: &str, description: &str) -> String {
    let today = Utc::now().format("%Y-%m-%d").to_string();
    let new_entry = format!("* **{action}**: {description}");

    if existing_log.trim().is_empty() {
        return format!("# Update Log\n\n## {today}\n{new_entry}\n");
    }

    let date_header = format!("## {today}");
    if existing_log.contains(&date_header) {
        let pos = existing_log.find(&date_header).unwrap();
        let after_header = pos + date_header.len();
        let insert_pos = existing_log[after_header..]
            .find('\n')
            .map(|p| after_header + p + 1)
            .unwrap_or(existing_log.len());
        let mut result = existing_log[..insert_pos].to_string();
        result.push_str(&new_entry);
        result.push('\n');
        result.push_str(&existing_log[insert_pos..]);
        result
    } else {
        let insert_after_title = existing_log
            .find("\n\n")
            .map(|p| p + 2)
            .unwrap_or(existing_log.len());
        let mut result = existing_log[..insert_after_title].to_string();
        result.push_str(&format!("{date_header}\n{new_entry}\n\n"));
        result.push_str(&existing_log[insert_after_title..]);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_okf_document() {
        let content = r#"---
type: Knowledge
title: "Test Doc"
description: "A test document"
tags: [rust, testing]
timestamp: "2026-06-19T10:00:00+00:00"
source_file: "test.md"
---

# Content

Some body text here.
"#;

        let doc = parse_document(content).unwrap();
        assert_eq!(doc.frontmatter.concept_type, "Knowledge");
        assert_eq!(doc.frontmatter.title.as_deref(), Some("Test Doc"));
        assert_eq!(
            doc.frontmatter.description.as_deref(),
            Some("A test document")
        );
        assert_eq!(doc.frontmatter.tags, vec!["rust", "testing"]);
        assert!(doc.body.contains("# Content"));
        assert!(doc.body.contains("Some body text here."));
        assert_eq!(
            doc.frontmatter.extensions.get("source_file"),
            Some(&serde_json::Value::String("test.md".to_string()))
        );
    }

    #[test]
    fn parse_no_frontmatter() {
        let content = "# Just a regular markdown file\n\nNo frontmatter.";
        assert!(parse_document(content).is_none());
    }

    #[test]
    fn roundtrip_serialization() {
        let fm = OkfFrontmatter::new("Experience")
            .with_title("fix-routing")
            .with_description("Fixed React routing issue")
            .with_tags(vec!["react".into(), "routing".into()])
            .with_timestamp(1718784000)
            .with_extension("thread_id", serde_json::json!("abc-123"));

        let doc = OkfDocument::new(fm, "# Lessons\n\nSome content.");
        let serialized = doc.to_string().unwrap();

        let parsed = parse_document(&serialized).unwrap();
        assert_eq!(parsed.frontmatter.concept_type, "Experience");
        assert_eq!(parsed.frontmatter.title.as_deref(), Some("fix-routing"));
        assert!(parsed.body.contains("# Lessons"));
    }

    #[test]
    fn generate_index_md_with_version() {
        let entries = vec![
            (
                "docs/test.md".into(),
                "Test Doc".into(),
                Some("A test".into()),
            ),
            ("docs/other.md".into(), "Other".into(), None),
        ];
        let result = generate_index_md("Knowledge Base", &entries, Some("0.1"));
        assert!(result.contains("okf_version: \"0.1\""));
        assert!(result.contains("* [Test Doc](docs/test.md) - A test"));
        assert!(result.contains("* [Other](docs/other.md)"));
    }

    #[test]
    fn append_log_new_file() {
        let result = append_log_entry("", "Creation", "Added new document test.md");
        assert!(result.contains("# Update Log"));
        assert!(result.contains("**Creation**: Added new document test.md"));
    }

    #[test]
    fn extract_body_with_frontmatter() {
        let content = "---\ntype: Knowledge\ntitle: Test\n---\n\n# Body\n\nText.";
        let body = extract_body(content);
        assert_eq!(body, "# Body\n\nText.");
    }

    #[test]
    fn extract_body_no_frontmatter() {
        let content = "# Just markdown\n\nNo frontmatter.";
        let body = extract_body(content);
        assert_eq!(body, content);
    }
}
