# 复刻主脚本步骤探测登录失败原因
import sys, time, json
sys.stdout.reconfigure(encoding='utf-8', line_buffering=True)
from playwright.sync_api import sync_playwright

def main():
    with sync_playwright() as p:
        b = p.chromium.launch(headless=True)
        ctx = b.new_context(viewport={"width":1440,"height":900}, locale="zh-CN")
        page = ctx.new_page()
        page.set_default_timeout(15000)
        page.goto("https://ipsapro.isoftstone.com/", wait_until="domcontentloaded", timeout=30000)
        # 等待 feilian
        deadline = time.time() + 70
        while time.time() < deadline:
            if page.locator("body").count() > 0 and "feilian" in page.url:
                break
            time.sleep(2)
        print("feilian url:", page.url[:100], flush=True)
        time.sleep(3)

        # 打开更多登录方式
        for sel in ["div.content_5klQOEKn", "div.container_SzPl3lwf"]:
            try:
                page.locator(sel).first.hover(timeout=5000)
                print(f"hovered {sel}", flush=True)
                time.sleep(1.5)
                break
            except Exception:
                continue
        time.sleep(2)
        print("items:", page.locator("div.item_oFCaWe7B").count(), flush=True)

        # 切换 iPSA
        loc = page.locator("div.item_oFCaWe7B").nth(1)
        loc.hover(timeout=5000)
        time.sleep(0.4)
        loc.locator("img").first.click(timeout=5000)
        time.sleep(1.5)
        print("account_input:", page.locator("#account_input").count(), flush=True)

        # 填账号（模仿 fill_final：click + fill + verify）
        page.locator("#account_input").first.click(timeout=5000)
        page.locator("#account_input").first.fill("junlong", timeout=5000)
        print("account filled:", page.locator("#account_input").first.input_value(), flush=True)

        # 按 Tab（主脚本有这步）
        try:
            page.locator("#account_input").press("Tab")
            print("pressed Tab", flush=True)
        except Exception as e:
            print("Tab err:", str(e)[:80], flush=True)

        # 填密码
        page.locator("#password_input").first.click(timeout=5000)
        page.locator("#password_input").first.fill("49718751L!abcd", timeout=5000)
        print("password filled:", page.locator("#password_input").first.input_value(), flush=True)

        # 勾选协议
        try:
            page.locator("div.agreement-container_yz9CPok6 label").click(timeout=5000)
            print("checkbox clicked", flush=True)
        except Exception as e:
            print("checkbox err:", str(e)[:100], flush=True)
        time.sleep(1)
        # 检查是否有 modal
        modal = page.evaluate("""() => {
            const m = document.querySelector('.arco-modal-wrapper');
            if (!m) return null;
            const vis = !!(m.offsetWidth || m.offsetHeight);
            return {visible: vis, text: (m.innerText||'').trim().slice(0,300)};
        }""")
        print("modal after checkbox:", json.dumps(modal, ensure_ascii=False), flush=True)
        print("checkbox checked state:", 
              page.evaluate("() => document.querySelector('input[type=checkbox]')?.checked"), flush=True)

        # 点击登录
        btns = page.locator("button")
        print(f"buttons: {btns.count()}", flush=True)
        for i in range(btns.count()):
            try:
                t = btns.nth(i).inner_text().strip()[:20]
                vis = btns.nth(i).is_visible()
                print(f"  btn[{i}] text={t!r} visible={vis} cls={btns.nth(i).get_attribute('class')}", flush=True)
            except Exception:
                pass
        try:
            page.locator("button.arco-btn.arco-btn-primary").click(timeout=5000)
            print("login clicked", flush=True)
        except Exception as e:
            print("login click err:", str(e)[:100], flush=True)

        # 观察 12 秒
        for i in range(12):
            time.sleep(1)
            url = page.url
            try:
                txt = page.evaluate("() => document.body.innerText.slice(0, 300)")
            except Exception:
                txt = ""
            print(f"t={i+1}s url={url[:80]} text={repr(txt[:100])}", flush=True)
            if "portal" in url or "SCMS" in url:
                print(">>> LOGIN SUCCESS!", flush=True)
                break
            if i == 5:
                page.screenshot(path="_probe_diag_login.png")
                print("saved _probe_diag_login.png", flush=True)

        ctx.close(); b.close()

if __name__ == "__main__":
    main()