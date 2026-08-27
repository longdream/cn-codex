# -*- coding: utf-8 -*-
"""
回放脚本（由录制文件 0da42eab-aa75-49f7-b62b-8bb74ef0ea21.trace.json 生成）
录制名称：recording-0da42eab

流程：
  1. SSO 登录页 (ipsademo.isoftstone.com/passport) —— 输入用户名 yiyuh，点击 Login
  2. 自动跳转 /Oidc -> /preaudit（预审页）
  3. 预审页：点击上传区域 -> 选择业务组 CORP -> 上传合同 docx -> 点击"开始预审"
  4. 跳转预审详情页 /preaudit/107 -> 刷新 -> 点击"规则库配置"页签
  5. 规则库页面 /rules

脚本要求：Playwright sync API、有界面浏览器 (headless=False)、每步中文注释与打印、
        多候选选择器回退、每步重试、导航后等待页面稳定、结束打印 REPLAY_RESULT。
"""

import json
import os
import re
import sys
import time

from playwright.sync_api import sync_playwright
from playwright.sync_api import TimeoutError as PwTimeoutError

# ---------------------------------------------------------------------------
# 常量：登录页 URL、上传文件路径
# 说明：录制以新标签页开始，首个 navigate 是 cause=redirect（用户输入地址后由
#       SSO 自动跳转到登录页）。回放时直接打开该登录页作为起点。
# ---------------------------------------------------------------------------
LOGIN_URL = (
    "https://ipsademo.isoftstone.com/passport/?returnUrl=%2fids%2fconnect%2fauthorize%2fcallback"
    "%3fclient_id%3dfdc3621b-bba3-4f61-bea3-d8e5272c0454"
    "%26redirect_uri%3dhttp%3a%2f%2f10.136.0.123%3a33372%2fOidc"
    "%26response_type%3did_token+token"
    "%26scope%3dopenid+profile+Media+Message+MasterData+MasterData2+iDaas+BIDApi"
    "%26nonce%3d582fb9ceabf66698c4c1752aac97e5be"
    "%26state%3d532d27ac2abe53445ef9318427ba2570"
)

# 录制器把上传文件复制到了录制证据目录（upload 事件 files[0].path，含 \\?\ 长路径前缀）
RAW_FILE = (
    r"\\?\E:\work\RustWorks\cn-codex\codey\recordings\uploads"
    r"\0da42eab-aa75-49f7-b62b-8bb74ef0ea21"
    r"\0029-00-1-技术开发协议（软通为受托方）-V4.1-2020 (2).docx"
)

USER_NAME = "yiyuh"        # 用户名输入框最终稳定值（合并多次插入/退格）
BIZ_GROUP = "CORP"         # 业务组下拉选中值

CUR_STEP = 0               # 当前步骤号（用于失败时定位）


# ---------------------------------------------------------------------------
# 通用工具
# ---------------------------------------------------------------------------
def log(msg):
    print(msg, flush=True)


def step_no():
    """返回并递增当前步骤号。"""
    global CUR_STEP
    CUR_STEP += 1
    return CUR_STEP


def resolve_upload_path():
    """解析上传文件的真实路径（去掉 \\?\ 前缀后检查存在性）。"""
    cands = [RAW_FILE]
    stripped = RAW_FILE
    if stripped.startswith("\\\\?\\"):
        stripped = stripped[4:]
    cands.append(stripped)
    cands.append(stripped.replace("/", "\\"))
    for c in cands:
        if c and os.path.exists(c):
            return c
    raise FileNotFoundError("未找到录制证据文件（上传副本）: " + RAW_FILE)


def wait_any(page, selectors, timeout_ms=8000):
    """在多候选选择器中轮询查找第一个可见元素，返回命中的 selector；找不到返回 None。"""
    deadline = time.time() + timeout_ms / 1000.0
    while time.time() < deadline:
        for sel in selectors:
            if not sel:
                continue
            try:
                el = page.query_selector(sel)
                if el is not None and el.is_visible():
                    return sel
            except Exception:
                pass
        time.sleep(0.25)
    return None


