import fs from "node:fs/promises";
import path from "node:path";
import { chromium } from "playwright";

const input = await readStdinJson();
const startedAt = Date.now();
const screenshots = [];
const assetBundles = [];
const actionResults = [];
const consoleMessages = [];
const launchNotes = [];

const defaultScreenshotDir = input.defaultScreenshotDir || process.cwd();
const defaultAssetDir = input.defaultAssetDir || path.join(defaultScreenshotDir, "assets");
await fs.mkdir(defaultScreenshotDir, { recursive: true });
await fs.mkdir(defaultAssetDir, { recursive: true });

const browserSession = await openBrowserSession(input);
const { browser, context, page, closeBrowser, mode } = browserSession;
let activePage = page;

try {
  attachPageListeners(activePage);
  context.on("page", (newPage) => attachPageListeners(newPage));

  if (input.url) {
    await activePage.goto(input.url, {
      waitUntil: input.waitUntil || "networkidle",
      timeout: input.navigationTimeoutMs || 30_000,
    });
  }

  const actions = Array.isArray(input.actions) ? input.actions : [];
  for (let index = 0; index < actions.length; index += 1) {
    const actionResult = await runAction(context, activePage, actions[index], index);
    if (actionResult.activePage) {
      activePage = actionResult.activePage;
      delete actionResult.activePage;
    }
    actionResults.push(actionResult);
  }

  if (input.storageStatePath) {
    await fs.mkdir(path.dirname(input.storageStatePath), { recursive: true });
    await context.storageState({ path: input.storageStatePath });
  }

  const output = {
    ok: true,
    browserMode: mode,
    cdpEndpoint: input.cdpEndpoint || null,
    finalUrl: activePage.url(),
    title: await activePage.title(),
    tabs: await describeTabs(context, activePage),
    durationMs: Date.now() - startedAt,
    actions: actionResults,
    screenshots,
    assetBundles,
    console: consoleMessages.slice(-50),
    notes: launchNotes,
  };
  console.log(JSON.stringify(output, null, 2));
} finally {
  if (closeBrowser) {
    await browser.close();
  }
}

