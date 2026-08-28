# -*- coding: utf-8 -*-
"""
recording-a7d985e4 回放脚本（由录制记录自动生成，Playwright sync API）。

- 录制文件: ../a7d985e4-70f1-4283-840b-ec15c1f8043b.trace.json
- 输入文档: a7d985e4-70f1-4283-840b-ec15c1f8043b.input.json
  （运行时读取，所有 fill 的实际值均取自 fields 数组，不在脚本中硬编码输入值）

流程概述:
  1. 打开录制会话入口页（IPSA 登录页，trace 中第一个业务页面）；
  2. 点击用户名输入框，输入最终稳定值（取自输入文档 fields[1].value）；
  3. 点击 Login 按钮提交登录；
  4. 登录后的 /Oidc、/preaudit 均为 SSO 自动 redirect（cause=redirect），只等待、不再 goto；
  5. 断言到达 /preaudit 且页面元素可见后，输出 REPLAY_RESULT。

运行前请确保: pip install playwright && playwright install chromium
"""

import json
import os
import re
import sys
import time

from playwright.sync_api import sync_playwright

try:
    # Windows 下管道输出默认跟随 ANSI 代码页，强制 UTF-8 避免中文/符号打印失败
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    sys.stderr.reconfigure(encoding="utf-8", errors="replace")
except Exception:
    pass

SESSION_ID = "a7d985e4-70f1-4283-840b-ec15c1f8043b"
RECORDING_NAME = "recording-a7d985e4"

# 输入文档定位：优先环境变量 REPLAY_INPUT_FILE，否则使用脚本同目录固定文件名
INPUT_FILE = os.environ.get("REPLAY_INPUT_FILE") or os.path.join(
    os.path.dirname(os.path.abspath(__file__)),
    "a7d985e4-70f1-4283-840b-ec15c1f8043b.input.json",
)

# trace 中会话首个业务页面的 URL（登录页本身；后续所有跳转均为自动 redirect，不允许再 goto）
ENTRY_URL = (
    "https://ipsademo.isoftstone.com/passport/"
    "?returnUrl=%2fids%2fconnect%2fauthorize%2fcallback"
    "%3fclient_id%3dfdc3621b-bba3-4f61-bea3-d8e5272c0454"
    "%26redirect_uri%3dhttp%3a%2f%2f10.136.0.123%3a33372%2fOidc"
    "%26response_type%3did_token+token"
    "%26scope%3dopenid+profile+Media+Message+MasterData+MasterData2+iDaas+BIDApi"
    "%26nonce%3df052bc5565853603b0964bfd8ec393f8"
    "%26state%3d13c1dba9062894c3d7c874790e5d6961"
)

# 登录成功后 SSO 自动跳转的宽松匹配片段（不断言带 code/state 的完整回调 URL）
APP_HOST_FRAGMENT = "10.136.0.123:33372"
PREAUDIT_PATTERN = re.compile(r"preaudit", re.IGNORECASE)


def print_result(ok, **extra):
    """按约定在关闭浏览器前打印单行 REPLAY_RESULT JSON。"""
    payload = {"ok": bool(ok)}
    payload.update(extra)
    print("REPLAY_RESULT: " + json.dumps(payload, ensure_ascii=False))


def fail_and_exit(error):
    """打印失败结果并以非 0 退出。"""
    print(f"[错误] {error}", file=sys.stderr)
    print_result(False, error=str(error))
    sys.exit(1)


def load_fields():
    """读取并校验输入文档，返回 fields 数组；缺项/文件不存在时明确报错退出，禁止回退示例数据。"""
    if not os.path.isfile(INPUT_FILE):
        fail_and_exit(f"输入文档不存在: {INPUT_FILE}")
    try:
        with open(INPUT_FILE, "r", encoding="utf-8") as f:
            data = json.load(f)
    except Exception as exc:
        fail_and_exit(f"输入文档解析失败: {exc}")
    fields = data.get("fields") if isinstance(data, dict) else None
    if not isinstance(fields, list) or not fields:
        fail_and_exit("输入文档缺少有效的 fields 数组")
    return fields


def field(fields, index, expected_types):
    """按序号取输入项并校验类型（expected_types 可为字符串或集合）。"""
    if isinstance(expected_types, str):
        expected_types = {expected_types}
    if index >= len(fields):
        fail_and_exit(f"输入文档 fields 缺少第 {index + 1} 项")
    item = fields[index]
    if item.get("type") not in expected_types:
        fail_and_exit(
            f"输入文档 fields[{index}] 类型为 {item.get('type')!r}，期望 {'/'.join(sorted(expected_types))!r}"
        )
    return item


def wait_page_stable(page, timeout_ms=15000):
    """导航后等待页面加载稳定（domcontentloaded + networkidle，失败不中断回放）。"""
    for state in ("domcontentloaded", "networkidle"):
        try:
            page.wait_for_load_state(state, timeout=timeout_ms)
        except Exception:
            pass


