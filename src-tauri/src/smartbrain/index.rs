use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tracing::{error, info};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExperienceEntry {
    pub thread_id: String,
    pub extracted_at: i64,
    pub source_updated_at: i64,
    #[serde(default)]
    pub usage_count: u32,
    #[serde(default)]
    pub last_used_at: Option<i64>,
    #[serde(default)]
    pub summary_slug: Option<String>,
    #[serde(default)]
    pub categories: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExperienceIndex {
    pub version: u32,
    pub entries: Vec<ExperienceEntry>,
    #[serde(default)]
    pub last_consolidated_at: Option<i64>,
}

impl Default for ExperienceIndex {
    fn default() -> Self {
        Self {
            version: 1,
            entries: Vec::new(),
            last_consolidated_at: None,
        }
    }
}

impl ExperienceIndex {
    pub fn load(experiences_dir: &Path) -> Self {
        let index_path = index_path(experiences_dir);
        if !index_path.exists() {
            return Self::default();
        }
        match std::fs::read_to_string(&index_path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or_else(|e| {
                error!("Failed to parse experience index: {e}");
                Self::default()
            }),
            Err(e) => {
                error!("Failed to read experience index: {e}");
                Self::default()
            }
        }
    }

    pub fn save(&self, experiences_dir: &Path) -> Result<(), String> {
        let index_path = index_path(experiences_dir);
        if let Some(parent) = index_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let content = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize experience index: {e}"))?;
        std::fs::write(&index_path, content)
            .map_err(|e| format!("Failed to write experience index: {e}"))?;
        Ok(())
    }

    pub fn has_entry(&self, thread_id: &str) -> bool {
        self.entries.iter().any(|e| e.thread_id == thread_id)
    }

    pub fn is_stale(&self, thread_id: &str, source_updated_at: i64) -> bool {
        self.entries
            .iter()
            .find(|e| e.thread_id == thread_id)
            .is_some_and(|e| e.source_updated_at < source_updated_at)
    }

    pub fn upsert_entry(&mut self, entry: ExperienceEntry) {
        if let Some(existing) = self
            .entries
            .iter_mut()
            .find(|e| e.thread_id == entry.thread_id)
        {
            existing.extracted_at = entry.extracted_at;
            existing.source_updated_at = entry.source_updated_at;
            existing.summary_slug = entry.summary_slug;
            existing.categories = entry.categories;
        } else {
            self.entries.push(entry);
        }
    }

    pub fn record_usage(&mut self, thread_id: &str) {
        let now = now_secs();
        if let Some(entry) = self.entries.iter_mut().find(|e| e.thread_id == thread_id) {
            entry.usage_count += 1;
            entry.last_used_at = Some(now);
        }
    }

    pub fn record_usage_all(&mut self) {
        let now = now_secs();
        for entry in &mut self.entries {
            entry.usage_count += 1;
            entry.last_used_at = Some(now);
        }
    }

    /// Return entries sorted by relevance (usage count + recency).
    pub fn ranked_entries(&self) -> Vec<&ExperienceEntry> {
        let mut entries: Vec<&ExperienceEntry> = self.entries.iter().collect();
        entries.sort_by(|a, b| {
            let score_a = ranking_score(a);
            let score_b = ranking_score(b);
            score_b
                .partial_cmp(&score_a)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        entries
    }

    pub fn prune_expired(&mut self, max_unused_days: i64) {
        let now = now_secs();
        let cutoff = now - (max_unused_days * 86400);
        let before_count = self.entries.len();
        self.entries.retain(|e| {
            if e.usage_count > 0 {
                e.last_used_at.unwrap_or(e.extracted_at) >= cutoff
            } else {
                e.extracted_at >= cutoff
            }
        });
        let pruned = before_count - self.entries.len();
        if pruned > 0 {
            info!("Pruned {pruned} expired experience entries");
        }
    }

    pub fn enforce_capacity(&mut self, max_entries: usize) {
        if self.entries.len() <= max_entries {
            return;
        }
        let mut indexed: Vec<(f64, usize)> = self
            .entries
            .iter()
            .enumerate()
            .map(|(i, e)| (ranking_score(e), i))
            .collect();
        indexed.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        let keep: std::collections::HashSet<usize> = indexed
            .into_iter()
            .take(max_entries)
            .map(|(_, i)| i)
            .collect();
        let mut idx = 0;
        self.entries.retain(|_| {
            let retained = keep.contains(&idx);
            idx += 1;
            retained
        });
    }

    pub fn remove_entry(&mut self, thread_id: &str) -> bool {
        let before = self.entries.len();
        self.entries.retain(|e| e.thread_id != thread_id);
        self.entries.len() < before
    }
}

fn ranking_score(entry: &ExperienceEntry) -> f64 {
    let usage_weight = entry.usage_count as f64;
    let recency_weight =
        entry.last_used_at.or(Some(entry.extracted_at)).unwrap_or(0) as f64 / 1_000_000.0;
    usage_weight * 10.0 + recency_weight
}

fn index_path(experiences_dir: &Path) -> PathBuf {
    experiences_dir.join("index.json")
}

pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
#[path = "index_tests.rs"]
mod tests;
