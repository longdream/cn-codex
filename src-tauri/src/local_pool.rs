use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use tracing::{info, warn};

use crate::config_system::ModelEndpointInfo;

/// Resolved endpoint ready for use by the agent.
#[derive(Debug, Clone)]
pub struct ResolvedEndpoint {
    pub endpoint_index: usize,
    pub url: String,
    pub api_key: Option<String>,
    pub wire_api: Option<String>,
}

/// Per-endpoint health tracking.
#[derive(Debug, Clone)]
struct EndpointHealth {
    fail_count: u32,
    last_failure: Option<Instant>,
}

impl EndpointHealth {
    fn new() -> Self {
        Self {
            fail_count: 0,
            last_failure: None,
        }
    }

    fn is_healthy(&self, recovery_secs: u64) -> bool {
        match self.last_failure {
            Some(t) if self.fail_count > 0 => t.elapsed().as_secs() >= recovery_secs,
            _ => true,
        }
    }
}

#[derive(Debug)]
struct PoolState {
    current_index: usize,
    health: HashMap<usize, EndpointHealth>,
}

impl PoolState {
    fn new() -> Self {
        Self {
            current_index: 0,
            health: HashMap::new(),
        }
    }

    fn with_index(initial_index: usize) -> Self {
        Self {
            current_index: initial_index,
            health: HashMap::new(),
        }
    }
}

const RECOVERY_SECS: u64 = 60;

/// Thread-safe pool resolver that selects endpoints for failover/round-robin.
///
/// Keyed by an arbitrary string (typically `"{provider_id}:{model}"`) so that
/// different models in the same provider each track their own endpoint state.
#[derive(Debug, Clone)]
pub struct PoolResolver {
    inner: Arc<Mutex<HashMap<String, PoolState>>>,
    initial_index: usize,
}

impl PoolResolver {
    pub fn new() -> Self {
        Self::with_initial_index(0)
    }

    pub fn with_initial_index(initial_index: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            initial_index,
        }
    }

    /// Pick the next healthy endpoint from `endpoints`.
    /// Uses sequential failover: always tries the first healthy endpoint.
    pub fn resolve_endpoint(
        &self,
        key: &str,
        endpoints: &[ModelEndpointInfo],
    ) -> Option<ResolvedEndpoint> {
        if endpoints.is_empty() {
            return None;
        }

        let mut guard = self.inner.lock().unwrap();
        let idx = self.initial_index;
        let state = guard
            .entry(key.to_string())
            .or_insert_with(|| PoolState::with_index(idx));

        let start = state.current_index.min(endpoints.len().saturating_sub(1));

        for offset in 0..endpoints.len() {
            let idx = (start + offset) % endpoints.len();
            let health = state.health.entry(idx).or_insert_with(EndpointHealth::new);

            if !health.is_healthy(RECOVERY_SECS) {
                continue;
            }

            state.current_index = idx;

            let ep = &endpoints[idx];
            return Some(ResolvedEndpoint {
                endpoint_index: idx,
                url: ep.url.clone(),
                api_key: ep.api_key.clone(),
                wire_api: ep.wire_api.clone(),
            });
        }

        warn!("Pool '{key}': all endpoints unhealthy, resetting health state");
        for health in state.health.values_mut() {
            health.fail_count = 0;
            health.last_failure = None;
        }
        state.current_index = 0;

        let ep = &endpoints[0];
        Some(ResolvedEndpoint {
            endpoint_index: 0,
            url: ep.url.clone(),
            api_key: ep.api_key.clone(),
            wire_api: ep.wire_api.clone(),
        })
    }

    /// Mark an endpoint as failed so the next `resolve_endpoint` call skips it.
    pub fn mark_failed(&self, key: &str, endpoint_index: usize) {
        let mut guard = self.inner.lock().unwrap();
        let state = guard.entry(key.to_string()).or_insert_with(PoolState::new);

        let health = state
            .health
            .entry(endpoint_index)
            .or_insert_with(EndpointHealth::new);
        health.fail_count = health.fail_count.saturating_add(1);
        health.last_failure = Some(Instant::now());

        info!(
            "Pool '{key}': endpoint {endpoint_index} marked failed (count={})",
            health.fail_count
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config_system::ModelEndpointInfo;

    fn make_endpoints(n: usize) -> Vec<ModelEndpointInfo> {
        (0..n)
            .map(|i| ModelEndpointInfo {
                url: format!("http://10.0.0.{i}:8080/v1"),
                label: Some(format!("Node {i}")),
                api_key: Some(format!("sk-{i}")),
                wire_api: Some("chat".to_string()),
            })
            .collect()
    }

    #[test]
    fn returns_first_healthy() {
        let resolver = PoolResolver::new();
        let eps = make_endpoints(3);

        let ep = resolver.resolve_endpoint("k", &eps).unwrap();
        assert_eq!(ep.endpoint_index, 0);
        assert_eq!(ep.url, "http://10.0.0.0:8080/v1");
    }

    #[test]
    fn skips_failed_endpoint() {
        let resolver = PoolResolver::new();
        let eps = make_endpoints(3);

        resolver.mark_failed("k", 0);
        let ep = resolver.resolve_endpoint("k", &eps).unwrap();
        assert_eq!(ep.endpoint_index, 1);
    }

    #[test]
    fn all_failed_resets_health() {
        let resolver = PoolResolver::new();
        let eps = make_endpoints(2);

        resolver.mark_failed("k", 0);
        resolver.mark_failed("k", 1);

        let ep = resolver.resolve_endpoint("k", &eps).unwrap();
        assert_eq!(ep.endpoint_index, 0);
    }

    #[test]
    fn empty_endpoints_returns_none() {
        let resolver = PoolResolver::new();
        assert!(resolver.resolve_endpoint("k", &[]).is_none());
    }

    #[test]
    fn sequential_failover_after_mark() {
        let resolver = PoolResolver::new();
        let eps = make_endpoints(3);

        let ep0 = resolver.resolve_endpoint("k", &eps).unwrap();
        assert_eq!(ep0.endpoint_index, 0);

        resolver.mark_failed("k", 0);
        let ep1 = resolver.resolve_endpoint("k", &eps).unwrap();
        assert_eq!(ep1.endpoint_index, 1);

        resolver.mark_failed("k", 1);
        let ep2 = resolver.resolve_endpoint("k", &eps).unwrap();
        assert_eq!(ep2.endpoint_index, 2);
    }

    #[test]
    fn resumes_from_initial_index() {
        let resolver = PoolResolver::with_initial_index(2);
        let eps = make_endpoints(3);

        let ep = resolver.resolve_endpoint("k", &eps).unwrap();
        assert_eq!(ep.endpoint_index, 2);
        assert_eq!(ep.url, "http://10.0.0.2:8080/v1");
    }

    #[test]
    fn initial_index_clamped_if_out_of_bounds() {
        let resolver = PoolResolver::with_initial_index(99);
        let eps = make_endpoints(3);

        let ep = resolver.resolve_endpoint("k", &eps).unwrap();
        assert_eq!(ep.endpoint_index, 2);
    }
}