def locator_candidates(item):
    """整理候选选择器：selector 优先，其后为 selectorCandidates，过滤空值与重复项。"""
    candidates = []
    for raw in [item.get("selector")] + list(item.get("selectorCandidates") or []):
        sel = (raw or "").strip()
        if sel and sel not in candidates:
            candidates.append(sel)
    return candidates


def resolve_locator(page, item, timeout_ms=15000, desc=""):
    """在候选选择器中轮询查找可见元素；找不到则抛错（绝不把空 selector 传给 Playwright）。"""
    candidates = locator_candidates(item)
    if not candidates:
        raise RuntimeError(f"{desc}: 没有可用的选择器")
    deadline = time.time() + timeout_ms / 1000.0
    last_error = None
    while time.time() < deadline:
        for sel in candidates:
            try:
                loc = page.locator(sel).first
                loc.wait_for(state="visible", timeout=2000)
                return loc
            except Exception as exc:
                last_error = exc
        page.wait_for_timeout(300)
    raise RuntimeError(f"{desc}: 未能定位元素（候选: {candidates}），最后错误: {last_error}")


def with_retry(page, desc, action, retries=3):
    """单步重试包装：失败时稍候重试，超过次数后抛出。"""
    last_error = None
    for attempt in range(1, retries + 1):
        try:
            return action()
        except Exception as exc:
            last_error = exc
            print(f"    第 {attempt}/{retries} 次尝试失败: {exc}")
            if attempt < retries:
                page.wait_for_timeout(1000)
    raise RuntimeError(f"{desc} 重试 {retries} 次后仍失败: {last_error}")


def tolerant_hover(page, candidates, desc):
    """trace 补充的悬停步骤：失败仅打印提示，不中断回放。"""
    item = {"selector": candidates[0], "selectorCandidates": candidates}
    try:
        loc = resolve_locator(page, item, timeout_ms=8000, desc=desc)
        loc.hover(timeout=5000)
        print(f"    已悬停: {desc}")
    except Exception as exc:
        print(f"    [提示] 悬停 {desc} 未成功（不中断回放）: {exc}")


