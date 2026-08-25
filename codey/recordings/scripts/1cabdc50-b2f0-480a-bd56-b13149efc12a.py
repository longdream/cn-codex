# -*- coding: utf-8 -*-
"""
回放脚本: recording-1cabdc50
============================
会话ID: 1cabdc50-b2f0-480a-bd56-b13149efc12a
录制名称: recording-1cabdc50
录制文件: ../1cabdc50-b2f0-480a-bd56-b13149efc12a.trace.json

流程:
  1. goto https://ipsapro.isoftstone.com/                    (cause=user)
  2. SSO 自动跳转: ipsapro → passport → feilian 登录页       (cause=redirect, 只等待)
  3. 登录页: 点击「账号密码登录」入口(第2个 item) → 输入账号/密码
     → 勾选协议 → 点击 登 录
  4. 回调自动跳回 ipsapro.isoftstone.com/portal              (cause=redirect)
  5. 门户页面鼠标移动 (容错)
"""

import json
import os
import re
import sys

from playwright.sync_api import sync_playwright, TimeoutError as PwTimeout

# ───── 运行模式 ─────
HEADLESS = os.environ.get("REPLAY_HEADLESS", "").strip().lower() in ("1", "true", "yes")
if "--headless" in sys.argv:
    HEADLESS = True

BASE_URL = "https://ipsapro.isoftstone.com/"

# ───── 选择器候选（按优先级） ─────
# 登录方式入口：录制中点击 div.item_oFCaWe7B（tagName=img）切换到「账号密码登录」模式。
# 实际页面中第 2 个 item（index 1）才是账号密码模式，其余是钉钉/WeLink/企业微信快捷登录。
ITEM_CLICK_SELS = [
    "div.item_oFCaWe7B",
    "div.content_5klQOEKn",
    "div.container_SzPl3lwf",
]

# 账号输入框
ACCOUNT_SELS = [
    "#account_input",
    '[placeholder="请输入账号"]',
    "#account input",
    "#account",
    "div.arco-form-item-control-children input",
    "input.arco-input",
]

# 密码输入框
PASSWORD_SELS = [
    "#password_input",
    '[placeholder="请输入密码"]',
    "span.arco-input-group input",
    "div.arco-input-group-wrapper input",
]

# 协议复选框
CHECKBOX_SELS = [
    "label.arco-checkbox",
    "label.arco-checkbox input",
    "div.arco-checkbox-mask",
]

# 登录按钮
LOGIN_BTN_SELS = [
    'text="登 录"',
    'text="登录"',
    "button.arco-btn.arco-btn-primary",
]

# 门户首页元素
PORTAL_SELS = [
    "#ActivArea",
    "div.navbar",
    "#g_wrapper",
    "div.row-fluid",
    "#g_content",
    "body.portal",
]


# ───── 工具函数 ─────

def log(msg):
    print(msg, flush=True)


def first_visible(page, selectors, per_try=2000):
    """返回第一个可见的 locator，未找到返回 None"""
    for sel in selectors:
        if not sel:
            continue
        try:
            loc = page.locator(sel).first
            loc.wait_for(state="visible", timeout=per_try)
            return loc
        except Exception:
            continue
    return None


def click_sel(page, selectors, label, required=True):
    """点击第一个匹配元素，3 次重试"""
    loc = first_visible(page, selectors)
    if loc is None:
        msg = f"[跳过] 未定位到「{label}」的可点击元素"
        if required:
            raise RuntimeError(msg)
        log(msg)
        return False
    last_err = None
    for attempt in range(3):
        try:
            loc.click(timeout=5000)
            return True
        except Exception as e:
            last_err = e
            page.wait_for_timeout(800)
    if required:
        raise RuntimeError(f"点击「{label}」失败: {last_err}")
    log(f"[跳过] 点击「{label}」失败: {last_err}")
    return False


