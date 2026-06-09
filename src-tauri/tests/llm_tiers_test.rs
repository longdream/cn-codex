use cn_codex_lib::llm_tiers::{LlmTiersManager, TierLevel, TokenBudget};

#[test]
fn default_manager_has_three_tiers() {
    let mgr = LlmTiersManager::default();
    assert_eq!(mgr.tiers.len(), 3);
    assert!(mgr.tiers.contains_key(&TierLevel::Low));
    assert!(mgr.tiers.contains_key(&TierLevel::Medium));
    assert!(mgr.tiers.contains_key(&TierLevel::High));
}

#[test]
fn default_tier_is_medium() {
    let mgr = LlmTiersManager::default();
    assert_eq!(mgr.default_tier, TierLevel::Medium);
}

#[test]
fn resolve_tier_for_chat_returns_low() {
    let mgr = LlmTiersManager::default();
    let tier = mgr.resolve_tier(Some("chat"));
    assert_eq!(tier.model, "gpt-4.1-mini");
}

#[test]
fn resolve_tier_for_explain_returns_low() {
    let mgr = LlmTiersManager::default();
    let tier = mgr.resolve_tier(Some("explain"));
    assert_eq!(tier.reasoning_effort, "low");
}

#[test]
fn resolve_tier_for_code_edit_returns_medium() {
    let mgr = LlmTiersManager::default();
    let tier = mgr.resolve_tier(Some("code_edit"));
    assert_eq!(tier.model, "gpt-4.1");
}

#[test]
fn resolve_tier_for_architecture_returns_high() {
    let mgr = LlmTiersManager::default();
    let tier = mgr.resolve_tier(Some("architecture"));
    assert_eq!(tier.model, "o3");
    assert_eq!(tier.service_tier, "priority");
}

#[test]
fn resolve_tier_for_unknown_scene_uses_default() {
    let mgr = LlmTiersManager::default();
    let tier = mgr.resolve_tier(Some("unknown_scene_xyz"));
    assert_eq!(tier.model, "gpt-4.1");
}

#[test]
fn resolve_tier_with_none_uses_default() {
    let mgr = LlmTiersManager::default();
    let tier = mgr.resolve_tier(None);
    assert_eq!(tier.model, "gpt-4.1");
}

#[test]
fn token_usage_starts_at_zero() {
    let mgr = LlmTiersManager::default();
    let stats = mgr.get_usage_stats();
    assert_eq!(stats.total_tokens, 0);
}

#[test]
fn record_usage_accumulates() {
    let mgr = LlmTiersManager::default();
    mgr.record_usage(100);
    mgr.record_usage(250);
    let stats = mgr.get_usage_stats();
    assert_eq!(stats.total_tokens, 350);
}

#[test]
fn token_budget_default_values() {
    let budget = TokenBudget::default();
    assert_eq!(budget.daily_limit, 0);
    assert!((budget.warning_threshold - 0.8).abs() < f64::EPSILON);
    assert!(budget.auto_downgrade);
}

#[test]
fn scene_bindings_complete() {
    let mgr = LlmTiersManager::default();
    let low_scenes = ["chat", "explain", "translate"];
    let medium_scenes = ["code_edit", "bug_fix", "test_write", "refactor"];
    let high_scenes = ["architecture", "code_review", "complex_debug", "plan"];

    for s in low_scenes {
        assert_eq!(
            mgr.scenes.get(s),
            Some(&TierLevel::Low),
            "scene '{s}' should be Low"
        );
    }
    for s in medium_scenes {
        assert_eq!(
            mgr.scenes.get(s),
            Some(&TierLevel::Medium),
            "scene '{s}' should be Medium"
        );
    }
    for s in high_scenes {
        assert_eq!(
            mgr.scenes.get(s),
            Some(&TierLevel::High),
            "scene '{s}' should be High"
        );
    }
}