def click_first(page, selectors, desc, timeout_ms=10000):
    """等待候选选择器可见后点击第一个命中的元素。"""
    sel = wait_any(page, selectors, timeout_ms)
    if sel is None:
        raise RuntimeError("未找到可点击元素（%s）: %s" % (desc, selectors))
    page.click(sel, timeout=5000)
    log("    -> 已点击 %s" % sel)
    return sel


def retry_run(fn, desc, retries=3, delay=0.8):
    """带重试地执行 fn。"""
    last = None
    for i in range(retries):
        try:
            return fn()
        except Exception as e:
            last = e
            log("    [重试 %d/%d] %s: %s: %s" % (i + 1, retries, desc, type(e).__name__, e))
            time.sleep(delay)
    raise last


def settle(page):
    """导航后等待页面稳定（networkidle 容错）。"""
    try:
        page.wait_for_load_state("networkidle", timeout=8000)
    except Exception:
        pass
    time.sleep(0.5)


def upload_file(page, file_selectors, path, trigger_selectors, desc, timeout_ms=10000):
    """上传文件。

    优先：直接在 DOM 中查找 file input（即使隐藏/不可见）并 set_input_files；
    兜底：点击触发元素时监听 file_chooser 事件，用 fc.set_files 注入文件。
    """
    for sel in file_selectors:
        if not sel:
            continue
        try:
            el = page.query_selector(sel)
            if el is not None:
                page.set_input_files(sel, path, timeout=timeout_ms)
                log("    -> 已通过 %s 上传文件" % sel)
                return sel
        except Exception as e:
            log("    [提示] set_input_files(%s) 失败: %s" % (sel, e))
    # 兜底：点击触发元素并捕获 file_chooser
    try:
        with page.expect_file_chooser(timeout=timeout_ms) as fc_info:
            click_first(page, trigger_selectors, desc, timeout_ms=timeout_ms)
        fc_info.value.set_files(path)
        log("    -> 已通过 file_chooser 上传文件")
        return trigger_selectors[0]
    except Exception as e:
        raise RuntimeError("无法上传文件（未找到 file input 且未触发文件选择器）: %s" % e)