def hover_any(page, selectors, label):
    """悬停，容错"""
    loc = first_visible(page, selectors, per_try=1500)
    if loc is None:
        log(f"[信息] hover「{label}」未找到元素，跳过")
        return
    try:
        loc.hover(timeout=3000)
    except Exception as e:
        log(f"[信息] hover「{label}」失败 ({e})，跳过")


def fill_stable(page, selectors, value, label):
    """先点击聚焦再 fill 最终稳定值"""
    loc = first_visible(page, selectors)
    if loc is None:
        raise RuntimeError(f"未定位到输入框「{label}」")
    loc.click(timeout=5000)
    page.wait_for_timeout(150)
    loc.fill(value, timeout=5000)
    return loc


def switch_to_account_login(page):
    """
    切换到「账号密码登录」模式。
    账号密码模式是 item 列表中的第 2 项（index 1）。
    若默认已显示账号输入框则跳过；否则遍历 item 直到 #account_input 出现。
    """
    # 若已显示，直接返回
    if first_visible(page, ACCOUNT_SELS, per_try=3000) is not None:
        return True

    items = page.locator("div.item_oFCaWe7B")
    count = items.count()
    log(f"[信息] 登录方式入口数: {count}，开始切换到账号密码模式")

    # 优先尝试第 2 个（账号密码登录模式）
    order = [1, 0, 2, 3]
    for idx in order:
        if idx >= count:
            continue
        try:
            item = items.nth(idx)
            item.scroll_into_view_if_needed(timeout=3000)
            item.click(timeout=5000)
            log(f"[信息] 已点击登录方式入口 item[{idx}]")
        except Exception as e:
            log(f"[信息] 点击 item[{idx}] 失败: {e}")
            continue
        page.wait_for_timeout(1500)

        # 关闭可能弹出的「协议与告知说明」弹窗
        try:
            cancel = page.locator('button:has-text("取消同意")').first
            if cancel.is_visible(timeout=500):
                cancel.click(timeout=3000)
                page.wait_for_timeout(500)
                log("[信息] 已关闭协议告知弹窗")
        except Exception:
            pass

        # 检查账号输入框是否出现
        if first_visible(page, ACCOUNT_SELS, per_try=3000) is not None:
            log("[信息] 账号密码登录表单已就绪")
            return True

    return False


# ───── 主流程 ─────

