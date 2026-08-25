# -*- coding: utf-8 -*-
"""
Playwright 回放脚本 - recording-bb7de982
录制时间: 2026-08-25T06:36:53 - 2026-08-25T06:39:37
描述: 登录 IPSA 门户（飞连 OIDC）→ SCMS 销售合同管理 - 信息综合查询
"""

import sys, json, time, traceback
from playwright.sync_api import sync_playwright

try:
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    sys.stderr.reconfigure(encoding="utf-8", errors="replace")
except Exception:
    pass

BUDGET = 170
_start = time.time()


def elapsed():
    return time.time() - _start


def remaining():
    return max(0.0, BUDGET - elapsed())


class StepError(Exception):
    pass


def log(msg, *args):
    m = msg % args if args else msg
    print(m, flush=True)


def step_no(cnt, title):
    log("步骤 %d：%s", cnt, title)
    return cnt + 1


def wait_url(page, fragment, timeout=None):
    t = timeout if timeout is not None else min(30, remaining())
    deadline = time.time() + t
    while time.time() < deadline:
        try:
            if fragment in page.url:
                return True
        except Exception:
            pass
        time.sleep(1)
    return False


def click_first(page, candidates, timeout=12, desc="", retries=3):
    t = min(timeout, remaining())
    for sel in candidates:
        if not sel:
            continue
        for i in range(retries):
            try:
                page.locator(sel).first.click(timeout=t * 1000)
                log("  [click] %s (%s)", sel, desc)
                return True
            except Exception:
                time.sleep(0.8)
    raise StepError(f"点击失败: {desc}")


def hover_first(page, candidates, timeout=8, desc=""):
    t = min(timeout, remaining())
    for sel in candidates:
        if not sel:
            continue
        try:
            page.locator(sel).first.hover(timeout=t * 1000)
            log("  [hover] %s (%s)", sel, desc)
            return True
        except Exception:
            continue
    return False


