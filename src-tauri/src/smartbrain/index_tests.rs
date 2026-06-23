use super::*;

#[test]
fn default_index_is_empty() {
    let idx = ExperienceIndex::default();
    assert_eq!(idx.version, 1);
    assert!(idx.entries.is_empty());
    assert!(idx.last_consolidated_at.is_none());
}

#[test]
fn upsert_and_lookup() {
    let mut idx = ExperienceIndex::default();
    assert!(!idx.has_entry("t1"));

    idx.upsert_entry(ExperienceEntry {
        thread_id: "t1".to_string(),
        extracted_at: 100,
        source_updated_at: 90,
        usage_count: 0,
        last_used_at: None,
        summary_slug: Some("test".to_string()),
        title: None,
        summary: None,
        categories: vec!["debug".to_string()],
    });
    assert!(idx.has_entry("t1"));
    assert_eq!(idx.entries.len(), 1);

    idx.upsert_entry(ExperienceEntry {
        thread_id: "t1".to_string(),
        extracted_at: 200,
        source_updated_at: 190,
        usage_count: 0,
        last_used_at: None,
        summary_slug: Some("updated".to_string()),
        title: None,
        summary: None,
        categories: vec![],
    });
    assert_eq!(idx.entries.len(), 1);
    assert_eq!(idx.entries[0].extracted_at, 200);
    assert_eq!(idx.entries[0].summary_slug.as_deref(), Some("updated"));
}

#[test]
fn record_usage_increments_count() {
    let mut idx = ExperienceIndex::default();
    idx.upsert_entry(ExperienceEntry {
        thread_id: "t1".to_string(),
        extracted_at: 100,
        source_updated_at: 90,
        usage_count: 0,
        last_used_at: None,
        summary_slug: None,
        title: None,
        summary: None,
        categories: vec![],
    });

    idx.record_usage("t1");
    assert_eq!(idx.entries[0].usage_count, 1);
    assert!(idx.entries[0].last_used_at.is_some());

    idx.record_usage("t1");
    assert_eq!(idx.entries[0].usage_count, 2);

    idx.record_usage("nonexistent");
    assert_eq!(idx.entries.len(), 1);
}

#[test]
fn stale_detection() {
    let mut idx = ExperienceIndex::default();
    idx.upsert_entry(ExperienceEntry {
        thread_id: "t1".to_string(),
        extracted_at: 100,
        source_updated_at: 90,
        usage_count: 0,
        last_used_at: None,
        summary_slug: None,
        title: None,
        summary: None,
        categories: vec![],
    });

    assert!(!idx.is_stale("t1", 90));
    assert!(!idx.is_stale("t1", 80));
    assert!(idx.is_stale("t1", 100));
}

#[test]
fn enforce_capacity_keeps_top_entries() {
    let mut idx = ExperienceIndex::default();
    for i in 0..5 {
        idx.upsert_entry(ExperienceEntry {
            thread_id: format!("t{i}"),
            extracted_at: 100 + i as i64,
            source_updated_at: 90 + i as i64,
            usage_count: i as u32,
            last_used_at: None,
            summary_slug: None,
            title: None,
            summary: None,
            categories: vec![],
        });
    }

    idx.enforce_capacity(3);
    assert_eq!(idx.entries.len(), 3);
}

#[test]
fn ranked_entries_orders_by_usage_and_recency() {
    let mut idx = ExperienceIndex::default();
    idx.upsert_entry(ExperienceEntry {
        thread_id: "low".to_string(),
        extracted_at: 200,
        source_updated_at: 190,
        usage_count: 0,
        last_used_at: None,
        summary_slug: None,
        title: None,
        summary: None,
        categories: vec![],
    });
    idx.upsert_entry(ExperienceEntry {
        thread_id: "high".to_string(),
        extracted_at: 100,
        source_updated_at: 90,
        usage_count: 10,
        last_used_at: Some(300),
        summary_slug: None,
        title: None,
        summary: None,
        categories: vec![],
    });

    let ranked = idx.ranked_entries();
    assert_eq!(ranked[0].thread_id, "high");
    assert_eq!(ranked[1].thread_id, "low");
}

#[test]
fn save_and_load_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let experiences_dir = dir.path().join("experiences");
    std::fs::create_dir_all(&experiences_dir).unwrap();

    let mut idx = ExperienceIndex::default();
    idx.upsert_entry(ExperienceEntry {
        thread_id: "t1".to_string(),
        extracted_at: 123,
        source_updated_at: 120,
        usage_count: 5,
        last_used_at: Some(456),
        summary_slug: Some("slug".to_string()),
        title: None,
        summary: None,
        categories: vec!["a".to_string(), "b".to_string()],
    });
    idx.last_consolidated_at = Some(789);
    idx.save(&experiences_dir).unwrap();

    let loaded = ExperienceIndex::load(&experiences_dir);
    assert_eq!(loaded.entries.len(), 1);
    assert_eq!(loaded.entries[0].thread_id, "t1");
    assert_eq!(loaded.entries[0].usage_count, 5);
    assert_eq!(loaded.last_consolidated_at, Some(789));
}

#[test]
fn remove_entry_works() {
    let mut idx = ExperienceIndex::default();
    idx.upsert_entry(ExperienceEntry {
        thread_id: "t1".to_string(),
        extracted_at: 100,
        source_updated_at: 90,
        usage_count: 0,
        last_used_at: None,
        summary_slug: None,
        title: None,
        summary: None,
        categories: vec![],
    });
    assert!(idx.remove_entry("t1"));
    assert!(!idx.has_entry("t1"));
    assert!(!idx.remove_entry("nonexistent"));
}
