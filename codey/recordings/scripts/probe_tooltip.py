# 探测每个 item 的 tooltip 文本 + 点击后的页面变化
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
        time.sleep(10)

        # 打开更多登录方式
        try:
            page.locator("div.content_5klQOEKn").first.hover(timeout=3000)
            print("hovered content_5klQOEKn", flush=True)
        except Exception as e:
            print("hover err:", str(e)[:100], flush=True)
        time.sleep(2)

        items = page.locator("div.item_oFCaWe7B")
        n = items.count()
        print("item count:", n, flush=True)

        # 逐个 hover 看 tooltip
        for i in range(n):
            it = items.nth(i)
            try:
                it.hover(timeout=3000, force=True)
                time.sleep(1.5)
                # 读取所有可见 tooltip
                tips = page.evaluate("""() => {
                    const els = document.querySelectorAll('div[class*="tooltip"], span[class*="tooltip"]');
                    const out = [];
                    for (const e of els) {
                        const vis = !!(e.offsetWidth || e.offsetHeight);
                        const t = (e.innerText || '').trim();
                        if (vis && t) out.push({cls: e.className.slice(0,60), text: t.slice(0,40)});
                    }
                    return out;
                }""")
                print(f"\n--- item[{i}] tooltips: {json.dumps(tips, ensure_ascii=False)}", flush=True)
            except Exception as e:
                print(f"item[{i}] hover err: {str(e)[:80]}", flush=True)

        ctx.close()
        b.close()

if __name__ == "__main__":
    main()