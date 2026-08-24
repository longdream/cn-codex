#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
回放脚本：recording-e708fc40
===================================================================
根据录制记录 ../e708fc40-fb28-40f0-b811-5151cced293a.trace.json 生成的
Playwright（Python sync API）回放脚本，可独立运行。

录制流程概要（2026-08-23 02:01 UTC，约 50 秒）：
  1. 打开起点页 https://www.modelscope.cn/home（录制起始页，容错）
  2. 访问 IPSA Pro 门户 https://ipsapro.isoftstone.com/
  3. SSO 跳转链：passport.isoftstone.com → 飞连登录页 feilian.isoftstone.com:10443/login
  4. 在登录页点击顶部提示工具条项目（Arco tooltip，容错）
  5. 点击账号输入框 #account_input
  6. 输入账号 junlong（录制值，可用环境变量 IPSA_ACCOUNT 覆盖）
  7. 输入密码（录制值，回放直接填最终值；可用环境变量 IPSA_PASSWORD 覆盖）
  8. 勾选"同意协议"复选框（Arco 组件：arco-checkbox-mask）
  9. 点击登录按钮 button.arco-btn.arco-btn-primary，提交表单
 10. OIDC 回调 passport.isoftstone.com/corplink/agw/callback → 门户 /portal

依赖安装：
    pip install playwright
    playwright install chromium

运行方式：
    python e708fc40-fb28-40f0-b811-5151cced293a.py
    # 建议用环境变量覆盖账号密码（避免脚本明文包含敏感信息）：
    #   IPSA_ACCOUNT=xxx IPSA_PASSWORD=xxx python e708fc40-fb28-40f0-b811-5151cced293a.py
    # 无头模式：REPLAY_HEADLESS=1  python e708fc40-fb28-40f0-b811-5151cced293a.py

退出码约定：0 = 回放成功；非 0 = 回放失败。
失败时 stdout 输出一行 REPLAY_RESULT JSON：
    {"ok": false, "recording": "...", "step": N, "step_desc": "...",
     "url": "...", "title": "...", "error": "ExceptionType: message"}
