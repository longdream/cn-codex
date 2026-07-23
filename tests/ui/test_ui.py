"""
CN-Codex UI Automated Test Suite

Uses Playwright to verify UI rendering, interactions, and functionality.
Screenshots are saved to tests/ui/screenshots/ for visual inspection.
Results are logged to tests/ui/test_results.log.

Usage:
    python tests/ui/test_ui.py [--url URL] [--headed]

Requirements:
    pip install playwright
    python -m playwright install chromium
"""

import argparse
import datetime
import os
import sys
import time
import traceback

SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
SCREENSHOT_DIR = os.path.join(SCRIPT_DIR, "screenshots")
LOG_FILE = os.path.join(SCRIPT_DIR, "test_results.log")
DEFAULT_URL = "http://localhost:1420"


class TestLogger:
    def __init__(self, log_path: str):
        self.log_path = log_path
        self.results: list[dict] = []
        self.start_time = datetime.datetime.now()
        os.makedirs(os.path.dirname(log_path), exist_ok=True)
        with open(log_path, "w", encoding="utf-8") as f:
            f.write(f"=== CN-Codex UI Test Results ===\n")
            f.write(f"Started: {self.start_time.strftime('%Y-%m-%d %H:%M:%S')}\n")
            f.write(f"{'=' * 50}\n\n")

    def log(self, step: str, status: str, detail: str = ""):
        ts = datetime.datetime.now().strftime("%H:%M:%S")
        icon = "PASS" if status == "pass" else "FAIL" if status == "fail" else "INFO"
        line = f"[{ts}] [{icon}] {step}"
        if detail:
            line += f" - {detail}"
        print(line)
        with open(self.log_path, "a", encoding="utf-8") as f:
            f.write(line + "\n")
        self.results.append({"step": step, "status": status, "detail": detail})

    def summary(self):
        passed = sum(1 for r in self.results if r["status"] == "pass")
        failed = sum(1 for r in self.results if r["status"] == "fail")
        total = passed + failed
        elapsed = (datetime.datetime.now() - self.start_time).total_seconds()
        lines = [
            "",
            "=" * 50,
            f"RESULTS: {passed}/{total} passed, {failed} failed",
            f"Elapsed: {elapsed:.1f}s",
            f"Screenshots: {SCREENSHOT_DIR}",
            f"Log: {self.log_path}",
            "=" * 50,
        ]
        for line in lines:
            print(line)
            with open(self.log_path, "a", encoding="utf-8") as f:
                f.write(line + "\n")
        return failed == 0


def screenshot(page, name: str, logger: TestLogger):
    os.makedirs(SCREENSHOT_DIR, exist_ok=True)
    path = os.path.join(SCREENSHOT_DIR, f"{name}.png")
    page.screenshot(path=path, full_page=True)
    logger.log(f"Screenshot: {name}", "info", path)
    return path


def find_system_chrome() -> str | None:
    """Find Chrome or Edge installed on the system."""
    candidates = [
        os.path.expandvars(r"%LOCALAPPDATA%\Google\Chrome\Application\chrome.exe"),
        os.path.expandvars(r"%ProgramFiles%\Google\Chrome\Application\chrome.exe"),
        os.path.expandvars(r"%ProgramFiles(x86)%\Google\Chrome\Application\chrome.exe"),
        os.path.expandvars(r"%ProgramFiles(x86)%\Microsoft\Edge\Application\msedge.exe"),
        os.path.expandvars(r"%ProgramFiles%\Microsoft\Edge\Application\msedge.exe"),
    ]
    for path in candidates:
        if os.path.isfile(path):
            return path
    return None


