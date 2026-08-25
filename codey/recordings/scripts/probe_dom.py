# 探测 feilian 登录页结构
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
        # 等渲染
        page.wait_for_selector("body", timeout=30000)
        time.sleep(10)

        # 所有按钮和可点击元素
        info = page.evaluate("""() => {
          const seen = new Set();
          const out = [];
          const els = document.querySelectorAll('button, [role=button], a, .arco-tabs-tab, [class*=item], [class*=tab]');
          for (const e of els) {
            const t = (e.innerText || e.textContent || '').trim().slice(0, 40);
            const vis = !!(e.offsetWidth || e.offsetHeight || e.getClientRects().length);
            if (!vis) continue;
            const key = t + '|' + e.tagName + '|' + (e.className && String(e.className).slice(0, 50));
            if (seen.has(key)) continue;
            seen.add(key);
            out.push({tag: e.tagName, text: t, cls: String(e.className).slice(0, 60), id: e.id});
          }
          return out.slice(0, 80);
        }""")
        print("elements:", json.dumps(info, ensure_ascii=False), flush=True)

        # 所有 input
        infos = page.eval_on_selector_all("input", "els => els.map(e => ({id:e.id, ph:e.placeholder, type:e.type, vis:!!(e.offsetWidth||e.offsetHeight)}))")
        print("inputs:", json.dumps(infos, ensure_ascii=False), flush=True)

        # ract 渲染的窗口/iframe
        frames = [f.url[:80] for f in page.frames]
        print("frames:", json.dumps(frames, ensure_ascii=False), flush=True)

        ctx.close(); b.close()

if __name__ == "__main__":
    main()