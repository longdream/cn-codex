# 探测 hover 展开详情
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

        # hover 到更多登录方式
        try:
            page.locator("div.content_5klQOEKn").hover(timeout=5000)
            print("hovered content_5klQOEKn", flush=True)
        except Exception as e:
            print("hover err:", str(e)[:120], flush=True)
        time.sleep(2)

        # 截图
        page.screenshot(path="_probe_hover.png")
        print("screenshot saved", flush=True)

        # 检查 hover 后 DOM 变化：img / svg / icon
        icons = page.evaluate("""() => {
          const out = [];
          const els = document.querySelectorAll('img, svg, i, [class*=icon], [class*=Icon], [class*=item]');
          for (const e of els) {
            const vis = !!(e.offsetWidth || e.offsetHeight || e.getClientRects().length);
            if (!vis) continue;
            const src = e.src ? e.src.slice(0, 60) : '';
            const cls = String(e.className).slice(0, 60);
            const title = e.title || e.getAttribute('aria-label') || '';
            out.push({tag: e.tagName, cls: cls, src: src, title: title});
          }
          const seen = new Set(); const res = [];
          for (const o of out) { const k = o.tag + '|' + o.cls + '|' + o.src + '|' + o.title; if (!seen.has(k)) { seen.add(k); res.push(o); } }
          return res.slice(0, 60);
        }""")
        print("icons:", json.dumps(icons, ensure_ascii=False), flush=True)

        # 点击 item_oFCaWe7B img（若有）
        try:
            it = page.locator("div.item_oFCaWe7B")
            print("item count:", it.count(), flush=True)
            if it.count() > 0:
                # 查看其内部 img src
                imgs = it.first.locator("img")
                print("item imgs:", imgs.count(), flush=True)
                for i in range(imgs.count()):
                    print("  img src:", imgs.nth(i).get_attribute("src"), flush=True)
                if imgs.count() > 0:
                    imgs.first.click(timeout=3000)
                    print("clicked item img", flush=True)
                    time.sleep(2)
                    # 再截图
                    page.screenshot(path="_probe_after_click.png")
        except Exception as e:
            print("item err:", str(e)[:150], flush=True)

        # 最终检查是否有账号密码输入框
        for sel in ["#account_input", "[placeholder*='账号']", "#password_input", "[placeholder*='密码']"]:
            fr, loc = None, None
            for f in page.frames:
                try:
                    l = f.locator(sel)
                    if l.count() > 0:
                        fr, loc = f, l
                        break
                except Exception:
                    pass
            print(f"{sel}: found={loc is not None} frame={fr.url[:40] if fr else None}", flush=True)

        ctx.close(); b.close()

if __name__ == "__main__":
    main()