# 探测 feilian 登录页 iframe 内部结构
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

        for idx, fr in enumerate(page.frames):
            print(f"\n=== frame {idx}: {fr.url[:100]} ===", flush=True)
            try:
                info = fr.evaluate("""() => {
                  const seen = new Set();
                  const out = [];
                  const els = document.querySelectorAll('input, button, [role=button], a, select, textarea, [class*=item], [class*=tab], [class*=radio]');
                  for (const e of els) {
                    const t = (e.innerText || e.textContent || '').trim().slice(0, 40);
                    const ph = e.placeholder || '';
                    const vis = !!(e.offsetWidth || e.offsetHeight || e.getClientRects().length);
                    const key = t + '|' + ph + '|' + e.tagName + '|' + String(e.className).slice(0, 40);
                    if (seen.has(key)) continue;
                    seen.add(key);
                    if (vis && (e.tagName === 'INPUT' || e.tagName === 'TEXTAREA' || e.tagName === 'SELECT' ||
                        (e.tagName === 'BUTTON' && t) || (e.tagName === 'A' && t) || t || ph))
                      out.push({tag: e.tagName, text: t, ph: ph, type: e.type, cls: String(e.className).slice(0, 60), vis: vis});
                  }
                  return out.slice(0, 100);
                }""")
                print("elements:", json.dumps(info, ensure_ascii=False), flush=True)
            except Exception as e:
                print("frame err:", str(e)[:200], flush=True)

        ctx.close(); b.close()

if __name__ == "__main__":
    main()