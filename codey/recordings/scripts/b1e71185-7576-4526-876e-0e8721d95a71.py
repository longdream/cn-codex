"""
Playwright 回放脚本 - 录制: recording-b1e71185
录制时间: 2026-08-24T14:48:30Z
描述: 飞连(Feilian) SSO 登录流程 - 输入账号密码并登录
"""

from playwright.sync_api import sync_playwright, TimeoutError as PwTimeoutError
import sys
import json
import traceback


def main():
    replay_result = {"ok": True, "step": "", "url": "", "title": ""}
    browser = None
    context = None
    page = None

    try:
        with sync_playwright() as p:
            # 启动浏览器
            browser = p.chromium.launch(
                headless=False,
                args=["--disable-blink-features=AutomationControlled"]
            )
            context = browser.new_context(
                viewport={"width": 1280, "height": 720},
                user_agent=(
                    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) "
                    "AppleWebKit/537.36 (KHTML, like Gecko) "
                    "Chrome/120.0.0.0 Safari/537.36"
                ),
                # 忽略 HTTPS 证书错误（内网域名）
                ignore_https_errors=True,
            )
            page = context.new_page()

            # ============================================================
            # 步骤 1：打开飞连登录页
            # 说明：trace 中的 navigate 均为 cause=redirect（SSO 自动跳转），
            # 无 cause=user 事件。直接 goto 到飞连登录页作为回放起点。
            # ============================================================
            step = 1
            print(f"步骤 {step}：打开飞连登录页 - feilian.isoftstone.com:10443/login", flush=True)
            page.goto(
                "https://feilian.isoftstone.com:10443/login"
                "?next=%2Fapi%2Foidc%2Fauthorize"
                "%3Fclient_id%3DcuhsYmdcwpEQrYjjckMArnOaEjZuIqbltJnxPVvA"
                "%26redirect_uri%3Dhttps%253A%252F%252Fpassport.isoftstone.com"
                "%253A443%252Fcorplink%252Fagw%252Fcallback"
                "%26response_type%3Dcode%26scope%3Dopenid"
                "%26state%3DeyJhY2Nlc3NfdXJsIjoiaHR0cHM6Ly9wYXNzcG9ydC5pc29mdHN0b25lLmNvbTo0NDMvP0RvbWFpblVybD1odHRwczovL2lwc2Fwcm8uaXNvZnRzdG9uZS5jb21cdTAwMjZSZXR1cm5Vcmw9JTJmcG9ydGFsYWd3X3VybF9oYXNodGFnIn0%3D",
                wait_until="networkidle",
                timeout=60000
            )
            page.wait_for_load_state("domcontentloaded")
            page.wait_for_timeout(1000)
            current_url = page.url
            current_title = page.title()
            print(f"  当前 URL: {current_url}", flush=True)
            print(f"  页面标题: {current_title}", flush=True)

            # ============================================================
            # 步骤 2：点击「更多登录方式」中的 iPSA 图标切换到账号密码登录
            # trace 事件: click div.item_oFCaWe7B.arco-tooltip-open
            # 说明: 4 个图标依次为 WeLink / iPSA / 企业微信 / 飞书，
            #       点击 iPSA（第 2 个）后出现 #account_input 账号密码表单
            # ============================================================
            step = 2
            print(f"步骤 {step}：点击 iPSA 图标切换到账号密码登录方式", flush=True)
            tab_selector = "div.item_oFCaWe7B >> nth=1"
            try:
                page.wait_for_selector(tab_selector, timeout=10000)
                page.click(tab_selector)
                page.wait_for_timeout(800)
                print("  已点击 iPSA 登录切换", flush=True)
            except Exception:
                print("  未找到 iPSA 切换图标，尝试直接等待账号表单", flush=True)

            # ============================================================
            # 步骤 3：点击账号输入框 #account_input
            # trace 事件: click #account_input
            # ============================================================
            step = 3
            print(f"步骤 {step}：点击账号输入框", flush=True)
            account_sel = "#account_input"
            try:
                page.wait_for_selector(account_sel, timeout=10000)
            except Exception:
                # 尝试备用选择器
                account_sel = "[placeholder=\"请输入账号\"]"
                page.wait_for_selector(account_sel, timeout=10000)
            page.click(account_sel)
            page.wait_for_timeout(300)
            print("  已聚焦账号输入框", flush=True)

            # ============================================================
            # 步骤 4：输入账号 "junlong"
            # trace 中的 type 事件合并为最终值，跳过中间 KEYIN
            # ============================================================
            step = 4
            print(f"步骤 {step}：输入账号 junlong", flush=True)
            page.fill(account_sel, "junlong")
            page.wait_for_timeout(300)
            print("  已输入账号", flush=True)

            # ============================================================
            # 步骤 5：按 Tab 键切换到密码框
            # trace 事件: key Tab on #account_input
            # ============================================================
            step = 5
            print(f"步骤 {step}：按 Tab 键切换到密码框", flush=True)
            page.press(account_sel, "Tab")
            page.wait_for_timeout(500)

            # ============================================================
            # 步骤 6：点击密码输入框 #password_input
            # trace 事件: click 后连续 type
            # ============================================================
            step = 6
            print(f"步骤 {step}：点击密码输入框", flush=True)
            password_sel = "#password_input"
            try:
                page.wait_for_selector(password_sel, timeout=10000)
            except Exception:
                password_sel = "[placeholder=\"请输入密码\"]"
                page.wait_for_selector(password_sel, timeout=10000)
            page.click(password_sel)
            page.wait_for_timeout(300)
            print("  已聚焦密码输入框", flush=True)

            # ============================================================
            # 步骤 7：输入密码 "49718751L!abcd"
            # trace 中所有 type 事件合并为最终值
            # ============================================================
            step = 7
            print(f"步骤 {step}：输入密码", flush=True)
            page.fill(password_sel, "49718751L!abcd")
            page.wait_for_timeout(300)
            print("  已输入密码", flush=True)

            # ============================================================
            # 步骤 8：勾选用户协议复选框
            # trace 事件: click div.arco-checkbox-mask; click label.arco-checkbox
            # ============================================================
            step = 8
            print(f"步骤 {step}：勾选同意协议复选框", flush=True)
            checkbox_clicked = False
            checkbox_candidates = [
                "div.arco-checkbox-mask",
                "label.arco-checkbox >> nth=0",
                "label.arco-checkbox input",
                "div.agreement-container_yz9CPok6",
            ]
            for sel in checkbox_candidates:
                try:
                    if page.locator(sel).count() > 0:
                        page.click(sel)
                        checkbox_clicked = True
                        print(f"  已勾选协议 (选择器: {sel})", flush=True)
                        break
                except Exception:
                    continue
            if not checkbox_clicked:
                print("  警告: 未找到复选框元素，尝试通过 JavaScript 勾选", flush=True)
                try:
                    label = page.locator("label.arco-checkbox").first
                    page.evaluate("el => el.click()", label.element_handle())
                    checkbox_clicked = True
                except Exception:
                    print("  跳过复选框勾选", flush=True)
            page.wait_for_timeout(500)

            # ============================================================
            # 步骤 9：点击登录按钮
            # trace 事件: click button.arco-btn.arco-btn-primary + submit form
            # ============================================================
            step = 9
            print(f"步骤 {step}：点击登录按钮", flush=True)
            login_btn_sel = "button.arco-btn.arco-btn-primary"
            try:
                page.wait_for_selector(login_btn_sel, timeout=10000)
                page.click(login_btn_sel)
                print("  已点击登录按钮", flush=True)
            except Exception:
                print("  未找到主按钮，尝试 form 中的 submit 按钮", flush=True)
                page.eval_on_selector(
                    "form.arco-form",
                    "el => el.querySelector('button[type=submit]')?.click()"
                )

            # ============================================================
            # 步骤 10：等待登录后跳转
            # trace 中登录后 navigate 到 passport callback → ipsapro/portal
            # ============================================================
            step = 10
            print(f"步骤 {step}：等待登录完成并跳转", flush=True)
            # 等待跳转到 ipsapro 或 passport 回调页
            target_hosts = ["ipsapro.isoftstone.com", "passport.isoftstone.com"]
            navigated = False
            for host in target_hosts:
                try:
                    page.wait_for_url(f"**/{host}/**", timeout=30000)
                    navigated = True
                    print(f"  已跳转到: {host}", flush=True)
                    break
                except PwTimeoutError:
                    continue
            if not navigated:
                # 等待页面加载，可能仍在当前域
                page.wait_for_timeout(5000)
                print(f"  未检测到跳转，当前 URL: {page.url}", flush=True)

            # 等待页面稳定
            try:
                page.wait_for_load_state("networkidle", timeout=15000)
            except Exception:
                pass
            page.wait_for_timeout(1000)

            final_url = page.url
            final_title = page.title()
            print(f"  最终 URL: {final_url}", flush=True)
            print(f"  最终页面标题: {final_title}", flush=True)

            # === 成功结果 ===
            replay_result["step"] = f"step_{step}"
            replay_result["url"] = final_url
            replay_result["title"] = final_title
            print(f"REPLAY_RESULT={json.dumps(replay_result, ensure_ascii=False)}", flush=True)

    except Exception as e:
        replay_result["ok"] = False
        replay_result["step"] = f"step_{step}" if "step" in dir() else "setup"
        replay_result["error"] = f"{type(e).__name__}: {str(e)}"
        replay_result["traceback"] = traceback.format_exc()
        print(f"REPLAY_RESULT={json.dumps(replay_result, ensure_ascii=False)}", flush=True)
        sys.exit(1)

    finally:
        # 浏览器关闭包在 try/except 中，用户手动关闭窗口不视为失败
        try:
            if context:
                context.close()
        except Exception:
            pass
        try:
            if browser:
                browser.close()
        except Exception:
            pass


if __name__ == "__main__":
    main()