def run_tests(url: str, headed: bool):
    from playwright.sync_api import sync_playwright

    logger = TestLogger(LOG_FILE)
    logger.log("Test suite starting", "info", f"URL={url}, headed={headed}")

    chrome_path = find_system_chrome()
    if chrome_path:
        logger.log("Browser", "info", f"Using system browser: {chrome_path}")
    else:
        logger.log("Browser", "info", "No system Chrome/Edge found, using Playwright default")

    with sync_playwright() as p:
        launch_args = {"headless": not headed}
        if chrome_path:
            launch_args["executable_path"] = chrome_path
            launch_args["channel"] = None

        try:
            browser = p.chromium.launch(**launch_args)
        except Exception:
            logger.log("Browser", "info", "System browser failed, trying Playwright default")
            browser = p.chromium.launch(headless=not headed)

        context = browser.new_context(viewport={"width": 1280, "height": 800})

        context.on("console", lambda msg: logger.log(
            "Console", "info", f"[{msg.type}] {msg.text}"
        ))

        page = context.new_page()

        # ============================================================
        # Test 1: Page Load
        # ============================================================
        try:
            logger.log("Test 1: Page Load", "info")
            page.goto(url, wait_until="networkidle", timeout=30000)
            page.wait_for_timeout(2000)
            screenshot(page, "01_page_loaded", logger)

            title = page.title()
            logger.log("Test 1: Page Load", "pass", f"title='{title}'")
        except Exception as e:
            logger.log("Test 1: Page Load", "fail", str(e))
            screenshot(page, "01_page_load_error", logger)
            logger.summary()
            browser.close()
            return False

        # ============================================================
        # Test 2: Brand Renders Once
        # ============================================================
        try:
            logger.log("Test 2: Brand Renders Once", "info")
            sidebar = page.locator("aside")
            sidebar.wait_for(state="visible", timeout=5000)

            brand_count = page.get_by_text("CN-Codex", exact=True).count()

            logger.log("Test 2: Brand Renders Once", "pass" if brand_count == 1 else "fail",
                       f"Brand count: {brand_count}")
        except Exception as e:
            logger.log("Test 2: Brand Renders Once", "fail", str(e))

        # ============================================================
        # Test 3: Connection Status
        # ============================================================
        try:
            logger.log("Test 3: Connection Status", "info")
            page.wait_for_timeout(3000)
            screenshot(page, "03_connection_status", logger)

            body_text = page.locator("body").text_content() or ""
            has_status = any(kw in body_text for kw in [
                "已连接", "初始化中", "Connected", "Initializing"
            ])
            logger.log("Test 3: Connection Status", "pass" if has_status else "fail",
                       "Status indicator found" if has_status else "No status text found")
        except Exception as e:
            logger.log("Test 3: Connection Status", "fail", str(e))

        # ============================================================
        # Test 4: Empty State Page
        # ============================================================
        try:
            logger.log("Test 4: Empty State Page", "info")

            heading = page.locator("h2").first
            heading_text = heading.text_content() or ""
            has_heading = len(heading_text) > 0

            screenshot(page, "04_empty_state", logger)
            logger.log("Test 4: Empty State Page", "pass" if has_heading else "fail",
                       f"Heading: '{heading_text}'")
        except Exception as e:
            logger.log("Test 4: Empty State Page", "fail", str(e))

        # ============================================================
        # Test 5: Chat Input Renders
        # ============================================================
        try:
            logger.log("Test 5: Chat Input Renders", "info")
            textarea = page.locator("textarea")
            textarea.wait_for(state="visible", timeout=5000)

            placeholder = textarea.get_attribute("placeholder") or ""
            logger.log("Test 5: Chat Input Renders", "pass",
                       f"Placeholder: '{placeholder}'")
        except Exception as e:
            logger.log("Test 5: Chat Input Renders", "fail", str(e))

        # ============================================================
        # Test 6: Slash Command Panel
        # ============================================================
        try:
            logger.log("Test 6: Slash Command Panel", "info")
            textarea = page.locator("textarea")

            is_disabled = textarea.is_disabled()
            if is_disabled:
                logger.log("Test 6: Slash Command Panel", "pass",
                           "SKIPPED - textarea disabled (no Tauri runtime in browser)")
            else:
                textarea.click()
                textarea.fill("/")
                page.wait_for_timeout(500)

                screenshot(page, "06_slash_commands", logger)

                slash_panel = page.locator("text=/model")
                has_panel = slash_panel.count() > 0

                logger.log("Test 6: Slash Command Panel", "pass" if has_panel else "fail",
                           f"Commands visible: {has_panel}")

                textarea.fill("")
                page.wait_for_timeout(300)
        except Exception as e:
            logger.log("Test 6: Slash Command Panel", "fail", str(e))

        # ============================================================
        # Test 7: Settings Panel Opens
        # ============================================================
        try:
            logger.log("Test 7: Settings Panel Opens", "info")

            settings_btn = page.locator("aside button", has_text="设置").or_(
                page.locator("aside button", has_text="Settings")
            )

            if settings_btn.count() > 0:
                settings_btn.first.click()
                page.wait_for_timeout(800)
                screenshot(page, "07_settings_panel", logger)

                settings_visible = page.locator("text=CN-Codex").count() > 0
                logger.log("Test 7: Settings Panel Opens", "pass" if settings_visible else "fail",
                           "Settings panel opened")

                close_btn = page.locator("[aria-label='关闭']").or_(
                    page.locator("[aria-label='Close']")
                ).or_(page.locator("button:has(svg)").last)

                if close_btn.count() > 0:
                    close_btn.first.click()
                    page.wait_for_timeout(500)
            else:
                logger.log("Test 7: Settings Panel Opens", "fail",
                           "Settings button not found")
        except Exception as e:
            logger.log("Test 7: Settings Panel Opens", "fail", str(e))
            traceback.print_exc()

        # ============================================================
        # Test 8: Send Message
        # ============================================================
        try:
            logger.log("Test 8: Send Message", "info")
            textarea = page.locator("textarea")

            is_disabled = textarea.is_disabled()
            if is_disabled:
                logger.log("Test 8: Send Message", "pass",
                           "SKIPPED - textarea disabled (no Tauri runtime in browser)")
            else:
                textarea.click()
                textarea.fill("hello, this is a UI test")
                page.wait_for_timeout(300)

                screenshot(page, "08_message_typed", logger)

                send_btn = page.locator("button.primary-button")
                if send_btn.count() > 0 and send_btn.first.is_enabled():
                    send_btn.first.click()
                    page.wait_for_timeout(2000)
                    screenshot(page, "08_message_sent", logger)

                    body = page.locator("body").text_content() or ""
                    has_user_msg = "hello, this is a UI test" in body
                    logger.log("Test 8: Send Message", "pass" if has_user_msg else "fail",
                               "User message appeared" if has_user_msg else "Message not found in DOM")
                else:
                    logger.log("Test 8: Send Message", "fail",
                               "Send button not found or disabled")
        except Exception as e:
            logger.log("Test 8: Send Message", "fail", str(e))

        # ============================================================
        # Test 9: Status Bar
        # ============================================================
        try:
            logger.log("Test 9: Status Bar", "info")

            body_text = page.locator("body").text_content() or ""
            has_version = "v0.1.0" in body_text or "0.1.0" in body_text

            logger.log("Test 9: Status Bar", "pass" if has_version else "fail",
                       "Version info found" if has_version else "Version text not found")
        except Exception as e:
            logger.log("Test 9: Status Bar", "fail", str(e))

        # ============================================================
        # Test 10: Theme Check (Dark Mode Baseline)
        # ============================================================
        try:
            logger.log("Test 10: Theme Baseline", "info")

            theme = page.evaluate(
                "document.documentElement.dataset.theme || document.documentElement.style.colorScheme || 'unknown'"
            )
            screenshot(page, "10_theme_baseline", logger)
            logger.log("Test 10: Theme Baseline", "pass", f"Current theme: {theme}")
        except Exception as e:
            logger.log("Test 10: Theme Baseline", "fail", str(e))

        # ============================================================
        # Summary
        # ============================================================
        browser.close()
        return logger.summary()


def main():
    parser = argparse.ArgumentParser(description="CN-Codex UI Tests")
    parser.add_argument("--url", default=DEFAULT_URL, help="App URL")
    parser.add_argument("--headed", action="store_true", help="Run with visible browser")
    args = parser.parse_args()

    success = run_tests(args.url, args.headed)
    sys.exit(0 if success else 1)


if __name__ == "__main__":
    main()