async function runAction(context, page, action, index) {
  const type = String(action?.type || "").trim();
  const timeout = action?.timeoutMs || 10_000;
  switch (type) {
    case "goto": {
      await page.goto(requiredString(action, "url"), {
        waitUntil: action.waitUntil || "networkidle",
        timeout: action.navigationTimeoutMs || 30_000,
      });
      return result(index, type, { url: page.url() });
    }
    case "click": {
      if (typeof action.selector === "string" && action.selector.trim()) {
        await page.click(action.selector, { timeout });
        return result(index, type, { selector: action.selector });
      }
      const x = requiredNumber(action, "x");
      const y = requiredNumber(action, "y");
      await page.mouse.click(x, y);
      return result(index, type, { x, y });
    }
    case "fill": {
      await page.fill(requiredString(action, "selector"), String(action.text ?? ""), { timeout });
      return result(index, type, { selector: action.selector, length: String(action.text ?? "").length });
    }
    case "type": {
      if (typeof action.selector === "string" && action.selector.trim()) {
        await page.locator(action.selector).type(String(action.text ?? ""), { timeout });
      } else {
        await page.keyboard.type(String(action.text ?? ""));
      }
      return result(index, type, { selector: action.selector ?? null, length: String(action.text ?? "").length });
    }
    case "press": {
      const key = requiredString(action, "key");
      if (typeof action.selector === "string" && action.selector.trim()) {
        await page.locator(action.selector).press(key, { timeout });
      } else {
        await page.keyboard.press(key);
      }
      return result(index, type, { key, selector: action.selector ?? null });
    }
    case "hover": {
      await page.hover(requiredString(action, "selector"), { timeout });
      return result(index, type, { selector: action.selector });
    }
    case "check": {
      await page.locator(requiredString(action, "selector")).check({ timeout });
      return result(index, type, { selector: action.selector });
    }
    case "uncheck": {
      await page.locator(requiredString(action, "selector")).uncheck({ timeout });
      return result(index, type, { selector: action.selector });
    }
    case "select_option": {
      const selector = requiredString(action, "selector");
      const value = action.value ?? action.values ?? action.label ?? action.index;
      if (value == null) {
        throw new Error("Action 'select_option' requires value, values, label, or index");
      }
      await page.locator(selector).selectOption(value, { timeout });
      return result(index, type, { selector, value: jsonSafe(value) });
    }
    case "wait_for_selector": {
      await page.waitForSelector(requiredString(action, "selector"), { timeout });
      return result(index, type, { selector: action.selector });
    }
    case "wait_for_timeout": {
      const ms = Math.max(0, Math.min(requiredNumber(action, "ms"), 60_000));
      await page.waitForTimeout(ms);
      return result(index, type, { ms });
    }
    case "screenshot": {
      const screenshotPath = resolveScreenshotPath(action.path, index);
      await fs.mkdir(path.dirname(screenshotPath), { recursive: true });
      await page.screenshot({
        path: screenshotPath,
        fullPage: Boolean(action.fullPage ?? action.full_page),
      });
      screenshots.push(screenshotPath);
      return result(index, type, { path: screenshotPath });
    }
    case "set_viewport": {
      const width = requiredInteger(action, "width");
      const height = requiredInteger(action, "height");
      if (width < 320 || height < 240) {
        throw new Error("Action 'set_viewport' requires width >= 320 and height >= 240");
      }
      await page.setViewportSize({ width, height });
      return result(index, type, { width, height });
    }
    case "reload": {
      await page.reload({
        waitUntil: action.waitUntil || "networkidle",
        timeout: action.navigationTimeoutMs || 30_000,
      });
      return result(index, type, { url: page.url(), title: await page.title() });
    }
    case "back": {
      await page.goBack({
        waitUntil: action.waitUntil || "networkidle",
        timeout: action.navigationTimeoutMs || 30_000,
      });
      return result(index, type, { url: page.url(), title: await page.title() });
    }
    case "forward": {
      await page.goForward({
        waitUntil: action.waitUntil || "networkidle",
        timeout: action.navigationTimeoutMs || 30_000,
      });
      return result(index, type, { url: page.url(), title: await page.title() });
    }
    case "title": {
      return result(index, type, { title: await page.title() });
    }
    case "url": {
      return result(index, type, { url: page.url() });
    }
    case "html": {
      const html = await page.content();
      return result(index, type, { html: truncate(html, action.maxChars || 8_000) });
    }
    case "snapshot": {
      const snapshot = await page.evaluate((maxItems) => {
        const text = document.body?.innerText || "";
        const serialize = (element) => {
          const rect = element.getBoundingClientRect();
          return {
            tag: element.tagName.toLowerCase(),
            text: (element.innerText || element.value || element.alt || element.title || "").trim().slice(0, 160),
            id: element.id || null,
            name: element.getAttribute("name"),
            type: element.getAttribute("type"),
            href: element.href || null,
            role: element.getAttribute("role"),
            ariaLabel: element.getAttribute("aria-label"),
            placeholder: element.getAttribute("placeholder"),
            visible: rect.width > 0 && rect.height > 0,
          };
        };
        const controls = Array.from(document.querySelectorAll("button,a,input,textarea,select,[role='button'],[role='link']"))
          .slice(0, maxItems)
          .map(serialize);
        return {
          title: document.title,
          url: location.href,
          text: text.slice(0, 6_000),
          controls,
        };
      }, action.maxItems || action.max_items || 80);
      return result(index, type, { snapshot });
    }
    case "assets": {
      const assets = await collectPageAssets(page, action.maxItems || action.max_items || 100);
      return result(index, type, { assets });
    }
    case "bundle_assets": {
      const bundle = await bundlePageAssets(page, action, index);
      assetBundles.push(bundle.manifestPath);
      return result(index, type, bundle);
    }
    case "eval": {
      const value = await page.evaluate((source) => globalThis.eval(source), requiredString(action, "script"));
      return result(index, type, { value: jsonSafe(value) });
    }
    case "text": {
      const selector = action.selector || "body";
      const text = await page.locator(selector).innerText({ timeout });
      return result(index, type, { selector, text: truncate(text, action.maxChars || 4_000) });
    }
    case "list_tabs": {
      return result(index, type, { tabs: await describeTabs(context, page) });
    }
    case "new_tab": {
      const nextPage = await context.newPage();
      attachPageListeners(nextPage);
      if (typeof action.url === "string" && action.url.trim()) {
        await nextPage.goto(action.url, {
          waitUntil: action.waitUntil || "networkidle",
          timeout: action.navigationTimeoutMs || 30_000,
        });
      }
      await nextPage.bringToFront().catch(() => {});
      return result(index, type, {
        activePage: nextPage,
        tabIndex: context.pages().indexOf(nextPage),
        url: nextPage.url(),
        title: await nextPage.title(),
      });
    }
    case "switch_tab": {
      const nextPage = await resolveTab(context, action);
      await nextPage.bringToFront().catch(() => {});
      return result(index, type, {
        activePage: nextPage,
        tabIndex: context.pages().indexOf(nextPage),
        url: nextPage.url(),
        title: await nextPage.title(),
      });
    }
    case "close_tab": {
      const selectedPage = action.index == null && action.tab_index == null && action.tabIndex == null
        ? page
        : await resolveTab(context, action);
      const pagesBefore = context.pages();
      const closedIndex = pagesBefore.indexOf(selectedPage);
      await selectedPage.close({ runBeforeUnload: Boolean(action.runBeforeUnload ?? action.run_before_unload) });
      let pages = context.pages();
      let nextPage = pages[Math.min(Math.max(closedIndex, 0), pages.length - 1)];
      if (!nextPage) {
        nextPage = await context.newPage();
        attachPageListeners(nextPage);
        pages = context.pages();
      }
      await nextPage.bringToFront().catch(() => {});
      return result(index, type, {
        activePage: nextPage,
        closedIndex,
        activeIndex: pages.indexOf(nextPage),
        tabs: await describeTabs(context, nextPage),
      });
    }
    default:
      throw new Error(`Unsupported browser action at index ${index}: ${type || "(missing type)"}`);
  }
}

