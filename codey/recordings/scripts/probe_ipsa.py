# 验证：hover item[1] (iPSA) 后再点击，切换到账号密码模式
import sys, time, json
sys.stdout.reconfigure(encoding='utf-8', line_buffering=True)
from playwright.sync_api import sync_playwright

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
        time.sleep(8)

        # 打开更多登录方式
        try:
            page.locator("div.content_5klQOEKn").first.hover(timeout=3000)
            print("hovered content_5klQOEKn", flush=True)
        except Exception as e:
            print("hover err:", str(e)[:100], flush=True)
        time.sleep(2)

        # 获取 item[1] (iPSA) 并 hover 以打开 tooltip
        it = page.locator("div.item_oFCaWe7B").nth(1)
        try:
            it.hover(timeout=3000)
            print("hovered item[1] (iPSA)", flush=True)
            time.sleep(1.5)
        except Exception as e:
            print("hover err:", str(e)[:100], flush=True)

        # 检查 tooltip 是否出现
        tips = page.evaluate("""() => {
            const els = document.querySelectorAll('div[class*="tooltip"], span[class*="tooltip"]');
            return Array.from(els).filter(e => !!(e.offsetWidth||e.offsetHeight)).map(e => e.innerText.trim()).filter(Boolean);
        }""")
        print(f"visible tooltips: {json.dumps(tips, ensure_ascii=False)}", flush=True)

        # 现在点击 item[1]
        try:
            it.click(timeout=5000)
            print("clicked item[1]", flush=True)
        except Exception as e:
            print("click err:", str(e)[:100], flush=True)

        time.sleep(3)

        # 检查是否切换到账号密码模式
        print(f"after click url: {page.url[:100]}", flush=True)
        text = page.evaluate("() => document.body.innerText.slice(0, 500)")
        print(f"body text: {repr(text[:200])}", flush=True)

        for sel in ["#account_input", "#password_input", "[placeholder*='账号']", "[placeholder*='密码']"]:
            for f in page.frames:
                try:
                    if f.locator(sel).count() > 0:
                        print(f"  ✓ FOUND {sel} in frame {f.url[:40]}", flush=True)
                except Exception:
                    pass

        # 截图确认
        page.screenshot(path="_probe_ipsa_click.png")
        print("saved _probe_ipsa_click.png", flush=True)

        ctx.close()
        b.close()

if __name__ == "__main__":
    main()