# ---------------------------------------------------------------------------
# 主流程
# ---------------------------------------------------------------------------
def main():
    global CUR_STEP
    result = {"ok": False, "step": 0, "url": "", "title": "", "error": ""}
    browser = None
    page = None
    try:
        upload_path = resolve_upload_path()
        log("上传文件（录制证据副本）: %s" % upload_path)

        with sync_playwright() as p:
            # 真实启动有界面的浏览器
            browser = p.chromium.launch(headless=False, args=["--start-maximized"])
            context = browser.new_context(viewport=None, locale="zh-CN")
            page = context.new_page()
            page.set_default_timeout(12000)

            # --------------------------------------------------------------
            # 步骤 1：打开 SSO 登录页
            # 说明：录制中首个 navigate 是 redirect（用户输入地址后 SSO 自动跳转），
            #       此处直接打开登录页作为回放起点。
            # --------------------------------------------------------------
            n = step_no()
            log("步骤 %d：打开 SSO 登录页" % n)
            page.goto(LOGIN_URL, timeout=60000, wait_until="domcontentloaded")
            settle(page)
            if wait_any(page, ["#userName", "[name=userName]", "div.container.body-content input"], 20000) is None:
                raise RuntimeError("登录页未加载（未找到用户名输入框）")
            log("    -> 登录页已加载")

            # --------------------------------------------------------------
            # 步骤 2：悬停并点击用户名输入框
            # 说明：录制中 hover + 两次 click（同一元素），合并为一次聚焦点击。
            # --------------------------------------------------------------
            n = step_no()
            log("步骤 %d：悬停并点击用户名输入框" % n)
            retry_run(lambda: page.hover("#userName", timeout=5000), "hover 用户名输入框")
            click_first(page, ["#userName", "[name=userName]"], "用户名输入框")

            # --------------------------------------------------------------
            # 步骤 3：输入用户名
            # 说明：录制中 8 次 type/key（插入、Backspace、deleteContentBackward）
            #       是同一输入框的连续编辑过程，合并为最终稳定值后一次 fill。
            # --------------------------------------------------------------
            n = step_no()
            log("步骤 %d：输入用户名 %s（合并中间 KEYIN/删除/退格）" % (n, USER_NAME))
            def _fill_user():
                page.click("#userName", timeout=5000)
                page.fill("#userName", USER_NAME, timeout=5000)
            retry_run(_fill_user, "fill 用户名")

            # --------------------------------------------------------------
            # 步骤 4：点击 Login 提交登录
            # 说明：录制中 click "Login" 后紧跟 submit 事件，submit 由点击触发，忽略。
            # --------------------------------------------------------------
            n = step_no()
            log("步骤 %d：点击 Login 登录" % n)
            click_first(page, ['text="Login"', "div.container.body-content button", "button[type=submit]"], "登录按钮")

            # --------------------------------------------------------------
            # 步骤 5：等待 SSO 回调自动跳转到预审页
            # 说明：/Oidc -> /preaudit 均为 cause=redirect（表单提交 / JS 跳转），
            #       禁止 page.goto，只等待 URL 与页面元素。
            # --------------------------------------------------------------
            n = step_no()
            log("步骤 %d：等待登录后自动跳转到预审页 /preaudit" % n)
            try:
                page.wait_for_url(lambda u: ("/preaudit" in u) and ("passport" not in u), timeout=45000)
            except PwTimeoutError:
                if wait_any(page, ["div.preaudit-upload", "main.app-main"], 15000) is None:
                    raise RuntimeError("登录后未跳转到预审页（可能登录失败或 SSO 回调超时）")
            settle(page)
            if "/preaudit" not in page.url:
                raise RuntimeError("断言失败：登录后 URL 未包含 /preaudit，实际 URL: %s" % page.url)
            log("    -> 当前 URL: %s" % page.url)

            # --------------------------------------------------------------
            # 步骤 6：点击上传区域（打开上传表单）
            # --------------------------------------------------------------
            n = step_no()
            log("步骤 %d：点击上传区域" % n)
            click_first(page, ["div.preaudit-upload"], "上传区域")

            # --------------------------------------------------------------
            # 步骤 7：悬停文件区域
            # --------------------------------------------------------------
            n = step_no()
            log("步骤 %d：悬停文件区域" % n)
            sel = wait_any(page, ["div.file-area"], 8000)
            if sel:
                page.hover(sel, timeout=5000)
            else:
                log("    [提示] 未找到 div.file-area，继续")

            # --------------------------------------------------------------
            # 步骤 8：选择业务组 CORP
            # 说明：录制中 click 打开下拉 + select CORP（重复 click 合并）。
            # --------------------------------------------------------------
            n = step_no()
            log("步骤 %d：选择业务组 %s" % (n, BIZ_GROUP))
            click_first(page, ["#biz-group", "[name=biz_group]", "#preaudit-form select"], "业务组下拉")
            retry_run(lambda: page.select_option("#biz-group", BIZ_GROUP, timeout=5000), "select 业务组")

            # --------------------------------------------------------------
            # 步骤 9：点击文件图标并上传合同文件
            # 说明：录制 upload 事件 files[].path 非空，必须用真实证据副本。
            #       录制中 click #file-input 打开的是系统文件选择器；回放时
            #       file input 通常是隐藏的（由图标触发），因此优先用
            #       set_input_files 直接注入，失败时监听 file_chooser 兜底。
            # --------------------------------------------------------------
            n = step_no()
            log("步骤 %d：点击文件图标并上传合同文件（set_input_files / file_chooser）" % n)
            upload_file(
                page,
                ["#file-input", "[name=file]", "#preaudit-form input[type=file]", "input[type=file]"],
                upload_path,
                ["div.file-icon", 'text="📁"'],
                "文件图标（触发文件选择器）",
            )
            time.sleep(1.5)  # 等待上传完成

            # --------------------------------------------------------------
            # 步骤 11：悬停并点击「开始预审」按钮
            # 说明：submit 事件由点击触发，忽略。
            # --------------------------------------------------------------
            n = step_no()
            log("步骤 %d：点击「开始预审」按钮" % n)
            retry_run(lambda: page.hover("#submit-btn", timeout=5000), "hover 开始预审按钮")
            click_first(page, ["#submit-btn", 'text="🔍 开始预审"', "button.btn.primary"], "开始预审按钮")

            # --------------------------------------------------------------
            # 步骤 12：等待提交后跳转到预审详情页 /preaudit/<id>
            # 说明：跳转为 cause=redirect（scriptInitiated），禁止 goto。
            # --------------------------------------------------------------
            n = step_no()
            log("步骤 %d：等待跳转到预审详情页 /preaudit/<id>" % n)
            try:
                page.wait_for_url(lambda u: re.search(r"/preaudit/\d+", u) is not None, timeout=60000)
            except PwTimeoutError:
                if wait_any(page, ["div.progress-track", "div.loading-area"], 20000) is None:
                    raise RuntimeError("提交预审后未跳转到详情页")
            settle(page)
            if re.search(r"/preaudit/\d+", page.url) is None:
                raise RuntimeError("断言失败：未进入预审详情页，实际 URL: %s" % page.url)
            log("    -> 详情页 URL: %s" % page.url)

            # --------------------------------------------------------------
            # 步骤 13：刷新预审详情页（录制 cause=reload）
            # --------------------------------------------------------------
            n = step_no()
            log("步骤 %d：刷新预审详情页（录制 cause=reload）" % n)
            retry_run(lambda: page.reload(wait_until="domcontentloaded", timeout=60000), "刷新详情页")
            settle(page)

            # --------------------------------------------------------------
            # 步骤 14：等待文档编辑 iframe 出现
            # --------------------------------------------------------------
            n = step_no()
            log("步骤 %d：等待文档编辑 iframe" % n)
            if wait_any(page, ["iframe[name=frameEditor]", "[name=frameEditor]", "#doc-office iframe"], 30000) is None:
                log("    [提示] 未检测到文档编辑 iframe，继续后续步骤")

            # --------------------------------------------------------------
            # 步骤 15：悬停并点击「规则库配置」页签
            # 说明：跳转 /rules 为 cause=link（anchorClick），由点击触发，禁止 goto。
            # --------------------------------------------------------------
            n = step_no()
            log("步骤 %d：点击「规则库配置」页签" % n)
            sel = wait_any(page, ["a.tab", 'text="规则库配置"', "div.tabs a"], 30000)
            if sel:
                retry_run(lambda: page.hover(sel, timeout=5000), "hover 规则库配置页签")
            click_first(page, ['text="规则库配置"', "a.tab", "div.tabs a"], "规则库配置页签")

            # --------------------------------------------------------------
            # 步骤 16：等待规则库页面 /rules 加载并断言
            # --------------------------------------------------------------
            n = step_no()
            log("步骤 %d：等待规则库页面 /rules 加载" % n)
            try:
                page.wait_for_url(lambda u: u.rstrip("/").endswith("/rules"), timeout=30000)
            except PwTimeoutError:
                pass
            if wait_any(page, ["#rules-tbody", "div.toolbar", "table.setting-table"], 20000) is None:
                raise RuntimeError("规则库页面未加载")
            settle(page)
            log("    -> 规则库页面已加载，URL: %s" % page.url)

            result = {
                "ok": True,
                "step": n,
                "url": page.url,
                "title": page.title() or "",
                "error": "",
            }

    except Exception as e:
        result["ok"] = False
        result["step"] = CUR_STEP
        result["error"] = "%s: %s" % (type(e).__name__, e)
        result["url"] = page.url if page is not None else ""
    finally:
        # 成功/失败都必须打印 REPLAY_RESULT（关闭浏览器之前）
        try:
            if page is not None:
                result["title"] = result.get("title") or (page.title() if page else "")
        except Exception:
            pass
        print("REPLAY_RESULT " + json.dumps(result, ensure_ascii=False), flush=True)
        try:
            if browser is not None:
                browser.close()
        except Exception:
            pass

    if not result["ok"]:
        sys.exit(1)


if __name__ == "__main__":
    main()