function attachPageListeners(page) {
  if (page.__cnCodexListenersAttached) {
    return;
  }
  page.__cnCodexListenersAttached = true;
  page.on("console", (message) => {
    consoleMessages.push({
      type: message.type(),
      text: message.text(),
      url: page.url(),
    });
  });
}

async function describeTabs(context, activePage) {
  const pages = context.pages();
  return await Promise.all(pages.map(async (tab, index) => ({
    index,
    url: tab.url(),
    title: await tab.title().catch(() => ""),
    active: tab === activePage,
    closed: tab.isClosed(),
  })));
}

async function collectPageAssets(page, maxItems) {
  return await page.evaluate((limit) => {
    const absolute = (value) => {
      if (!value) return null;
      try {
        return new URL(value, location.href).href;
      } catch {
        return value;
      }
    };
    const take = (items) => Array.from(items).slice(0, limit);
    return {
      images: take(document.images).map((image) => ({
        src: absolute(image.currentSrc || image.src),
        alt: image.alt || "",
        width: image.naturalWidth || image.width || null,
        height: image.naturalHeight || image.height || null,
      })),
      stylesheets: take(document.querySelectorAll("link[rel~='stylesheet']")).map((link) => ({
        href: absolute(link.getAttribute("href")),
        media: link.getAttribute("media"),
      })),
      scripts: take(document.scripts).map((script) => ({
        src: absolute(script.getAttribute("src")),
        type: script.getAttribute("type"),
      })),
      links: take(document.querySelectorAll("a[href]")).map((link) => ({
        href: absolute(link.getAttribute("href")),
        text: (link.innerText || link.textContent || "").trim().slice(0, 160),
      })),
    };
  }, maxItems);
}

async function bundlePageAssets(page, action, index) {
  const assets = await collectPageAssets(page, action.maxItems || action.max_items || 100);
  const bundleDir = resolveAssetBundleDir(action.path || action.output_path, index);
  await fs.mkdir(bundleDir, { recursive: true });

  const include = normalizeAssetInclude(action.include);
  const maxDownloads = Math.max(0, Math.min(Number(action.maxDownloads ?? action.max_downloads ?? 50), 200));
  const download = action.download !== false;
  const candidates = flattenAssetCandidates(assets).filter((asset) => include.has(asset.kind));
  const files = [];
  const skipped = [];

  for (const asset of candidates.slice(0, maxDownloads)) {
    if (!asset.url) {
      skipped.push({ ...asset, reason: "missing URL" });
      continue;
    }
    if (!download) {
      files.push({ ...asset, path: null, downloaded: false });
      continue;
    }

    try {
      const saved = await saveAsset(asset.url, bundleDir, `${files.length + 1}-${asset.kind}`);
      files.push({ ...asset, ...saved, downloaded: true });
    } catch (error) {
      skipped.push({ ...asset, reason: error?.message || String(error) });
    }
  }

  const manifestPath = path.join(bundleDir, "manifest.json");
  const manifest = {
    url: page.url(),
    title: await page.title(),
    createdAt: new Date().toISOString(),
    include: Array.from(include).sort(),
    download,
    assets,
    files,
    skipped,
  };
  await fs.writeFile(manifestPath, JSON.stringify(manifest, null, 2), "utf8");

  return {
    bundleDir,
    manifestPath,
    assetCount: candidates.length,
    downloadedCount: files.filter((file) => file.downloaded).length,
    skippedCount: skipped.length,
    files,
    skipped,
  };
}

