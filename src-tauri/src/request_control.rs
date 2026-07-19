use std::future::Future;
use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub(crate) const RESPONSE_HEADER_TIMEOUT: Duration = Duration::from_secs(60);
pub(crate) const STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(300);
const CANCEL_POLL_INTERVAL: Duration = Duration::from_millis(25);

pub(crate) fn should_bypass_proxy(url: &str) -> bool {
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return false;
    };
    let Some(host) = parsed.host_str() else {
        return false;
    };
    let normalized = host.trim_matches(['[', ']']).to_ascii_lowercase();

    if normalized == "localhost"
        || normalized.ends_with(".localhost")
        || normalized.ends_with(".local")
        || normalized == "host.docker.internal"
    {
        return true;
    }

    match normalized.parse::<IpAddr>() {
        Ok(IpAddr::V4(address)) => {
            address.is_private()
                || address.is_loopback()
                || address.is_link_local()
                || address.is_unspecified()
        }
        Ok(IpAddr::V6(address)) => {
            address.is_loopback()
                || address.is_unique_local()
                || address.is_unicast_link_local()
                || address.is_unspecified()
        }
        Err(_) => false,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WaitOutcome<T> {
    Ready(T),
    Cancelled,
    TimedOut,
}

async fn wait_until_cancelled(cancel_flag: &AtomicBool) {
    while !cancel_flag.load(Ordering::SeqCst) {
        tokio::time::sleep(CANCEL_POLL_INTERVAL).await;
    }
}

pub(crate) async fn wait_with_cancel_and_timeout<F>(
    future: F,
    cancel_flag: &AtomicBool,
    timeout: Duration,
) -> WaitOutcome<F::Output>
where
    F: Future,
{
    tokio::pin!(future);
    tokio::select! {
        biased;
        _ = wait_until_cancelled(cancel_flag) => WaitOutcome::Cancelled,
        value = &mut future => WaitOutcome::Ready(value),
        _ = tokio::time::sleep(timeout) => WaitOutcome::TimedOut,
    }
}

pub(crate) async fn sleep_or_cancel(duration: Duration, cancel_flag: &AtomicBool) -> bool {
    tokio::select! {
        biased;
        _ = wait_until_cancelled(cancel_flag) => false,
        _ = tokio::time::sleep(duration) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::{WaitOutcome, should_bypass_proxy, sleep_or_cancel, wait_with_cancel_and_timeout};
    use std::future::pending;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    #[tokio::test]
    async fn pending_request_stops_promptly_after_cancellation() {
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let trigger = cancel_flag.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(10)).await;
            trigger.store(true, Ordering::SeqCst);
        });

        let result = tokio::time::timeout(
            Duration::from_millis(250),
            wait_with_cancel_and_timeout(
                pending::<()>(),
                cancel_flag.as_ref(),
                Duration::from_secs(30),
            ),
        )
        .await
        .expect("cancellation should wake a pending request");

        assert_eq!(result, WaitOutcome::Cancelled);
    }

    #[tokio::test]
    async fn stalled_stream_reports_idle_timeout() {
        let cancel_flag = AtomicBool::new(false);
        let result =
            wait_with_cancel_and_timeout(pending::<()>(), &cancel_flag, Duration::from_millis(10))
                .await;

        assert_eq!(result, WaitOutcome::TimedOut);
    }

    #[tokio::test]
    async fn retry_backoff_is_cancellable() {
        let cancel_flag = AtomicBool::new(true);
        let completed = sleep_or_cancel(Duration::from_secs(30), &cancel_flag).await;
        assert!(!completed);
    }

    #[test]
    fn proxy_bypass_is_limited_to_local_and_private_targets() {
        assert!(should_bypass_proxy("http://127.0.0.1:8080/v1"));
        assert!(should_bypass_proxy("http://10.20.30.40:8080/v1"));
        assert!(should_bypass_proxy("http://model-node.local:8080/v1"));
        assert!(!should_bypass_proxy(
            "https://ipsapro.isoftstone.com/thor/v1"
        ));
        assert!(!should_bypass_proxy("https://api.openai.com/v1"));
    }
}