def main():
    try:
        sys.stdout.reconfigure(encoding="utf-8")
    except Exception:
        pass

    result = {"ok": False, "step": 0}
    step = 0
    playwright = None
    browser = None
    context = None
    page = None

    try:
        playwright = sync_playwright().start()
        browser = playwright.chromium.launch(
            headless=HEADLESS,
            args=["--disable-blink-features=AutomationControlled"],
        )
        context = browser.new_context(
            viewport={"width": 1440, "height": 900},
            locale="zh-CN",
        )
        page = context.new_page()
        page.set_default_timeout(15000)

        # ---------------------------------------------------------------
        # 步骤 1: goto（cause=user）
        # ---------------------------------------------------------------
        step = 1
        log(f"步骤 {step}：打开 {BASE_URL}（cause=user → page.goto）")
        page.goto(BASE_URL, wait_until="domcontentloaded", timeout=60000)

        # ---------------------------------------------------------------
        # 步骤 2: 等待 SSO 自动跳转至 feilian 登录页（cause=redirect, 只等待）
        # ---------------------------------------------------------------
        step = 2
        log(f"步骤 {step}：等待 SSO 自动跳转至 feilian 登录页...")
        feilian_login = False
        try:
            page.wait_for_url(
                re.compile(r"feilian\.isoftstone\.com.*login"),
                timeout=60000,
            )
            feilian_login = True
            log(f"[信息] 到达 feilian 登录页: {page.url[:80]}")
        except PwTimeout:
            log(f"[信息] 未到达 feilian 登录页 (60s 超时)，当前 URL: {page.url[:80]}")
            if "feilian" in page.url or "passport" in page.url:
                feilian_login = True
                log("[信息] 宽松匹配确认在登录页")
            elif re.search(r"ipsapro\.isoftstone\.com", page.url):
                log("[信息] 已直接进入门户，跳过登录步骤")
            else:
                for grp in [ACCOUNT_SELS, PASSWORD_SELS, LOGIN_BTN_SELS]:
                    if first_visible(page, grp, per_try=5000) is not None:
                        feilian_login = True
                        log("[信息] 通过登录控件检测确认在登录页")
                        break

        if feilian_login:
            # ---------------------------------------------------------------
            # 步骤 3: 切换到账号密码登录模式（点击登录方式入口）
            # ---------------------------------------------------------------
            step = 3
            log(f"步骤 {step}：切换到账号密码登录模式（点击登录方式入口）")
            ok_mode = switch_to_account_login(page)
            if not ok_mode:
                raise RuntimeError("未能切换到账号密码登录模式（#account_input 未出现）")

            # ---------------------------------------------------------------
            # 步骤 4: 等待账号输入框出现（页面稳定）
            # ---------------------------------------------------------------
            step = 4
            log(f"步骤 {step}：确认账号输入框可用")
            acct = first_visible(page, ACCOUNT_SELS, per_try=5000)
            if acct is None:
                raise RuntimeError("登录页账号输入框未出现")

            # ---------------------------------------------------------------
            # 步骤 5: 点击账号输入框（聚焦）
            # ---------------------------------------------------------------
            step = 5
            log(f"步骤 {step}：点击账号输入框 #account_input（聚焦）")
            click_sel(page, ACCOUNT_SELS, "账号输入框")

            # ---------------------------------------------------------------
            # 步骤 6: hover 账号输入框
            # ---------------------------------------------------------------
            step = 6
            log(f"步骤 {step}：hover 账号输入框")
            hover_any(page, ACCOUNT_SELS, "账号输入框")

            # ---------------------------------------------------------------
            # 步骤 7: 再次点击账号输入框
            # ---------------------------------------------------------------
            step = 7
            log(f"步骤 {step}：再次点击账号输入框")
            click_sel(page, ACCOUNT_SELS, "账号输入框")

            # ---------------------------------------------------------------
            # 步骤 8: 输入账号 junlong（合并所有 KEYIN）
            # ---------------------------------------------------------------
            step = 8
            log(f"步骤 {step}：输入账号 junlong（合并所有 KEYIN）")
            fill_stable(page, ACCOUNT_SELS, "junlong", "账号输入框")

            # ---------------------------------------------------------------
            # 步骤 9: Tab 切换到密码框
            # ---------------------------------------------------------------
            step = 9
            log(f"步骤 {step}：按 Tab 切换到密码框")
            page.keyboard.press("Tab")
            page.wait_for_timeout(300)

            # ---------------------------------------------------------------
            # 步骤 10: 点击密码输入框（补充聚焦）
            # ---------------------------------------------------------------
            step = 10
            log(f"步骤 {step}：点击密码输入框（补充聚焦）")
            click_sel(page, PASSWORD_SELS, "密码输入框")

            # ---------------------------------------------------------------
            # 步骤 11: 输入密码 49718751L!abcd（合并所有 KEYIN）
            # ---------------------------------------------------------------
            step = 11
            log(f"步骤 {step}：输入密码（合并所有 KEYIN）")
            fill_stable(page, PASSWORD_SELS, "49718751L!abcd", "密码输入框")

            # ---------------------------------------------------------------
            # 步骤 12: 勾选协议复选框（合并录制中 2 次近邻 click，最终为勾选态）
            # ---------------------------------------------------------------
            step = 12
            log(f"步骤 {step}：勾选用户协议复选框")
            click_sel(page, CHECKBOX_SELS, "协议复选框")
            page.wait_for_timeout(300)

            # 校验勾选态，若未勾选再点一次
            cb_checked = False
            try:
                if page.locator("label.arco-checkbox input").first.is_checked():
                    cb_checked = True
            except Exception:
                pass
            if not cb_checked:
                log("[信息] 复选框未勾选，再点一次")
                click_sel(page, CHECKBOX_SELS, "协议复选框（第二次）")
                page.wait_for_timeout(300)
                try:
                    cb_checked = page.locator("label.arco-checkbox input").first.is_checked()
                except Exception:
                    pass
            if not cb_checked:
                log("[警告] 复选框最终状态可能未勾选，继续尝试登录")

            # ---------------------------------------------------------------
            # 步骤 13: hover 协议复选框
            # ---------------------------------------------------------------
            step = 13
            log(f"步骤 {step}：hover 协议复选框")
            hover_any(page, CHECKBOX_SELS, "协议复选框")

            # ---------------------------------------------------------------
            # 步骤 14: hover 登录按钮
            # ---------------------------------------------------------------
            step = 14
            log(f"步骤 {step}：hover 登录按钮「登 录」")
            hover_any(page, LOGIN_BTN_SELS, "登 录")

            # ---------------------------------------------------------------
            # 步骤 15: 点击登录按钮
            # ---------------------------------------------------------------
            step = 15
            log(f"步骤 {step}：点击登录按钮「登 录」")
            click_sel(page, LOGIN_BTN_SELS, "登 录 按钮")

            # ---------------------------------------------------------------
            # 步骤 16: 等待登录回调跳转回门户（cause=redirect, 只等待）
            # ---------------------------------------------------------------
            step = 16
            log(f"步骤 {step}：等待登录成功回跳 ipsapro...")
            try:
                page.wait_for_url(
                    re.compile(r"ipsapro\.isoftstone\.com"),
                    timeout=90000,
                )
                log(f"[信息] 已跳转到门户: {page.url[:80]}")
            except PwTimeout:
                try:
                    page.screenshot(path="replay-error-login.png")
                except Exception:
                    pass
                raise RuntimeError(
                    f"登录后未跳转到门户（90s 超时），当前 URL: {page.url[:80]}"
                )

        # ---------------------------------------------------------------
        # 步骤 17: 验证门户页面已加载
        # ---------------------------------------------------------------
        step = 17
        log(f"步骤 {step}：验证门户页面初始化元素")
        portal_loc = first_visible(page, PORTAL_SELS, per_try=3000)
        if portal_loc is None:
            page.wait_for_timeout(5000)
            portal_loc = first_visible(page, PORTAL_SELS, per_try=3000)
            if portal_loc is None:
                raise RuntimeError("门户页面元素未加载完成")
        log(f"[信息] 门户页面加载完成: {page.url}")

        # ---------------------------------------------------------------
        # 步骤 18-21: 门户页面鼠标移动（容错）
        # ---------------------------------------------------------------
        portal_hover_sels = [
            (["#g_loading", "div.modal-backdrop", "html.js.flexbox"], "加载指示器"),
            (["div.row-fluid", "#g_wrapper", "div.container-fluid"], "页面主体"),
            (["#ActivArea", "div.span12", "div.row-fluid", "#g_content"], "活动区域"),
            (["div.navbar", "div.row-fluid", "#g_footer", "div.container-fluid"], "顶部导航栏"),
        ]
        for i, (sels, desc) in enumerate(portal_hover_sels, start=18):
            step = i
            log(f"步骤 {step}：鼠标移动到门户页面「{desc}」（容错）")
            hover_any(page, sels, desc)

        # ---------------------------------------------------------------
        # 成功
        # ---------------------------------------------------------------
        step = 22
        result = {
            "ok": True,
            "step": step,
            "url": page.url,
            "title": page.title(),
        }
        log("回放成功！")

    except Exception as exc:
        result = {
            "ok": False,
            "step": step,
            "error": f"{type(exc).__name__}: {exc}",
        }
        log(f"回放失败: {exc}")

    finally:
        log("REPLAY_RESULT " + json.dumps(result, ensure_ascii=False))
        if context is not None:
            try:
                context.close()
            except Exception:
                pass
        if browser is not None:
            try:
                browser.close()
            except Exception:
                pass
        if playwright is not None:
            try:
                playwright.stop()
            except Exception:
                pass

    if not result.get("ok", False):
        sys.exit(1)


if __name__ == "__main__":
    main()