function normalizeAssetInclude(value) {
  const valid = new Set(["images", "stylesheets", "scripts", "links"]);
  if (value === "all") {
    return valid;
  }
  if (Array.isArray(value)) {
    const selected = new Set(value.map(String).filter((item) => valid.has(item)));
    if (selected.size > 0) {
      return selected;
    }
  }
  return new Set(["images", "stylesheets", "scripts"]);
}

function flattenAssetCandidates(assets) {
  return [
    ...(assets.images || []).map((asset) => ({ kind: "images", url: asset.src, metadata: asset })),
    ...(assets.stylesheets || []).map((asset) => ({ kind: "stylesheets", url: asset.href, metadata: asset })),
    ...(assets.scripts || []).map((asset) => ({ kind: "scripts", url: asset.src, metadata: asset })),
    ...(assets.links || []).map((asset) => ({ kind: "links", url: asset.href, metadata: asset })),
  ];
}

async function saveAsset(url, bundleDir, basename) {
  if (url.startsWith("data:")) {
    const parsed = parseDataUrl(url);
    const extension = extensionForMime(parsed.mimeType) || ".bin";
    const filePath = path.join(bundleDir, `${basename}${extension}`);
    await fs.writeFile(filePath, parsed.bytes);
    return { path: filePath, bytes: parsed.bytes.length, mimeType: parsed.mimeType };
  }

  if (!/^https?:\/\//i.test(url)) {
    throw new Error("only http, https, and data URLs can be bundled");
  }

  const response = await fetch(url);
  if (!response.ok) {
    throw new Error(`fetch failed with HTTP ${response.status}`);
  }
  const bytes = Buffer.from(await response.arrayBuffer());
  const contentType = response.headers.get("content-type") || "";
  const extension = extensionForUrl(url) || extensionForMime(contentType) || ".bin";
  const filePath = path.join(bundleDir, `${basename}${extension}`);
  await fs.writeFile(filePath, bytes);
  return { path: filePath, bytes: bytes.length, mimeType: contentType || null };
}

function parseDataUrl(url) {
  const match = /^data:([^;,]*)(;base64)?,(.*)$/s.exec(url);
  if (!match) {
    throw new Error("invalid data URL");
  }
  const mimeType = match[1] || "text/plain";
  const bytes = match[2]
    ? Buffer.from(match[3], "base64")
    : Buffer.from(decodeURIComponent(match[3]), "utf8");
  return { mimeType, bytes };
}

function extensionForUrl(url) {
  try {
    const pathname = new URL(url).pathname;
    const extension = path.extname(pathname);
    return extension && extension.length <= 12 ? extension : null;
  } catch {
    return null;
  }
}

function extensionForMime(mimeType) {
  const normalized = String(mimeType || "").split(";")[0].trim().toLowerCase();
  return {
    "text/css": ".css",
    "text/html": ".html",
    "text/javascript": ".js",
    "application/javascript": ".js",
    "application/json": ".json",
    "image/png": ".png",
    "image/jpeg": ".jpg",
    "image/gif": ".gif",
    "image/svg+xml": ".svg",
    "image/webp": ".webp",
  }[normalized] || null;
}

async function resolveTab(context, action) {
  const pages = context.pages().filter((page) => !page.isClosed());
  if (pages.length === 0) {
    throw new Error("No browser tabs are open");
  }

  const explicitIndex = action?.index ?? action?.tab_index ?? action?.tabIndex;
  if (explicitIndex != null) {
    const index = requiredInteger({ ...action, index: explicitIndex }, "index");
    if (index < 0 || index >= pages.length) {
      throw new Error(`Tab index ${index} is out of range; ${pages.length} tab(s) are open`);
    }
    return pages[index];
  }

  const urlIncludes = stringOrNull(action?.url_contains ?? action?.urlContains);
  if (urlIncludes) {
    const found = pages.find((page) => page.url().includes(urlIncludes));
    if (!found) {
      throw new Error(`No tab URL contains '${urlIncludes}'`);
    }
    return found;
  }

  const titleIncludes = stringOrNull(action?.title_contains ?? action?.titleContains);
  if (titleIncludes) {
    for (const page of pages) {
      const title = await page.title().catch(() => "");
      if (title.includes(titleIncludes)) {
        return page;
      }
    }
    throw new Error(`No tab title contains '${titleIncludes}'`);
  }

  throw new Error(`Action '${action?.type}' requires index, tab_index, url_contains, or title_contains`);
}