def main():
    result = {"ok": False, "step": 0, "url": "", "title": "", "error": ""}
    browser = None
    context = None
    page = None
    try:
        with sync_playwright() as p:
            browser = p.chromium.launch(headless=True)
            context = browser.new_context(viewport={"width": 1440, "height": 900}, locale="zh-CN")
            page = context.new_page()
            n = 0

            # 步骤 1: goto IPSA 门户（cause=user）
            n = step_no(n, "打开 IPSA 门户首页")
            page.goto("https://ipsapro.isoftstone.com/", wait_until="domcontentloaded", timeout=30000)
            log("  URL: %s", page.url[:130])

            # 步骤 2: 等待 SSO 跳转 + 登录页渲染（redirect，不 goto）
            n = step_no(n, "等待 SSO 跳转到飞连登录页并渲染稳定")
            ok_sel = None
            # SSO 跳转偶尔失败/缓慢：分轮次等待，超时后重新 goto 门户重试跳转链
            for _attempt in range(3):
                deadline = time.time() + min(25, remaining())
                while time.time() < deadline:
                    try:
                        if "feilian" in page.url:
                            ok_sel = "feilian"
                            break
                        if "passport.isoftstone" in page.url:
                            ok_sel = "passport"
                            break
                    except Exception:
                        pass
                    time.sleep(2)
                if ok_sel:
                    break
                if remaining() < 10:
                    break
                log("  [retry] SSO 未跳转，重新加载门户（第 %d 次）", _attempt + 1)
                try:
                    page.goto("https://ipsapro.isoftstone.com/", wait_until="domcontentloaded", timeout=20000)
                except Exception:
                    pass
            if not ok_sel:
                raise StepError("登录页渲染超时：未到达飞连登录页")
            log("  feilian 登录页稳定 URL=%s", page.url[:90])
            # 展开"更多登录方式"弹出层（录制中首先要 hover 容器）
            hover_first(page, ["div.content_5klQOEKn", "div.action-box_IIlGwOXK"], timeout=15, desc="展开更多登录方式")
            deadline = time.time() + min(15, remaining())
            while time.time() < deadline:
                try:
                    if page.locator("div.item_oFCaWe7B").count() > 0:
                        log("  ✓ 登录方式图标已出现 (count=%d)", page.locator("div.item_oFCaWe7B").count())
                        break
                except Exception:
                    pass
                time.sleep(1)

            # 步骤 3: 切换登录方式为账号密码（录制 hover+click item[1]）
            n = step_no(n, "切换登录方式为「账号密码」")
            switched = False
            for idx in (1, 0):
                try:
                    loc = page.locator("div.item_oFCaWe7B").nth(idx)
                    loc.hover(timeout=5000)
                    time.sleep(0.4)
                    img = loc.locator("img")
                    if img.count() > 0:
                        img.first.click(timeout=5000)
                    else:
                        loc.click(timeout=5000)
                    time.sleep(1.5)
                    if page.locator("#account_input").count() > 0:
                        log("  [switch] item[%d] -> 账号密码表单", idx)
                        switched = True
                        break
                except Exception:
                    continue
            if not switched:
                raise StepError("未能切换到账号密码登录表单")

            # 步骤 4: 输入账号 junlong（合并最终稳定值）
            n = step_no(n, "输入账号（合并多次输入为最终值）")
            page.locator("#account_input").click(timeout=5000)
            page.locator("#account_input").fill("junlong")
            log("  [fill] #account_input = junlong (账号)")

            # 步骤 5: 输入密码 49718751L!abcd（与探针验证一致）
            n = step_no(n, "输入密码（合并为最终值）")
            page.locator("#password_input").click(timeout=5000)
            page.locator("#password_input").fill("49718751L!abcd")
            log("  [fill] #password_input = 49718751L!abcd (密码)")

            # 步骤 6: 勾选协议（录制二次 click label.arco-checkbox，一次即可）
            n = step_no(n, "勾选「我已阅读并同意…」协议复选框")
            page.locator("label.arco-checkbox").first.click(timeout=5000)
            time.sleep(0.5)
            checked = page.locator("label.arco-checkbox.arco-checkbox-checked").count() > 0
            if not checked:
                page.locator("label.arco-checkbox input").first.check(force=True, timeout=5000)
                time.sleep(0.5)
                checked = page.locator("label.arco-checkbox.arco-checkbox-checked").count() > 0
            if not checked:
                raise StepError("协议复选框未能勾选成功")
            log("  [ok] 协议复选框已勾选")

            # 步骤 7: 点击登录按钮（录制 click button.arco-btn-primary）
            n = step_no(n, "点击「登 录」按钮提交")
            page.locator("button.arco-btn.arco-btn-primary").first.click(timeout=15000)
            log("  [click] 登录按钮")

            # 步骤 8: 等待 SSO 认证回调回跳门户（redirect，不 goto，不断言 code/state）
            # 使用 Playwright 内置 wait_for_url（比自定义轮询更可靠，支持 glob 匹配）
            n = step_no(n, "等待 SSO 认证回调并回跳门户（redirect，只等待）")
            try:
                page.wait_for_url("**ipsapro.isoftstone.com/portal**", timeout=min(60, remaining()) * 1000)
            except Exception:
                try:
                    page.wait_for_url("**SCMS/**", timeout=min(30, remaining()) * 1000)
                except Exception:
                    raise StepError("SSO 登录后未回跳（可能密码错误或账户过期）")

            # 步骤 9: 门户 hover+click「更多 ...」（录制 hover->click）
            n = step_no(n, "hover 并点击「更多 ...」展开应用菜单")
            try:
                page.locator("text=更多").first.wait_for(state="visible", timeout=min(30, remaining())*1000)
            except Exception:
                pass
            hover_first(page, ["text=\"更多 ...\"", "text=更多", "#g_menu a"], desc="更多菜单")
            click_first(page, ["text=\"更多 ...\"", "text=更多", "#g_menu a"], desc="更多菜单")

            # 步骤 10: hover+click「销售合同管理」（录制 hover->click text="销售合同管理"）
            n = step_no(n, "hover 并点击「销售合同管理」")
            try:
                page.locator("text=销售合同管理").first.wait_for(state="visible", timeout=min(20, remaining())*1000)
            except Exception:
                pass
            hover_first(page, ["text=\"销售合同管理\"", "text=销售合同管理", "div.span1 h5"], desc="销售合同管理")
            click_first(page, ["text=\"销售合同管理\"", "div.span1 h5", "div.row-fluid.offset1 h5"], desc="销售合同管理")

            # 步骤 11: 等待跳转到 SCMS
            n = step_no(n, "等待进入 SCMS（redirect）")
            try:
                page.wait_for_url("**/SCMS/**", timeout=min(50, remaining()) * 1000)
            except Exception:
                raise StepError("点击销售合同管理后未进入 SCMS")
            time.sleep(3)

            # 步骤 12: hover「综合查询及报表」子菜单展开浮层（录制 hover->click submenu-title）
            # SCMS 菜单为 horizontal 模式：信息综合查询位于「综合查询及报表」子菜单的弹出浮层中，
            # 必须 hover 该子菜单标题后浮层才可见（录制中 submenu-title 带 ivu-menu-opened 状态）
            n = step_no(n, "hover 并点击「综合查询及报表」子菜单展开")
            try:
                page.locator("div.ivu-menu-submenu-title").first.wait_for(state="visible", timeout=min(20, remaining())*1000)
            except Exception:
                pass
            hover_first(page, ["text=综合查询及报表", "text=\"综合查询及报表\"", "div.ivu-menu-submenu-title"], desc="综合查询及报表子菜单")
            # hover 后浮层弹出，等待下拉菜单出现
            try:
                page.locator("ul.ivu-menu-drop-list li").first.wait_for(state="visible", timeout=min(8, remaining())*1000)
            except Exception:
                pass
            click_first(page, ["text=综合查询及报表", "div.ivu-menu-submenu-title"], desc="综合查询及报表子菜单")

            # 步骤 13: 点击「信息综合查询」（录制 hover->click li.ivu-menu-item）
            n = step_no(n, "hover 并点击「信息综合查询」")
            try:
                page.locator("ul.ivu-menu-drop-list li").first.wait_for(state="visible", timeout=min(8, remaining())*1000)
            except Exception:
                pass
            hover_first(page, ["text=信息综合查询", "ul.ivu-menu-drop-list li", "li.ivu-menu-item"], desc="信息综合查询")
            click_first(page, ["text=信息综合查询", "ul.ivu-menu-drop-list li", "li.ivu-menu-item"], desc="信息综合查询")

            # 步骤 14: 等待进入信息综合查询页
            n = step_no(n, "等待进入 /SCMS/InfoQuery 页面")
            try:
                page.wait_for_url("**/SCMS/InfoQuery**", timeout=min(40, remaining()) * 1000)
            except Exception:
                # 降级：检查 URL 是否包含 InfoQuery 片段
                if not wait_url(page, "InfoQuery", timeout=min(15, remaining())):
                    raise StepError("未进入信息综合查询页面")

            result.update(ok=True, step=n, url=page.url, title=page.title())
            log("【回放成功】关键步骤全部完成，共 %d 步", n)

    except StepError as e:
        result.update(ok=False, error=str(e))
        log("【回放失败】StepError: %s", e)
    except Exception as e:
        result.update(ok=False, error=f"{type(e).__name__}: {e}")
        log("【回放失败】异常: %s", traceback.format_exc())
    finally:
        try:
            if result["url"] == "" and page is not None:
                try:
                    result["url"] = page.url
                    result["title"] = page.title()
                except Exception:
                    pass
        except Exception:
            pass
        log("\nREPLAY_RESULT=%s", json.dumps(result, ensure_ascii=False))
        if browser:
            try:
                browser.close()
            except Exception:
                pass
        if context:
            try:
                context.close()
            except Exception:
                pass
    if not result.get("ok"):
        sys.exit(1)


if __name__ == "__main__":
    main()