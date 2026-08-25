# 全面 dump feilian 登录页文本与控件
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
        time.sleep(12)

        # 1) 主页面可见文本
        txt = page.evaluate("""() => {
          const b = document.body;
          const all = b.innerText.split('\\n').map(s => s.trim()).filter(s => s);
          return all.slice(0, 120);
        }""")
        print("\n--- body text ---", flush=True)
        print(json.dumps(txt, ensure_ascii=False), flush=True)

        # 2) 主页面所有可见 div 带 class 的（前 60）
        divs = page.evaluate("""() => {
          const out = [];
          const els = document.querySelectorAll('div[class]');
          for (const e of els) {
            const vis = !!(e.offsetWidth || e.offsetHeight || e.getClientRects().length);
            if (!vis) continue;
            const cls = String(e.className);
            if (cls.length < 4) continue;
            const t = (e.innerText || '').trim().slice(0, 30);
            out.push({cls: cls.slice(0, 70), t: t});
          }
          const seen = new Set(); const res = [];
          for (const o of out) { const k = o.cls + '|' + o.t; if (!seen.has(k)) {seen.add(k); res.push(o);} }
          return res.slice(0, 80);
        }""")
        print("\n--- visible divs ---", flush=True)
        print(json.dumps(divs, ensure_ascii=False), flush=True)

        # 3) iframe 内容
        for idx, fr in enumerate(page.frames):
            if fr.url.startswith("about"):
                try:
                    t2 = fr.evaluate("() => document.body ? document.body.innerText.slice(0, 500) : ''")
                    print(f"\n--- srcdoc frame text --- {t2}", flush=True)
                except Exception as e:
                    print("frame err:", str(e)[:150], flush=True)

        ctx.close(); b.close()

if __name__ == "__main__":
    main()