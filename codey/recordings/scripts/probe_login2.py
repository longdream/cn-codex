# 探测登录后页面状态（是否有错误提示）
import sys, time, json
sys.stdout.reconfigure(encoding='utf-8', line_buffering=True)
from playwright.sync_api import sync_playwright

def find_in_frames(page, sel):
    for f in page.frames:
        try:
            if f.locator(sel).count() > 0:
                return f
        except Exception:
            pass
    return None

def main():
    with sync_playwright() as p:
        b = p.chromium.launch(headless=True)
        ctx = b.new_context(viewport={"width":1280,"height":900}, locale="zh-CN")
        page = ctx.new_page()
        page.goto("https://ipsapro.isoftstone.com/", wait_until="domcontentloaded", timeout=30000)
        for _ in range(60):
            if "feilian" in page.url:
                break
            page.wait_for_timeout(500)
        print("url:", page.url[:100], flush=True)
        page.wait_for_selector("body", timeout=30000)
        time.sleep(6)

        # 打开更多登录方式
        for sel in ["div.content_5klQOEKn", "div.container_SzPl3lwf"]:
            try:
                page.locator(sel).first.hover(timeout=5000)
                print(f"hovered {sel}", flush=True)
                time.sleep(1.5)
                break
            except Exception:
                continue

        # 点击 item[1] (iPSA)
        loc = page.locator("div.item_oFCaWe7B").nth(1)
        loc.hover(timeout=5000)
        time.sleep(0.4)
        loc.click(timeout=5000)
        time.sleep(2)
        print("switched:", page.locator("#account_input").count() > 0, flush=True)

        # 填写账号密码
        page.locator("#account_input").click()
        page.locator("#account_input").fill("junlong")
        page.locator("#password_input").click()
        page.locator("#password_input").fill("49718751L!abcd")
        print("filled", flush=True)
        time.sleep(1)

        # 截图：填写后的表单
        page.screenshot(path="_probe_before_login.png")

        # 点击协议复选框
        try:
            page.locator("div.agreement-container_yz9CPok6 label").click(timeout=5000)
            print("checkbox clicked", flush=True)
        except Exception as e:
            print("checkbox err:", str(e)[:100], flush=True)
        time.sleep(0.5)

        # 点击登录
        try:
            page.locator("button.arco-btn.arco-btn-primary").click(timeout=5000)
            print("login clicked", flush=True)
        except Exception as e:
            print("login err:", str(e)[:100], flush=True)

        # 观察 15 秒内的 URL 变化 + 页面文本
        for i in range(20):
            time.sleep(1)
            url = page.url
            try:
                txt = page.evaluate("() => document.body.innerText.slice(0, 400)")
            except Exception:
                txt = ""
            print(f"t={i+1}s url={url[:80]} text={repr(txt[:120])}", flush=True)
            if "portal" in url or "ipsapro.isoftstone.com/SCMS" in url:
                print(">>> LOGIN SUCCESS!", flush=True)
                break
            if i == 6:
                page.screenshot(path="_probe_after_login.png")

        ctx.close(); b.close()

if __name__ == "__main__":
    main()