同时会在当前目录保存一张失败截图 recording-e708fc40-failure.png（尽力而为）。
"""

import json
import os
import re
import sys
import time
from typing import Callable, List, Optional, Tuple

from playwright.sync_api import sync_playwright

# Playwright 异常兼容：1.62 版本起 PlaywrightTimeoutError 重命名为 TimeoutError
try:
    from playwright.sync_api import TimeoutError as PlaywrightTimeoutError
except ImportError:  # pragma: no cover - 旧版本兼容
    from playwright.sync_api import PlaywrightTimeoutError  # type: ignore[no-redef]

# ====================================================================
# 可配置常量（可通过环境变量覆盖）
# ====================================================================

RECORDING_NAME = "recording-e708fc40"

# 录制起点页（容错，失败仅警告）
START_URL = "https://www.modelscope.cn/home"

# 门户入口（SSO 跳转链的起点）
PORTAL_URL = "https://ipsapro.isoftstone.com/"

# 飞连登录页根 URL（兜底用，不带已过期的 OIDC state 参数）
LOGIN_URL = "https://feilian.isoftstone.com:10443/login"

# 账号密码（录制值；建议通过环境变量覆盖，避免明文硬编码）
ACCOUNT = os.environ.get("IPSA_ACCOUNT", "junlong")
PASSWORD = os.environ.get("IPSA_PASSWORD", "49718751L!abcd")

# 浏览器模式：默认有头（headed），设 REPLAY_HEADLESS=1 切到无头
HEADLESS = os.environ.get("REPLAY_HEADLESS", "0").strip().lower() in ("1", "true", "yes")

# 全局超时（毫秒），默认 30 秒
TIMEOUT = int(os.environ.get("REPLAY_TIMEOUT_MS", "30000"))

# ====================================================================
# 选择器候选列表（多候选回退，提高脚本健壮性）
# ====================================================================

# 登录表单
LOGIN_FORM = [
    "form.arco-form",
    "form",
    "#account_input",
    "div.container_ntEtTuSM",
]

# 账号输入框
ACCOUNT_INPUT = [
    "#account_input",
    "[placeholder='请输入账号']",
    "input[type='text'][placeholder='请输入账号']",
    "input[type='text']",
]

# 密码输入框
PASSWORD_INPUT = [
    "#password_input",
    "[placeholder='请输入密码']",
    "input[type='password'][placeholder='请输入密码']",
    "input[type='password']",
]

# 账号密码登录入口（"更多登录方式"中的图标，点击后展开账号密码表单）
ACCOUNT_LOGIN_ENTRY = [
    ".content_5klQOEKn > .item_oFCaWe7B:nth-child(2)",
    ".item_oFCaWe7B:nth-child(2)",
    ".item_oFCaWe7B",
]

# 顶部提示工具条项目（录制中为 div.item_oFCaWe7B.arco-tooltip-open，容错步骤）
TOOLTIP_ITEM = [
    "div.item_oFCaWe7B.arco-tooltip-open",
    "div.item_oFCaWe7B",
    ".container_SzPl3lwf .item_oFCaWe7B",
]

# 协议复选框（Arco Design 组件）
CHECKBOX = [
    "div.arco-checkbox-mask",
    "label.arco-checkbox:has-text('同意')",
    "label.arco-checkbox",
    "input[type='checkbox']",
]

# 登录按钮
LOGIN_BUTTON = [
    "button:has-text('登 录')",
    "button:has-text('登录')",
    "button.arco-btn.arco-btn-primary:not(:has-text('一键登录'))",
    "button.arco-btn.arco-btn-primary",
    "button[type='submit']",
]

# ====================================================================
# 工具函数
# ====================================================================

# 当前步骤上下文（用于异常时输出结构化失败信息）
_CURRENT = {"step": 0, "desc": ""}


def _safe(fn: Callable) -> str:
    """安全执行函数，异常时返回空字符串。"""
    try:
        return fn()
    except Exception:
        return ""


def run_step(n: int, desc: str, fn: Callable) -> None:
    """步骤包装器：打印起始/结束标记，并更新 _CURRENT 上下文。"""
    _CURRENT["step"] = n
    _CURRENT["desc"] = desc
    print(f"步骤 {n}：{desc}……", flush=True)
    fn()
    print(f"步骤 {n}：{desc} —— 完成", flush=True)


def retry(fn: Callable, attempts: int = 3, interval: float = 2.0) -> None:
    """带重试的通用执行器，两次重试之间休眠 interval 秒。"""
    last_err = None
    for i in range(attempts):
        try:
            fn()
            return
        except Exception as e:  # noqa: BLE001
            last_err = e
            if i < attempts - 1:
                print(f"  重试 {i + 1}/{attempts}：{type(e).__name__}: {e}", flush=True)
                time.sleep(interval)
    raise last_err  # type: ignore[misc]


def wait_page_stable(page, timeout_ms: int = 10000) -> None:
    """等待页面稳定：优先 networkidle，退化到 domcontentloaded + 短眠。"""
    try:
        page.wait_for_load_state("networkidle", timeout=timeout_ms)
    except PlaywrightTimeoutError:
        pass
    try:
        page.wait_for_load_state("domcontentloaded", timeout=timeout_ms)
    except PlaywrightTimeoutError:
        pass
    time.sleep(1.0)


def wait_any(page, selectors: List[str], state: str = "visible", timeout: int = 0) -> str:
    """
    从候选选择器中等待第一个可见元素。
    返回匹配到的选择器字符串；若全部超时则抛出 RuntimeError。
    """
    timeout = timeout or TIMEOUT
    for sel in selectors:
        try:
            page.wait_for_selector(sel, state=state, timeout=timeout)
            return sel
        except PlaywrightTimeoutError:
            continue
    raise RuntimeError(f"候选元素均未出现（state={state}）：{selectors}")


def click_first(page, selectors: List[str], timeout: int = 0) -> str:
    """从候选选择器中点击第一个可见元素；全部不可点击时抛出 RuntimeError。"""
    timeout = timeout or TIMEOUT
    for sel in selectors:
        try:
            loc = page.locator(sel).first
            loc.wait_for(state="visible", timeout=timeout)
            loc.click(timeout=timeout)
            return sel
        except Exception:
            continue
    raise RuntimeError(f"候选元素均不可点击：{selectors}")


def fill_first(page, selectors: List[str], value: str, timeout: int = 0) -> str:
    """从候选选择器中向第一个可见输入框填入文本；填入前先 click 聚焦。全部不可用时抛出 RuntimeError。"""
    timeout = timeout or TIMEOUT
    for sel in selectors:
        try:
            loc = page.locator(sel).first
            loc.wait_for(state="visible", timeout=timeout)
            # 先点击聚焦，再填入文本（确保 type/fill 前有 click/focus）
            try:
                loc.click(timeout=timeout)
            except Exception:
                pass
            loc.fill(value, timeout=timeout)
            return sel
        except Exception:
            continue
    raise RuntimeError(f"候选元素均无法输入文本：{selectors}")


def assert_url_contains(page, *fragments: str) -> str:
    """
    断言当前 URL 包含至少一个指定片段。
    返回当前 URL 字符串。
    """
    url = page.url
    if not any(f in url for f in fragments):
        raise AssertionError(f"URL 断言失败：期望包含 {fragments}，实际 {url}")
    return url


def assert_input_value_contains(page, selectors: List[str], expected: str) -> str:
    """
    断言候选输入框的当前值包含期望文本。
    返回匹配到的选择器字符串。
    """
    for sel in selectors:
        try:
            loc = page.locator(sel).first
            if not loc.is_visible():
                continue
            value = loc.input_value()
            if value == expected or expected in value:
                return sel
        except Exception:
            continue
    raise AssertionError(f"输入框值断言失败：期望 {expected!r}，候选 {selectors}")


def ensure_checkbox_checked(page, selectors: List[str], timeout: int = 0) -> str:
    """
    勾选协议复选框：优先原生 checkbox，然后 Arco mask，最后 JS 兜底。
    返回匹配到的选择器字符串。
    """
    timeout = timeout or TIMEOUT

    for sel in selectors:
        try:
            loc = page.locator(sel).first
            loc.wait_for(state="visible", timeout=6000)
            tag = (loc.evaluate("el => el.tagName.toLowerCase()") or "").strip()
            is_input = tag == "input"

            if is_input:
                # 原生 checkbox 输入框
                if not loc.is_checked():
                    try:
                        loc.check(timeout=timeout)
                    except Exception:
                        # 不可见时强制 JS 点击
                        loc.evaluate("el => el.click()")
                    time.sleep(0.3)
            else:
                # Arco 组件：先判断容器是否已勾选，避免重复点击导致取消
                need_click = not page.evaluate(
                    """(q) => {
                        try {
                            const m = document.querySelector(q);
                            if (!m) return false;
                            const cb = m.closest('.arco-checkbox');
                            return cb && cb.classList.contains('arco-checkbox-checked');
                        } catch(e) { return false; }
                    }""",
                    sel,
                )
                if need_click:
                    loc.click(timeout=timeout)
                    time.sleep(0.5)

            # 校验：原生已勾选 或 Arco 容器有 checked 类
            checked = page.evaluate(
                """() => {
                    const inputs = Array.from(document.querySelectorAll('input[type=checkbox]'));
                    const native = inputs.some(el => el.checked);
                    const cls = !!document.querySelector('.arco-checkbox-checked');
                    return native || cls;
                }"""
            )
            if checked:
                return sel
        except Exception:
            continue

    # 兜底：JS 直接勾选所有未选中的可见复选框
    ok = page.evaluate(
        """() => {
            const els = Array.from(document.querySelectorAll('input[type=checkbox]'))
                .filter(el => !el.disabled);
            if (!els.length) return false;
            els.forEach(el => { if (!el.checked) el.click(); });
            return els.some(el => el.checked);
        }"""
    )
    if not ok:
        raise AssertionError("协议复选框未能勾选（未检测到已勾选状态）")
    return "js-fallback"


# ====================================================================
# 主流程编排
# ====================================================================

def run_flow(page) -> None:
    """Playwright 页面操作序列，对应录制记录中的每一步。"""

    # ------------------------------------------------------------------
    # 步骤 1：打开录制起点页 modelscope（容错：失败仅警告，不中断流程）
    # ------------------------------------------------------------------
    try:
        run_step(
            1,
            "打开起点页 https://www.modelscope.cn/home（录制起点，容错）",
            lambda: retry(
                lambda: (
                    page.goto(START_URL, timeout=60000, wait_until="domcontentloaded"),
                    wait_page_stable(page),
                ),
                attempts=2,
                interval=2,
            ),
        )
    except Exception as e:
        print(f"  步骤 1 已跳过（起点页不可用）：{type(e).__name__}: {e}", flush=True)

    # ------------------------------------------------------------------
    # 步骤 2：打开 IPSA Pro 门户，跟随 SSO 跳转链
    #   ipsapro → passport → 飞连登录页
    #   如跳转链中断，兜底直接打开飞连登录页
    # ------------------------------------------------------------------
    def _goto_portal():
        page.goto(PORTAL_URL, timeout=60000, wait_until="domcontentloaded")
        try:
            page.wait_for_url(
                re.compile(r"feilian\.isoftstone\.com|passport\.isoftstone\.com"),
                timeout=45000,
            )
        except PlaywrightTimeoutError:
            # 兜底：直接打开飞连登录页（不带过期 OIDC state 参数）
            print("  跳转链未自动完成，兜底直接打开飞连登录页", flush=True)
            page.goto(LOGIN_URL, timeout=60000, wait_until="domcontentloaded")
            page.wait_for_url(re.compile(r"feilian\.isoftstone\.com"), timeout=30000)
        wait_page_stable(page)

    run_step(
        2,
        "打开门户入口并等待 SSO 跳转（ipsapro → passport → 飞连登录页）",
        lambda: retry(_goto_portal, attempts=3, interval=3),
    )
    print(f"  当前 URL：{page.url}", flush=True)

    # ------------------------------------------------------------------
    # 步骤 3：等待登录页面加载完成，断言位于飞连登录域名
    #   注意：页面默认显示"一键登录"模式，账号密码表单尚未出现
    # ------------------------------------------------------------------
    def _check_login_form():
        # 等待页面基本结构渲染（复选框或一键登录按钮作为页面就绪标志）
        try:
            page.wait_for_selector("div.arco-checkbox-mask", state="visible", timeout=TIMEOUT)
        except PlaywrightTimeoutError:
            # 退化：等待 body 可见即可
            page.locator("body").first.wait_for(state="visible", timeout=TIMEOUT)
        # 断言仍在飞连登录域
        assert_url_contains(page, "feilian.isoftstone.com")

    run_step(3, "等待登录页面加载并校验（飞连登录域名）", _check_login_form)

    # ------------------------------------------------------------------
    # 步骤 4：切换到账号密码登录模式
    #   飞连登录页默认显示"一键登录"，需要点击"更多登录方式"中的
    #   账号密码登录图标才能展开账号密码输入表单
    # ------------------------------------------------------------------
    def _switch_to_account_login():
        # 检查账号输入框是否已可见（可能页面默认就是账号密码模式）
        try:
            acct = page.locator("#account_input").first
            if acct.is_visible(timeout=3000):
                print("  账号密码登录表单已可见，无需切换", flush=True)
                return
        except Exception:
            pass
        # 点击"更多登录方式"中的账号密码登录图标
        click_first(page, ACCOUNT_LOGIN_ENTRY, timeout=10000)
        # 等待账号输入框出现
        wait_any(page, ACCOUNT_INPUT, timeout=TIMEOUT)
        time.sleep(1.0)  # 等待表单动画完成

    run_step(4, "切换到账号密码登录模式", _switch_to_account_login)

    # ------------------------------------------------------------------
    # 步骤 5：点击账号输入框以聚焦
    # ------------------------------------------------------------------
    run_step(5, "点击账号输入框聚焦", lambda: click_first(page, ACCOUNT_INPUT))

    # ------------------------------------------------------------------
    # 步骤 6：输入账号
    # ------------------------------------------------------------------
    run_step(
        6,
        f"输入账号（{ACCOUNT}）",
        lambda: fill_first(page, ACCOUNT_INPUT, ACCOUNT),
    )

    # ------------------------------------------------------------------
    # 步骤 7：校验账号输入框内容
    # ------------------------------------------------------------------
    run_step(
        7,
        "校验账号输入框内容",
        lambda: assert_input_value_contains(page, ACCOUNT_INPUT, ACCOUNT),
    )

    # ------------------------------------------------------------------
    # 步骤 8：点击密码输入框聚焦，然后输入密码
    #   录制中分两次输入，回放直接填入最终值
    # ------------------------------------------------------------------
    def _fill_password():
        # 先点击密码输入框以聚焦（确保 type/fill 前有 click/focus）
        click_first(page, PASSWORD_INPUT)
        time.sleep(0.3)
        fill_first(page, PASSWORD_INPUT, PASSWORD)

    run_step(8, "点击密码输入框聚焦并输入密码", _fill_password)

    # ------------------------------------------------------------------
    # 步骤 9：校验密码输入框内容
    # ------------------------------------------------------------------
    run_step(
        9,
        "校验密码输入框内容",
        lambda: assert_input_value_contains(page, PASSWORD_INPUT, PASSWORD),
    )

    # ------------------------------------------------------------------
    # 步骤 10：勾选协议复选框（录制中 arco-checkbox-mask + input 两次点击，
    #   回放合并为一次可靠勾选：先判断是否已勾选，再按需点击，最后 JS 兜底）
    # ------------------------------------------------------------------
    run_step(
        10,
        "勾选「同意协议」复选框",
        lambda: ensure_checkbox_checked(page, CHECKBOX),
    )

    # ------------------------------------------------------------------
    # 步骤 11：点击登录按钮，提交表单
    #   按钮点击后，飞连会发起 OIDC 认证请求，浏览器自动跟随跳转
    # ------------------------------------------------------------------
    def _submit_login():
        click_first(page, LOGIN_BUTTON)
        # 等待表单提交引发的首次页面跳转
        try:
            page.wait_for_load_state("domcontentloaded", timeout=15000)
        except PlaywrightTimeoutError:
            pass

    run_step(11, "点击登录按钮并提交表单", _submit_login)

    # ------------------------------------------------------------------
    # 步骤 12：等待 OIDC 回调跳转（feilian → passport callback）
    #   回调 URL 中携带一次性的 code 和 state 参数
    # ------------------------------------------------------------------
    def _wait_callback():
        page.wait_for_url(
            re.compile(r"passport\.isoftstone\.com"),
            timeout=60000,
        )

    run_step(
        12,
        "等待 OIDC 回调跳转（feilian → passport.isoftstone.com/callback）",
        lambda: retry(_wait_callback, attempts=2, interval=3),
    )
    print(f"  回调 URL：{page.url}", flush=True)

    # ------------------------------------------------------------------
    # 步骤 13：等待进入门户 /portal 并断言页面正常
    # ------------------------------------------------------------------
    def _wait_portal():
        # 等待 URL 中包含门户地址
        page.wait_for_url(
            re.compile(r"ipsapro\.isoftstone\.com"),
            timeout=60000,
        )
        wait_page_stable(page)
        # 断言页面主体可见（说明页面正常渲染）
        page.locator("body").first.wait_for(state="visible", timeout=15000)
        # 断言 URL 包含 ipsapro 域名
        assert_url_contains(page, "ipsapro.isoftstone.com")

    run_step(
        13,
        "等待进入门户页面 /portal 并断言页面正常",
        lambda: retry(_wait_portal, attempts=2, interval=3),
    )
    print(f"  门户 URL：{page.url}", flush=True)
    print(f"  页面标题：{_safe(lambda: page.title())}", flush=True)

    # 全部步骤完成


def main() -> int:
    """主入口：启动浏览器，执行回放，返回退出码。"""
    # 确保 stdout/stderr 支持 UTF-8 输出，避免 GBK 无法编码 emoji 等 Unicode 字符
    try:
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    except Exception:
        pass
    try:
        sys.stderr.reconfigure(encoding="utf-8", errors="replace")
    except Exception:
        pass

    playwright = None
    browser = None
    context = None
    page = None
    exit_code = 1
    try:
        playwright = sync_playwright().start()
        # 启动 Chromium
        browser = playwright.chromium.launch(headless=HEADLESS)
        # 创建上下文：中文环境，宽屏视口，忽略 HTTPS 证书错误（适用于内网自签名证书）
        context = browser.new_context(
            ignore_https_errors=True,
            viewport={"width": 1440, "height": 900},
            locale="zh-CN",
        )
        page = context.new_page()
        page.set_default_timeout(TIMEOUT)

        try:
            run_flow(page)
            print("[OK] 回放成功：已进入 IPSA Pro 门户", flush=True)
            print(
                "REPLAY_RESULT "
                + json.dumps(
                    {
                        "ok": True,
                        "recording": RECORDING_NAME,
                        "step": "done",
                        "url": _safe(lambda: page.url),
                        "title": _safe(lambda: page.title()),
                        "error": None,
                    },
                    ensure_ascii=False,
                ),
                flush=True,
            )
            exit_code = 0
        except Exception as e:  # noqa: BLE001
            payload = {
                "ok": False,
                "recording": RECORDING_NAME,
                "step": _CURRENT["step"],
                "step_desc": _CURRENT["desc"],
                "url": _safe(lambda: page.url),
                "title": _safe(lambda: page.title()),
                "error": f"{type(e).__name__}: {e}",
            }
            print("REPLAY_RESULT:" + json.dumps(payload, ensure_ascii=False), flush=True)
            try:
                page.screenshot(
                    path=f"{RECORDING_NAME}-failure.png",
                    timeout=5000,
                )
            except Exception:
                pass
            exit_code = 1
    except Exception as e:  # noqa: BLE001
        # 启动或拆浏览器阶段异常：若流程已成功打印结果，仍按成功退出。
        if exit_code == 0:
            return 0
        print(
            "REPLAY_RESULT:"
            + json.dumps(
                {
                    "ok": False,
                    "recording": RECORDING_NAME,
                    "step": _CURRENT["step"],
                    "step_desc": _CURRENT["desc"],
                    "url": "",
                    "title": "",
                    "error": f"{type(e).__name__}: {e}",
                },
                ensure_ascii=False,
            ),
            flush=True,
        )
        return 1
    finally:
        for closer in (
            getattr(context, "close", None),
            getattr(browser, "close", None),
            getattr(playwright, "stop", None),
        ):
            if closer is None:
                continue
            try:
                closer()
            except Exception:
                pass
    return exit_code


if __name__ == "__main__":
    sys.exit(main())