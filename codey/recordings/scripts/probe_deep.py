# 深度探测 feilian 登录页：localStorage + 账号密码模式切换
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

        # Full localStorage dump
        ls = page.evaluate("""() => {
            const result = {};
            for (let i = 0; i < localStorage.length; i++) {
                const key = localStorage.key(i);
                let val = localStorage.getItem(key);
                if (val && val.length > 500) val = val.slice(0, 500) + '...';
                result[key] = val;
            }
            return result;
        }""")
        print(f"\nlocalStorage: {json.dumps(ls, ensure_ascii=False, indent=2)}", flush=True)

        # Check for signInPage data
        signin = page.evaluate("() => localStorage.getItem('signInPage')")
        if signin:
            print(f"\nsignInPage: {signin[:1000]}", flush=True)

        # Try to find current tab/mode state
        mode = page.evaluate("""() => {
            const root = document.getElementById('root');
            if (!root) return 'no root';
            return root.innerHTML.slice(0, 2000);
        }""")
        print(f"\nroot innerHTML: {mode}", flush=True)

        # Try to find React state / props
        state = page.evaluate("""() => {
            const root = document.getElementById('root');
            if (!root || !root._reactRootContainer) return 'no react root';
            return 'found react root';
        }""")
        print(f"\nReact state: {state}", flush=True)

        # Check URL query params
        url_params = page.evaluate("() => window.location.search")
        print(f"\nURL query params: {url_params}", flush=True)
        url_hash = page.evaluate("() => window.location.hash")
        print(f"URL hash: {url_hash}", flush=True)

        # Try to check if there's a global variable for sign-in mode
        global_vars = page.evaluate("""() => {
            const keys = Object.keys(window).filter(k => 
                k.toLowerCase().includes('sign') || 
                k.toLowerCase().includes('login') || 
                k.toLowerCase().includes('account') ||
                k.toLowerCase().includes('mode')
            );
            return keys.slice(0, 20);
        }""")
        print(f"window keys: {global_vars}", flush=True)

        # Try to find the SSO login URL - maybe we can go directly to passport
        print("\n--- Trying direct passport login ---", flush=True)
        page.goto("https://passport.isoftstone.com/?DomainUrl=https://ipsapro.isoftstone.com&ReturnUrl=%2Fportal", 
                   wait_until="domcontentloaded", timeout=30000)
        time.sleep(5)
        print(f"  passport url: {page.url[:120]}", flush=True)

        # Check if passport has account form
        for sel in ["#account_input", "#password_input", "input[type='text']", "input[type='password']"]:
            try:
                if page.locator(sel).count() > 0:
                    print(f"  passport has: {sel}", flush=True)
            except Exception:
                pass

        # Check if we can use feilian API directly
        ctx2 = b.new_context(viewport={"width":1280,"height":900}, locale="zh-CN")
        page2 = ctx2.new_page()
        # Try with a cookie that might enable account mode
        page2.context.add_cookies([{
            'name': 'signInPage',
            'value': 'account',
            'domain': '.feilian.isoftstone.com',
            'path': '/'
        }])
        page2.goto("https://feilian.isoftstone.com:10443/login?next=%2Fapi%2Foidc%2Fauthorize%3Fclient_id%3DcuhsYmdcwpEQrYjjckMArnOaEjZuIqbltJnxPVvA%26redirect_uri%3Dhttps%253A%252F%252Fpassport.isoftstone.com%253A443%252Fcorplink%252Fagw%252Fcallback%26response_type%3Dcode%26scope%3Dopenid%26state%3DeyJhY2Nlc3NfdXJsIjoiaHR0cHM6Ly9wYXNzcG9ydC5pc29mdHN0b25lLmNvbTo0NDMvP0RvbWFpblVybD1odHRwczovL2lwc2Fwcm8uaXNvZnRzdG9uZS5jb21cdTAwMjZSZXR1cm5Vcmw9JTJmcG9ydGFsYWd3X3VybF9oYXNodGFnIn0%3D",
                   wait_until="domcontentloaded", timeout=30000)
        time.sleep(8)
        print(f"\n  cookie-test url: {page2.url[:120]}", flush=True)
        for sel in ["#account_input", "#password_input", "[placeholder*='账号']", "[placeholder*='密码']"]:
            try:
                if page2.locator(sel).count() > 0:
                    print(f"  cookie-test has: {sel}", flush=True)
            except Exception:
                pass

        ctx.close()
        ctx2.close()
        b.close()

if __name__ == "__main__":
    main()