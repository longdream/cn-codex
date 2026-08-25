# 精准探测 feilian 登录页：找到切换到账号密码模式的元素
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

        # Step 1: Check if account form already exists in DOM (hidden or visible)
        print("\n=== 检查账号密码表单是否存在 ===", flush=True)
        for sel in ["#account_input", "#password_input", "[placeholder*='账号']", "[placeholder*='密码']"]:
            for f in page.frames:
                try:
                    count = f.locator(sel).count()
                    vis = f.locator(sel).first.is_visible() if count > 0 else False
                    if count > 0:
                        print(f"  {sel}: count={count}, visible={vis} frame={f.url[:50]}", flush=True)
                except Exception as e:
                    pass

        # Step 2: Check if there's a hidden form
        hidden_inputs = page.evaluate("""() => {
            const inputs = document.querySelectorAll('input[type="text"], input:not([type]), input[type="password"]');
            return Array.from(inputs).map(i => ({
                id: i.id,
                placeholder: i.placeholder,
                type: i.type,
                visible: !!(i.offsetWidth || i.offsetHeight || i.getClientRects().length),
                rect: i.getBoundingClientRect().width > 0 ? `${i.getBoundingClientRect().width}x${i.getBoundingClientRect().height}` : '0x0'
            }));
        }""")
        print(f"\n所有输入框: {json.dumps(hidden_inputs, ensure_ascii=False)}", flush=True)

        # Step 3: Check if the form is inside a different container that needs toggling
        # 钉钉扫码区域 vs 账号密码区域
        page_structure = page.evaluate("""() => {
            const containers = document.querySelectorAll('[class*="container"], [class*="card"], [class*="box"]');
            const result = [];
            for (const c of containers) {
                const cls = Array.from(c.classList).join(' ').slice(0, 60);
                const vis = !!(c.offsetWidth || c.offsetHeight);
                const txt = (c.innerText || '').trim().slice(0, 80);
                const inputs = c.querySelectorAll('input').length;
                if (vis || inputs > 0) {
                    result.push({cls, vis, inputs, txt});
                }
            }
            return result.slice(0, 30);
        }""")
        print(f"\n页面容器结构: {json.dumps(page_structure, ensure_ascii=False, indent=2)}", flush=True)

        # Step 4: Check cookies/localStorage for login mode
        cookies = ctx.cookies()
        print(f"\ncookies ({len(cookies)}):", flush=True)
        for c in cookies:
            if 'login' in c['name'].lower() or 'mode' in c['name'].lower() or 'account' in c['name'].lower():
                print(f"  {c['name']}={c['value']}", flush=True)

        ls = page.evaluate("() => JSON.stringify(localStorage)")
        print(f"\nlocalStorage keys: {ls[:200] if ls else 'none'}", flush=True)

        # Step 5: Try to expand the "更多登录方式" popover
        print("\n=== 尝试展开更多登录方式 ===", flush=True)
        for sel in ["div.content_5klQOEKn", "div.container_SzPl3lwf", "div.arco-divider-horizontal-with-text"]:
            try:
                el = page.locator(sel).first
                if el.is_visible(timeout=2000):
                    el.hover(timeout=3000)
                    print(f"  hovered: {sel}", flush=True)
                    time.sleep(2)
                    break
            except Exception as e:
                print(f"  hover {sel} failed: {str(e)[:80]}", flush=True)

        # Step 6: Dump the 4 items
        items = page.locator("div.item_oFCaWe7B")
        n = items.count()
        print(f"\nitem count: {n}", flush=True)
        for i in range(n):
            it = items.nth(i)
            try:
                outer = it.evaluate("el => el.outerHTML.slice(0, 300)")
                text = it.evaluate("el => (el.innerText || '').trim().slice(0, 30)")
                tag = it.evaluate("el => el.tagName")
                visible = it.is_visible(timeout=1000)
                print(f"\nitem[{i}]: tag={tag} visible={visible}", flush=True)
                print(f"  text: {repr(text)}", flush=True)
                print(f"  html: {outer}", flush=True)
                # Try clicking
                it.click(timeout=3000, force=True)
                time.sleep(2)
                print(f"  clicked item[{i}]", flush=True)
                # Check for account form
                for sel in ["#account_input", "#password_input", "[placeholder*='账号']", "[placeholder*='密码']"]:
                    for f in page.frames:
                        try:
                            if f.locator(sel).count() > 0:
                                print(f"  ✓ FOUND {sel} in frame {f.url[:40]}", flush=True)
                                page.screenshot(path=f"_found_item{i}.png")
                                ctx.close()
                                b.close()
                                print(f"\nSUCCESS: item[{i}] switches to account mode!", flush=True)
                                return
                        except Exception:
                            pass
                print(f"  after click url: {page.url[:100]}", flush=True)
                # Check if page changed (DingTalk redirect)
                if "dingtalk" in page.url or "oapi" in page.url:
                    print(f"  → redirected to DingTalk! Going back...", flush=True)
                    page.go_back()
                    time.sleep(3)
            except Exception as e:
                print(f"item[{i}] err: {str(e)[:120]}", flush=True)

        # Step 7: If nothing worked, try to directly show account form via JS
        print("\n=== 尝试直接 JS 显示账号密码表单 ===", flush=True)
        result = page.evaluate("""() => {
            const forms = document.querySelectorAll('form');
            for (const f of forms) {
                f.style.display = 'block';
                f.style.visibility = 'visible';
                f.style.opacity = '1';
            }
            const inputs = document.querySelectorAll('input');
            for (const inp of inputs) {
                inp.style.display = 'block';
                inp.style.visibility = 'visible';
            }
            return Array.from(inputs).map(i => i.id || i.placeholder || i.name).filter(Boolean);
        }""")
        print(f"  after JS: inputs={result}", flush=True)
        time.sleep(2)
        for sel in ["#account_input", "#password_input"]:
            if page.locator(sel).count() > 0:
                print(f"  ✓ {sel} now visible!", flush=True)

        ctx.close()
        b.close()
        print("\nFAILED: could not find account input", flush=True)

if __name__ == "__main__":
    main()