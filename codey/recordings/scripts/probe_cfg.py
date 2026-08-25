# 检查 feilian signInPage 配置：找账号密码登录组件
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
        print("url:", page.url[:120], flush=True)
        page.wait_for_selector("body", timeout=30000)
        time.sleep(8)

        signin = page.evaluate("() => localStorage.getItem('signInPage')")
        print(f"signInPage length: {len(signin) if signin else 0}", flush=True)
        # 找 component_name 列表
        names = page.evaluate("""() => {
            const s = localStorage.getItem('signInPage') || '';
            const re = /"component_name":"([^"]+)"/g;
            const found = [];
            let m;
            while ((m = re.exec(s)) !== null) found.push(m[1]);
            return found;
        }""")
        print(f"component names: {json.dumps(names, ensure_ascii=False)}", flush=True)

        # 找 CoreCard 的相关内容
        core = page.evaluate("""() => {
            const s = localStorage.getItem('signInPage') || '';
            const idx = s.indexOf('CoreCard');
            if (idx < 0) return 'no CoreCard';
            return s.slice(idx, idx + 1500);
        }""")
        print(f"\nCoreCard section: {core}", flush=True)

        # 全量落盘到文件供本地分析
        try:
            with open("_signin_cfg.json", "w", encoding="utf-8") as f:
                f.write(signin or "")
            print("\nsaved _signin_cfg.json", flush=True)
        except Exception as e:
            print("save err:", e, flush=True)

        # 看看所有 srcdoc iframe 内容
        print("\n--- frames ---", flush=True)
        for i, f in enumerate(page.frames):
            try:
                t = f.evaluate("() => document.body ? document.body.innerText.slice(0, 300) : ''")
                print(f"frame[{i}] {f.url[:60]}: {repr(t[:150])}", flush=True)
            except Exception as e:
                print(f"frame[{i}] err: {str(e)[:80]}", flush=True)

        # 尝试 hover 更多登录方式后截图
        try:
            page.locator("div.content_5klQOEKn").first.hover(timeout=3000)
            time.sleep(2)
            page.screenshot(path="_more_login.png")
            print("\nsaved _more_login.png", flush=True)
            # 展开后把所有按钮/图标/文本列出来
            info = page.evaluate("""() => {
                const items = document.querySelectorAll('div.item_oFCaWe7B');
                return Array.from(items).map(it => ({
                    html: it.outerHTML.slice(0, 200),
                    title: it.getAttribute('title') || '',
                    aria: it.getAttribute('aria-label') || '',
                    tooltip: it.className
                }));
            }""")
            print(f"\nitems: {json.dumps(info, ensure_ascii=False, indent=1)}", flush=True)
        except Exception as e:
            print("hover err:", str(e)[:100], flush=True)

        ctx.close()
        b.close()

if __name__ == "__main__":
    main()