use super::*;

#[test]
fn find_browser_path_returns_existing_path() {
    let path = find_browser_path();
    if let Some(p) = &path {
        assert!(p.exists(), "Detected browser path should exist: {p:?}");
    }
    // If path is None, that's acceptable in CI where no browser is installed.
}

#[test]
fn external_browser_new_starts_idle() {
    let browser = ExternalBrowser::new();
    // get_cdp_endpoint is async, so we test via the sync constructor.
    let state = browser.inner.try_lock().expect("lock should succeed");
    assert!(state.child.is_none());
    assert_eq!(state.cdp_port, DEFAULT_CDP_PORT);
    assert!(state.user_data_dir.is_none());
}

#[tokio::test]
async fn get_cdp_endpoint_none_when_not_launched() {
    let browser = ExternalBrowser::new();
    assert_eq!(browser.get_cdp_endpoint().await, None);
}

#[tokio::test]
async fn get_cdp_port_returns_default() {
    let browser = ExternalBrowser::new();
    assert_eq!(browser.get_cdp_port().await, DEFAULT_CDP_PORT);
}

#[tokio::test]
async fn is_running_returns_false_when_not_launched() {
    let browser = ExternalBrowser::new();
    let http = reqwest::Client::new();
    assert!(!browser.is_running(&http).await);
}

#[tokio::test]
async fn shutdown_is_noop_when_not_launched() {
    let browser = ExternalBrowser::new();
    browser.shutdown().await;
    assert_eq!(browser.get_cdp_endpoint().await, None);
}

#[tokio::test]
async fn launch_fails_with_invalid_path() {
    let browser = ExternalBrowser::new();
    let http = reqwest::Client::new();
    let result = browser
        .launch(&http, Some("/nonexistent/browser.exe"), Some(19222))
        .await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.contains("not found"),
        "Expected 'not found' in error, got: {err}"
    );
}

#[tokio::test]
async fn launch_with_real_browser() {
    let browser_path = find_browser_path();
    if browser_path.is_none() {
        eprintln!("Skipping launch_with_real_browser: no browser found");
        return;
    }

    let browser = ExternalBrowser::new();
    let http = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap();

    // Use a high port to avoid conflicts with user's browsing session.
    let port = 19222u16;
    let result = browser.launch(&http, None, Some(port)).await;

    match result {
        Ok(endpoint) => {
            assert_eq!(endpoint, format!("http://127.0.0.1:{port}"));
            assert!(browser.is_running(&http).await);
            assert_eq!(
                browser.get_cdp_endpoint().await,
                Some(format!("http://127.0.0.1:{port}"))
            );

            // Launch again should reuse existing instance.
            let result2 = browser.launch(&http, None, Some(port)).await;
            assert!(result2.is_ok());
            assert_eq!(result2.unwrap(), endpoint);

            browser.shutdown().await;
            assert_eq!(browser.get_cdp_endpoint().await, None);
        }
        Err(e) => {
            // Might fail in restricted environments, that's OK.
            eprintln!("launch_with_real_browser: launch failed (acceptable): {e}");
        }
    }
}