def main():
    print(f"=== 回放开始: {RECORDING_NAME} ({SESSION_ID}) ===")
    print(f"输入文档: {INPUT_FILE}")
    fields = load_fields()
    print(f"输入文档 fields 数量: {len(fields)}")

    # fields[0]: 点击 #userName；fields[1]: 输入最终值；fields[2]: 点击 text="Login"
    click_user = field(fields, 0, {"click"})
    type_user = field(fields, 1, {"type", "fill"})
    click_login = field(fields, 2, {"click"})

    with sync_playwright() as playwright:
        browser = None
        context = None
        try:
            # 步骤 1：启动有界面的 Chromium 浏览器，并新建上下文与页面
            print("步骤 1：启动 Chromium 浏览器（有界面模式 headless=False）")
            browser = playwright.chromium.launch(headless=False)
            context = browser.new_context(viewport={"width": 1440, "height": 900})
            page = context.new_page()
            page.set_default_timeout(15000)
            page.set_default_navigation_timeout(45000)

            # 步骤 2：打开录制会话入口登录页。
            # 说明：trace 中该 navigate 的 cause=redirect（用户从新标签页进入应用后被 SSO 带到登录页）；
            # 它是录制会话的第一个页面，回放从空白上下文开始必须先打开它；
            # 其后的所有跳转（/Oidc、/preaudit）均为自动 redirect，只等待、禁止再 goto。
            print("步骤 2：打开登录入口页（录制会话首个页面）")
            with_retry(
                page,
                "打开登录入口页",
                lambda: page.goto(ENTRY_URL, timeout=30000, wait_until="domcontentloaded"),
            )
            wait_page_stable(page)

            # 步骤 3：等待登录页稳定，并断言用户名输入框可见（关键元素校验）
            print("步骤 3：等待登录页加载并断言用户名输入框可见")
            with_retry(
                page,
                "等待用户名输入框出现",
                lambda: resolve_locator(page, click_user, timeout_ms=20000, desc="用户名输入框"),
            )
            print(f"    当前 URL: {page.url}")

            # 步骤 4：悬停登录表单区域（trace 中的 hover 事件，先悬停再点击）
            print("步骤 4：悬停登录表单区域（trace 中的 hover）")
            tolerant_hover(
                page,
                ["div.container.body-content", "div.container.body-content form", "form"],
                "登录表单容器",
            )

            # 步骤 5：点击用户名输入框（输入文档 fields 步骤 1，type=click）
            print(f"步骤 5：点击用户名输入框（fields 步骤 1: {click_user.get('label')}）")

            def click_username():
                loc = resolve_locator(page, click_user, timeout_ms=8000, desc="用户名输入框")
                loc.click(timeout=10000)

            with_retry(page, "点击用户名输入框", click_username)

            # 步骤 6：输入用户名最终稳定值（输入文档 fields 步骤 2，合并 trace 中 yi/yiyu/yiyuh 等中间编辑值，只 fill 最终值）
            print(f"步骤 6：输入用户名最终稳定值（fields 步骤 2: {type_user.get('label')}）")
            username_value = type_user.get("value")
            if username_value is None:
                raise RuntimeError("输入文档 fields[1] 缺少 value（用户名最终值）")

            def fill_username():
                loc = resolve_locator(page, type_user, timeout_ms=8000, desc="用户名输入框")
                try:
                    loc.click(timeout=3000)  # 先点击聚焦
                except Exception:
                    loc.focus(timeout=3000)
                loc.fill(str(username_value), timeout=10000)
                actual = loc.input_value(timeout=5000)
                if actual != str(username_value):
                    raise RuntimeError(f"输入校验失败: 期望 {username_value!r}, 实际 {actual!r}")

            with_retry(page, "输入用户名", fill_username)
            print(f"    已输入最终值: {username_value}")

            # 步骤 7：悬停 Login 按钮（trace 中的 hover 事件，悬停展开/聚焦后再点击）
            print("步骤 7：悬停 Login 按钮（trace 中的 hover）")
            tolerant_hover(
                page,
                ['text="Login"', "div.container.body-content button", "button"],
                "Login 按钮",
            )

            # 步骤 8：点击 Login 按钮提交登录（输入文档 fields 步骤 3，type=click）
            print(f"步骤 8：点击 Login 按钮提交登录（fields 步骤 3: {click_login.get('label')}）")

            def click_login_button():
                loc = resolve_locator(page, click_login, timeout_ms=8000, desc="Login 按钮")
                loc.click(timeout=10000)

            try:
                with_retry(page, "点击 Login 按钮", click_login_button)
            except Exception as exc:
                # 点击可能因立即开始的跳转而中断，这里提示后继续等待跳转结果
                print(f"    [提示] 点击 Login 出现异常（可能因跳转被中断，继续等待）: {exc}")

            # 步骤 9：等待登录后的 SSO 自动跳转（cause=redirect，只用宽松片段等待，禁止 goto）。
            # trace 顺序: passport → /Oidc（formSubmissionPost）→ /preaudit（scriptInitiated）
            print("步骤 9：等待登录后的 SSO 自动跳转（/Oidc → /preaudit，只等待不 goto）")
            try:
                page.wait_for_url(PREAUDIT_PATTERN, timeout=60000)
            except Exception as exc:
                current = page.url
                if "preaudit" in current.lower() or APP_HOST_FRAGMENT in current:
                    print(f"    [提示] wait_for_url 超时，但当前 URL 已包含目标片段: {current}")
                else:
                    raise RuntimeError(f"登录后未跳转到 /preaudit，当前 URL: {current}") from exc
            wait_page_stable(page, timeout_ms=20000)
            print(f"    当前 URL: {page.url}")

            # 步骤 10：断言业务页加载完成（工具栏可见或 URL 已到达 /preaudit）
            print("步骤 10：断言登录后的业务页加载完成")
            toolbar_ok = False
            try:
                resolve_locator(
                    page,
                    {"selector": "div.toolbar", "selectorCandidates": ["div.toolbar", ".toolbar"]},
                    timeout_ms=15000,
                    desc="工具栏",
                )
                toolbar_ok = True
                print("    已确认工具栏可见")
            except Exception as exc:
                print(f"    [提示] 未找到工具栏元素: {exc}")
            url_ok = "preaudit" in page.url.lower()
            if not (toolbar_ok or url_ok):
                raise RuntimeError(f"登录后的页面断言失败，当前 URL: {page.url}")

            # 步骤 11：悬停工具栏（trace 最后一个 hover 事件）
            print("步骤 11：悬停工具栏（trace 最后一个 hover）")
            tolerant_hover(page, ["div.toolbar", ".toolbar"], "工具栏")

            # 步骤 12：回放完成，在关闭浏览器前输出 REPLAY_RESULT
            print("步骤 12：回放完成，输出结果")
            try:
                title = page.title()
            except Exception:
                title = ""
            print_result(True, step=12, url=page.url, title=title)
            print(f"=== 回放成功: {RECORDING_NAME} ===")

        finally:
            # 用户手动关闭浏览器窗口时 close() 可能抛错，必须忽略，不得当作回放失败
            try:
                if context is not None:
                    context.close()
            except Exception:
                pass
            try:
                if browser is not None:
                    browser.close()
            except Exception:
                pass


if __name__ == "__main__":
    try:
        main()
    except SystemExit:
        raise
    except Exception as exc:
        # 任何失败都要输出 REPLAY_RESULT 并以非 0 退出
        print_result(False, error=f"{type(exc).__name__}: {exc}")
        print(f"=== 回放失败: {RECORDING_NAME} ===")
        sys.exit(1)