async function launchBrowser(options) {
  const headless = options.headless ?? true;
  const channels = [];
  if (options.channel) {
    channels.push(options.channel);
  } else if (process.platform === "win32") {
    channels.push("msedge", "chrome");
  }
  channels.push(null);

  let lastError;
  for (const channel of channels) {
    try {
      const launchOptions = { headless };
      if (channel) {
        launchOptions.channel = channel;
      }
      return await chromium.launch(launchOptions);
    } catch (error) {
      lastError = error;
    }
  }

  throw new Error(
    `Failed to launch Playwright browser: ${lastError?.message || lastError}. ` +
      "Install a browser with `pnpm exec playwright install chromium` if no system browser is available.",
  );
}

async function openBrowserSession(options) {
  if (typeof options.cdpEndpoint === "string" && options.cdpEndpoint.trim()) {
    try {
      return await connectToVisibleBrowser(options);
    } catch (error) {
      launchNotes.push({
        type: "visible-browser-fallback",
        text: `Failed to connect to built-in browser at ${options.cdpEndpoint}: ${error?.message || error}`,
      });
      if (options.requireVisibleBrowser) {
        throw error;
      }
    }
  }

  if (options.visibleBrowserError) {
    launchNotes.push({
      type: "visible-browser-unavailable",
      text: String(options.visibleBrowserError),
    });
  }

  const browser = await launchBrowser(options);
  const contextOptions = {
    acceptDownloads: true,
    viewport: options.viewport || { width: 1280, height: 800 },
  };
  if (options.storageStatePath && await fileExists(options.storageStatePath)) {
    contextOptions.storageState = options.storageStatePath;
  }
  const context = await browser.newContext(contextOptions);
  const page = await context.newPage();
  return { browser, context, page, closeBrowser: true, mode: "standalone" };
}

async function connectToVisibleBrowser(options) {
  const endpoint = options.cdpEndpoint.trim();
  const browser = await retry(
    () => chromium.connectOverCDP(endpoint),
    options.cdpConnectTimeoutMs || 6_000,
  );
  const context = browser.contexts()[0] || await browser.newContext({ acceptDownloads: true });
  const page = context.pages()[0] || await context.newPage();
  if (options.viewport && typeof page.setViewportSize === "function") {
    await page.setViewportSize(options.viewport);
  }
  return { browser, context, page, closeBrowser: false, mode: "visible-cdp" };
}

async function readStdinJson() {
  const chunks = [];
  for await (const chunk of process.stdin) {
    chunks.push(chunk);
  }
  const raw = Buffer.concat(chunks).toString("utf8").trim();
  if (!raw) {
    return {};
  }
  return JSON.parse(raw);
}

function resolveScreenshotPath(rawPath, index) {
  if (typeof rawPath === "string" && rawPath.trim()) {
    return path.resolve(rawPath);
  }
  return path.join(defaultScreenshotDir, `browser-${Date.now()}-${index}.png`);
}

function resolveAssetBundleDir(rawPath, index) {
  if (typeof rawPath === "string" && rawPath.trim()) {
    return path.resolve(rawPath);
  }
  return path.join(defaultAssetDir, `bundle-${Date.now()}-${index}`);
}

function requiredString(action, field) {
  const value = action?.[field];
  if (typeof value !== "string" || !value.trim()) {
    throw new Error(`Action '${action?.type}' requires string field '${field}'`);
  }
  return value;
}

function requiredNumber(action, field) {
  const value = Number(action?.[field]);
  if (!Number.isFinite(value)) {
    throw new Error(`Action '${action?.type}' requires numeric field '${field}'`);
  }
  return value;
}

function requiredInteger(action, field) {
  const value = requiredNumber(action, field);
  if (!Number.isInteger(value)) {
    throw new Error(`Action '${action?.type}' requires integer field '${field}'`);
  }
  return value;
}

function stringOrNull(value) {
  return typeof value === "string" && value.trim() ? value.trim() : null;
}

function result(index, type, details) {
  return { index, type, ...details };
}

function jsonSafe(value) {
  try {
    JSON.stringify(value);
    return value;
  } catch {
    return String(value);
  }
}

function truncate(value, maxChars) {
  const text = String(value ?? "");
  return text.length > maxChars ? `${text.slice(0, maxChars)}\n... [truncated]` : text;
}

async function fileExists(filePath) {
  try {
    await fs.access(filePath);
    return true;
  } catch {
    return false;
  }
}

async function retry(fn, timeoutMs) {
  const deadline = Date.now() + Math.max(1_000, timeoutMs);
  let lastError;
  while (Date.now() < deadline) {
    try {
      return await fn();
    } catch (error) {
      lastError = error;
      await new Promise((resolve) => setTimeout(resolve, 250));
    }
  }
  throw lastError;
}
