# 快速诊断：goto ipsapro 后 URL 变化链
import sys, time
sys.stdout.reconfigure(encoding="utf-8", errors="replace")
from playwright.sync_api import sync_playwright

def main():
    with sync_playwright() as p:
        b = p.chromium.launch(headless=True)
        ctx = b.new_context(viewport={"width": 1440, "height": 900}, locale="zh-CN")
        page = ctx.new_page()
        page.goto("https://ipsapro.isoftstone.com/", wait_until="domcontentloaded", timeout=30000)
        print("after goto:", page.url[:120], flush=True)
        for i in range(40):
            time.sleep(2)
            try:
                u = page.url
                body = page.locator("body").count()
                print(f"t={i*2}s url={u[:110]} body={body}", flush=True)
                if "feilian" in u:
                    print(">>> FEILIAN REACHED", flush=True)
                    break
            except Exception as e:
                print(f"t={i*2}s err={str(e)[:80]}", flush=True)
        # 打印页面文本前 200 字符
        try:
            txt = page.evaluate("() => document.body.innerText.slice(0, 200)")
            print("body text:", repr(txt), flush=True)
        except Exception as e:
            print("text err:", str(e)[:80], flush=True)
        ctx.close(); b.close()

if __name__ == "__main__":
    main()