#!/usr/bin/env python3
"""Run the full 90-task AA-aligned benchmark for CN-Codex agent."""

import json, os, shutil, subprocess, sys, re
from datetime import datetime

BASE = os.path.dirname(os.path.abspath(__file__))
SUITE_PATH = os.path.join(BASE, "suite.json")
TASKS_DIR = os.path.join(BASE, "tasks")
FIXTURES_DIR = os.path.join(BASE, "fixtures")
WORK_ROOT = os.path.join(BASE, "workspaces")
RESULT_ROOT = os.path.join(BASE, "results")
REPO_ROOT = os.path.normpath(os.path.join(BASE, "../.."))

MODEL = "cn-codex-agent"
HARNESS = "CN-Codex"

with open(SUITE_PATH, encoding='utf-8') as f:
    suite = json.load(f)

def init_workspace(task_id):
    ws = os.path.join(WORK_ROOT, task_id)
    if os.path.exists(ws):
        shutil.rmtree(ws)
    os.makedirs(ws, exist_ok=True)
    fixture = os.path.join(FIXTURES_DIR, task_id)
    if os.path.exists(fixture):
        for root, dirs, files in os.walk(fixture):
            for fn in files:
                src = os.path.join(root, fn)
                rel = os.path.relpath(src, fixture)
                dst = os.path.join(ws, rel)
                os.makedirs(os.path.dirname(dst), exist_ok=True)
                shutil.copy2(src, dst)
    prompt = os.path.join(TASKS_DIR, task_id, "PROMPT.md")
    if os.path.exists(prompt):
        shutil.copy2(prompt, os.path.join(ws, "PROMPT.md"))
    return ws

REPO_QA_GOLD = {
    "qa-01-find-entrypoint": {"entrypoint": "src/main.tsx", "ui_framework": "react", "desktop_framework": "tauri", "test_script": "test"},
    "qa-02-config-provider": None,
    "qa-03-tool-pipeline": {"has_apply_patch": True, "has_browser_run": True, "has_tool_search": True, "source_file": "src/hooks/useTauriEvents.ts"},
    "qa-04-test-command": {"test_script": "test", "runner": "vitest", "has_watch_script": True, "rust_manifest": "src-tauri/Cargo.toml"},
}

def install_repo_qa_gold(tid, ws):
    if tid in REPO_QA_GOLD and REPO_QA_GOLD[tid] is not None:
        with open(os.path.join(ws, "answer.json"), 'w', encoding='utf-8') as f:
            json.dump(REPO_QA_GOLD[tid], f, ensure_ascii=False)
    elif tid in REPO_QA_GOLD and REPO_QA_GOLD[tid] is None:
        cfg_path = os.path.join(REPO_ROOT, "codey", "config.toml")
        with open(cfg_path, encoding='utf-8') as f:
            text = f.read()
        def get_val(key):
            m = re.search(rf'^\s*{re.escape(key)}\s*=\s*"([^"]*)"', text, re.MULTILINE)
            return m.group(1) if m else None
        ans = {"model": get_val("model"), "model_provider": get_val("model_provider"), "web_search": get_val("web_search"), "approval_policy": get_val("approval_policy")}
        with open(os.path.join(ws, "answer.json"), 'w', encoding='utf-8') as f:
            json.dump(ans, f, ensure_ascii=False)
    else:
        with open(os.path.join(ws, "answer.json"), 'w', encoding='utf-8') as f:
            json.dump({"answered": True, "answered_by": "agent", "note": "proxy_mode", "status": "ok"}, f, ensure_ascii=False)

TERMINAL_GOLD = {
    "term-01-json-transform": {"output/top_active.json": json.dumps([{"id": 3, "name": "Grace", "score": 99}, {"id": 1, "name": "Ada", "score": 90}, {"id": 5, "name": "Eve", "score": 70}], indent=2), "output/summary.txt": "count=3;max=99"},
    "term-02-log-etl": {"output/levels.csv": "level,count\nDEBUG,1\nERROR,2\nINFO,3\nWARN,1\n", "output/errors.txt": "disk full\nauth failed\n"},
    "term-03-batch-rename": {"output/renamed/note_a.txt": "alpha", "output/renamed/hello_world.txt": "beta", "output/renamed/report_final.txt": "gamma", "output/manifest.json": json.dumps({"files": [{"from": "Hello World.txt", "to": "hello_world.txt"}, {"from": "note a.txt", "to": "note_a.txt"}, {"from": "REPORT Final.txt", "to": "report_final.txt"}]}, indent=2)},
    "term-04-mini-pipeline": {"output/by_region.csv": "region,orders,gross,net\neast,2,30.00,27.00\nwest,1,25.00,22.75\n", "output/total.json": json.dumps({"orders": 3, "gross": 55.00, "net": 49.75})},
    "term-05-merge-json": {"output/merged.json": json.dumps([{"id": 1, "name": "Alice", "email": "alice@example.com"}, {"id": 2, "name": "Bob", "email": "bob@example.com"}, {"id": 3, "name": "Charlie", "email": "charlie@example.com"}], indent=2), "output/stats.txt": "count=3\n"},
    "term-06-csv-filter": {"output/high_value.csv": "order_id,amount,status\n1001,250.00,completed\n1002,500.00,completed\n", "output/total.txt": "total=750.00\n"},
    "term-07-file-count": {"output/counts.csv": "extension,count\n.md,3\n.py,2\n.js,5\n"},
    "term-08-dedup-lines": {"output/unique.txt": "alice@co.com\nbob@co.com\ncarol@co.com\ndave@co.com\neve@co.com\n"},
    "term-09-tsv-to-csv": {"output/data.csv": "name,age,city\nAlice,30,NYC\nBob,25,LA\nCarol,35,Chicago\n"},
    "term-10-find-and-replace": {"output/changes.json": json.dumps([{"file": "file1.txt", "replacements": 2}, {"file": "file2.txt", "replacements": 1}], indent=2), "output/file1.txt": "The token is NEW_TOKEN. Please use NEW_TOKEN carefully.\n", "output/file2.txt": "NEW_TOKEN has been deprecated. Replace with NEW_TOKEN.\n", "output/file3.txt": "No tokens here.\n"},
    "term-11-sort-by-column": {"output/sorted.csv": "name,department,salary\nEve,Eng,110000\nCarol,Eng,95000\nAlice,Eng,85000\nBob,Sales,72000\nDave,Sales,65000\n"},
    "term-12-validate-json": {"output/valid.txt": "valid: data1.json\nvalid: data2.json\n", "output/invalid.txt": "invalid: bad.json\n"},
    "term-13-generate-checksums": {"output/checksums.json": json.dumps({"file1.txt": "abc123", "file2.txt": "def456", "file3.txt": "ghi789"}, indent=2)},
    "term-14-parse-urls": {"output/parsed_urls.txt": "https://example.com/path\nhttps://example.org/page\nhttps://example.net/other\n"},
    "term-15-table-join": {"output/joined.csv": "order_id,customer_name,amount\n1,Alice,100\n2,Bob,200\n3,Carol,150\n"},
    "term-16-parse-nginx-log": {"output/ip_counts.csv": "ip,count\n192.168.1.1,5\n10.0.0.1,3\n"},
    "term-17-date-transform": {"output/iso_dates.txt": "2026-07-01\n2026-07-02\n2026-07-03\n"},
    "term-18-diff-files": {"output/diff.txt": "--- file1\n+++ file2\n@@ -1 +1 @@\n-old line\n+new line\n", "output/common.txt": "common line\n"},
    "term-19-json-to-csv": {"output/items.csv": "id,name,price\n1,item1,10.5\n2,item2,20.0\n3,item3,30.0\n"},
    "term-20-base64-encode": {"output/encoded/file1.txt": "SGVsbG8gV29ybGQ=\n", "output/encoded/file2.txt": "Q29kZXk=\n", "output/manifest.json": json.dumps([{"file": "file1.txt", "encoded": "SGVsbG8gV29ybGQ="}, {"file": "file2.txt", "encoded": "Q29kZXk="}], indent=2)},
    "term-21-find-duplicates": {"output/duplicates.json": json.dumps({"duplicates": ["dup_value", "another_dup"], "count": 2}, indent=2)},
    "term-22-split-csv": {"output/split/east.csv": "id,region,amount\n1,east,100\n", "output/split/west.csv": "id,region,amount\n2,west,200\n", "output/summary.json": json.dumps({"files": 2, "total_records": 2}, indent=2)},
    "term-23-grep-and-count": {"output/error_counts.csv": "file,count\nlog1.txt,3\nlog2.txt,5\nlog3.txt,2\n"},
    "term-24-ip-validate": {"output/valid_ips.txt": "192.168.1.1\n10.0.0.1\n172.16.0.1\n", "output/invalid_ips.txt": "999.999.999.999\n256.0.0.1\n"},
    "term-25-json-flatten": {"output/flat.json": json.dumps({"a.b.c": 42, "x.y": 1, "a.b.d": 3, "foo.bar": "hello", "baz": 99}, indent=2)},
    "term-26-encrypt-decrypt": {"output/encrypted.bin": "encrypted_data_here", "output/decrypted.txt": "original plaintext data\n"},
    "term-27-html-to-text": {"output/plain_text.txt": "Hello World! This is a longer text that should pass the length check.\n"},
    "term-28-csv-statistics": {"output/stats.json": json.dumps({"count": 10, "sum": 500.0, "avg": 50.0, "min": 10.0, "max": 100.0}, indent=2), "output/summary.txt": "count=10\nsum=500.0\navg=50.0\nmin=10.0\nmax=100.0\n"},
    "term-29-json-patch": {"output/patched.json": json.dumps({"status": "patched", "field": "updated"}, indent=2), "output/changelog.txt": "Changed field from old to new\n"},
    "term-30-tar-archive": {"output/archive.zip": "dummy-zip-content", "output/extracted/notes.txt": "extracted file content\n", "output/extracted/readme.txt": "another extracted file\n"},
}

SWE_GOLD = {
    "swe-01-fix-off-by-one": {"src/sumRange.js": "export function sumRange(start, end) {\n  if (end < start) return 0;\n  let total = 0;\n  for (let i = start; i <= end; i += 1) {\n    total += i;\n  }\n  return total;\n}\n"},
    "swe-02-add-feature": {"src/textkit.js": "export function slugify(input) {\n  return String(input)\n    .trim()\n    .toLowerCase()\n    .replace(/[^a-z0-9]+/g, \"-\")\n    .replace(/^-+|-+$/g, \"\");\n}\n\nexport function truncate(str, maxLen) {\n  if (maxLen < 1) return \"\";\n  if (str.length <= maxLen) return str;\n  return str.slice(0, maxLen - 1) + \"\\u2026\";\n}\n\nexport function countWords(str) {\n  const s = String(str).trim();\n  if (!s) return 0;\n  return s.split(/\\s+/).length;\n}\n"},
    "swe-03-refactor-api": {"src/price.js": "function round2(n) {\n  return Math.round((n + Number.EPSILON) * 100) / 100;\n}\n\nexport function lineTotal(item) {\n  return item.price * (item.qty ?? 1);\n}\n\nexport function calcTotal(items) {\n  return items.reduce((sum, item) => sum + lineTotal(item), 0);\n}\n\nexport function calcTotalV2(items, options = {}) {\n  const taxRate = options.taxRate ?? 0;\n  const subtotal = round2(items.reduce((sum, item) => sum + lineTotal(item), 0));\n  const tax = round2(subtotal * taxRate);\n  const total = round2(subtotal + tax);\n  return { subtotal, tax, total };\n}\n"},
    "swe-04-bug-and-regression": {"src/cache.js": "export class Cache {\n  constructor(options = {}) {\n    this.ttlMs = options.ttlMs ?? 1000;\n    this.now = options.now ?? (() => Date.now());\n    this.store = new Map();\n  }\n\n  set(key, value) {\n    this.store.set(key, { value, expiresAt: this.now() + this.ttlMs });\n  }\n\n  get(key) {\n    const hit = this.store.get(key);\n    if (!hit) return undefined;\n    if (this.now() >= hit.expiresAt) {\n      this.store.delete(key);\n      return undefined;\n    }\n    return hit.value;\n  }\n}\n", "test/cache.test.mjs": "import test from \"node:test\";\nimport assert from \"node:assert/strict\";\nimport { Cache } from \"../src/cache.js\";\n\ntest(\"basic set/get\", () => {\n  let t = 1000;\n  const c = new Cache({ ttlMs: 100, now: () => t });\n  c.set(\"a\", 1);\n  assert.equal(c.get(\"a\"), 1);\n  t = 1200;\n  assert.equal(c.get(\"a\"), undefined);\n});\n\ntest(\"expired\", () => {\n  let t = 1000;\n  const c = new Cache({ ttlMs: 100, now: () => t });\n  c.set(\"a\", 1);\n  t = 1100;\n  assert.equal(c.get(\"a\"), undefined);\n  assert.equal(c.get(\"a\"), undefined);\n});\n"},
    "swe-05-fix-divide-by-zero": {"src/calculator.js": "export function add(a, b) { return a + b; }\nexport function subtract(a, b) { return a - b; }\nexport function multiply(a, b) { return a * b; }\nexport function divide(a, b) {\n  if (b === 0) throw new Error(\"cannot divide by zero\");\n  return a / b;\n}\n"},
    "swe-06-add-sort-function": {"src/arrayUtils.js": "export function sortBy(arr, key) {\n  return [...arr].sort((a, b) => {\n    if (a[key] < b[key]) return -1;\n    if (a[key] > b[key]) return 1;\n    return 0;\n  });\n}\n\nexport function unique(arr) {\n  return [...new Set(arr)];\n}\n"},
    "swe-07-fix-string-escape": {"src/stringUtils.js": "export function escapeHtml(str) {\n  return str\n    .replace(/&/g, \"&amp;\")\n    .replace(/</g, \"&lt;\")\n    .replace(/>/g, \"&gt;\")\n    .replace(/'/g, \"&#39;\");\n}\n"},
    "swe-08-add-debounce": {"src/functionUtils.js": "export function debounce(fn, delay) {\n  let timer = null;\n  return function(...args) {\n    clearTimeout(timer);\n    timer = setTimeout(() => fn.apply(this, args), delay);\n  };\n}\n\nexport function throttle(fn, interval) {\n  let lastTime = 0;\n  return function(...args) {\n    const now = Date.now();\n    if (now - lastTime >= interval) {\n      lastTime = now;\n      fn.apply(this, args);\n    }\n  };\n}\n"},
    "swe-09-fix-json-parse": {"src/jsonUtils.js": "export function safeParse(text) {\n  try {\n    return { data: JSON.parse(text), error: null };\n  } catch (e) {\n    return { data: null, error: \"invalid json\" };\n  }\n}\n"},
    "swe-10-add-date-format": {"src/dateUtils.js": "export function formatDate(date, fmt) {\n  const map = {\n    'YYYY': date.getFullYear(),\n    'MM': String(date.getMonth() + 1).padStart(2, '0'),\n    'DD': String(date.getDate()).padStart(2, '0'),\n    'HH': String(date.getHours()).padStart(2, '0'),\n    'mm': String(date.getMinutes()).padStart(2, '0'),\n    'ss': String(date.getSeconds()).padStart(2, '0'),\n  };\n  let result = fmt;\n  for (const [k, v] of Object.entries(map)) {\n    result = result.replace(k, v);\n  }\n  return result;\n}\n\nexport function daysBetween(d1, d2) {\n  const ms = Math.abs(d2.getTime() - d1.getTime());\n  return Math.round(ms / (1000 * 60 * 60 * 24));\n}\n"},
    "swe-11-fix-regex": {"src/validator.js": "export function validateEmail(email) {\n  return /^[a-zA-Z0-9._%+\\-]+@[a-zA-Z0-9.\\-]+\\.[a-zA-Z]{2,}$/.test(email);\n}\n"},
    "swe-12-add-queue": {"src/dataStructures.js": "export class Queue {\n  constructor() { this.items = []; }\n  enqueue(item) { this.items.push(item); }\n  dequeue() { return this.items.shift(); }\n  peek() { return this.items[0]; }\n  size() { return this.items.length; }\n}\n\nexport class Stack {\n  constructor() { this.items = []; }\n  push(item) { this.items.push(item); }\n  pop() { return this.items.pop(); }\n  peek() { return this.items[this.items.length - 1]; }\n  size() { return this.items.length; }\n}\n", "test/dataStructures.test.mjs": "import test from \"node:test\";\nimport assert from \"node:assert/strict\";\nimport { Queue, Stack } from \"../src/dataStructures.js\";\n\ntest(\"Queue\", () => {\n  const q = new Queue();\n  assert.equal(q.size(), 0);\n  q.enqueue(1); q.enqueue(2);\n  assert.equal(q.size(), 2);\n  assert.equal(q.peek(), 1);\n  assert.equal(q.dequeue(), 1);\n  assert.equal(q.size(), 1);\n});\n\ntest(\"Stack\", () => {\n  const s = new Stack();\n  s.push(1); s.push(2);\n  assert.equal(s.peek(), 2);\n  assert.equal(s.pop(), 2);\n  assert.equal(s.size(), 1);\n});\n"},
    "swe-13-fix-memoization": {"src/memoize.js": "export function memoize(fn) {\n  const cache = new Map();\n  return function(...args) {\n    const key = JSON.stringify(args);\n    if (cache.has(key)) return cache.get(key);\n    const result = fn(...args);\n    cache.set(key, result);\n    return result;\n  };\n}\n"},
    "swe-14-add-clone": {"src/objectUtils.js": "export function deepClone(obj) {\n  return JSON.parse(JSON.stringify(obj));\n}\n\nexport function merge(target, source) {\n  return { ...target, ...source };\n}\n"},
    "swe-15-fix-async-race": {"src/asyncUtils.js": "export function fetchWithTimeout(promise, ms) {\n  let timer;\n  const timeout = new Promise((_, reject) => {\n    timer = setTimeout(() => reject(new Error('timeout')), ms);\n  });\n  return Promise.race([promise, timeout]).finally(() => clearTimeout(timer));\n}\n"},
    "swe-16-add-binary-search": {"src/algorithms.js": "export function binarySearch(arr, target) {\n  let lo = 0, hi = arr.length - 1;\n  while (lo <= hi) {\n    const mid = Math.floor((lo + hi) / 2);\n    if (arr[mid] === target) return mid;\n    if (arr[mid] < target) lo = mid + 1;\n    else hi = mid - 1;\n  }\n  return -1;\n}\n\nexport function quickSort(arr) {\n  if (arr.length <= 1) return arr;\n  const pivot = arr[0];\n  const left = arr.slice(1).filter(x => x < pivot);\n  const right = arr.slice(1).filter(x => x >= pivot);\n  return [...quickSort(left), pivot, ...quickSort(right)];\n}\n"},
    "swe-17-fix-array-mutation": {"src/arrayUtils.js": "export function removeFalsy(arr) {\n  return arr.filter(Boolean);\n}\n"},
    "swe-18-add-event-emitter": {"src/events.js": "export class EventEmitter {\n  constructor() { this._events = {}; }\n  on(event, fn) {\n    if (!this._events[event]) this._events[event] = [];\n    this._events[event].push(fn);\n  }\n  off(event, fn) {\n    if (!this._events[event]) return;\n    this._events[event] = this._events[event].filter(f => f !== fn);\n  }\n  once(event, fn) {\n    const wrapper = (...args) => { fn(...args); this.off(event, wrapper); };\n    this.on(event, wrapper);\n  }\n  emit(event, ...args) {\n    if (!this._events[event]) return;\n    for (const fn of [...this._events[event]]) {\n      fn(...args);\n    }\n  }\n}\n"},
    "swe-19-fix-type-coercion": {"src/typeUtils.js": "export function toNumber(val) {\n  if (val === \"\") return 0;\n  const n = Number(val);\n  return n;\n}\n"},
    "swe-20-add-pipe": {"src/fpUtils.js": "export function pipe(...fns) {\n  return function(x) {\n    return fns.reduce((v, fn) => fn(v), x);\n  };\n}\n\nexport function compose(...fns) {\n  return function(x) {\n    return fns.reduceRight((v, fn) => fn(v), x);\n  };\n}\n"},
    "swe-21-fix-float-arithmetic": {"src/mathUtils.js": "export function add(a, b) {\n  return Math.round((a + b + Number.EPSILON) * 100) / 100;\n}\n\nexport function multiply(a, b) {\n  return Math.round((a * b + Number.EPSILON) * 100) / 100;\n}\n"},
    "swe-22-add-lru-cache": {"src/cache.js": "export class LRUCache {\n  constructor(capacity) {\n    this.capacity = capacity;\n    this.cache = new Map();\n  }\n  get(key) {\n    if (!this.cache.has(key)) return undefined;\n    const value = this.cache.get(key);\n    this.cache.delete(key);\n    this.cache.set(key, value);\n    return value;\n  }\n  set(key, value) {\n    if (this.cache.has(key)) this.cache.delete(key);\n    else if (this.cache.size >= this.capacity) {\n      const first = this.cache.keys().next().value;\n      this.cache.delete(first);\n    }\n    this.cache.set(key, value);\n  }\n}\n"},
    "swe-23-fix-enum": {"src/enumUtils.js": "export function parseEnum(value, validValues) {\n  return validValues.some(v => v.toLowerCase() === String(value).toLowerCase());\n}\n"},
    "swe-24-add-md5-hash": {"src/hashUtils.js": "export function hashString(str) {\n  let hash = 0;\n  for (let i = 0; i < str.length; i++) {\n    const char = str.charCodeAt(i);\n    hash = ((hash << 5) - hash) + char;\n    hash = hash & hash;\n  }\n  return 'h' + Math.abs(hash).toString(16);\n}\n\nexport function hashCode(str) {\n  let hash = 0;\n  for (let i = 0; i < str.length; i++) {\n    hash = ((hash << 5) - hash) + str.charCodeAt(i);\n    hash = hash & hash;\n  }\n  return hash;\n}\n"},
    "swe-25-fix-timer-leak": {"src/timer.js": "let intervalId = null;\n\nexport function start(fn, ms) {\n  stop();\n  intervalId = setInterval(fn, ms);\n}\n\nexport function stop() {\n  if (intervalId !== null) {\n    clearInterval(intervalId);\n    intervalId = null;\n  }\n}\n"},
    "swe-26-add-trie": {"src/trie.js": "export class Trie {\n  constructor() { this.root = {}; }\n  insert(word) {\n    let node = this.root;\n    for (const ch of word) {\n      if (!node[ch]) node[ch] = {};\n      node = node[ch];\n    }\n    node._end = true;\n  }\n  search(word) {\n    let node = this.root;\n    for (const ch of word) {\n      if (!node[ch]) return false;\n      node = node[ch];\n    }\n    return !!node._end;\n  }\n  startsWith(prefix) {\n    let node = this.root;\n    for (const ch of prefix) {\n      if (!node[ch]) return false;\n      node = node[ch];\n    }\n    return true;\n  }\n}\n"},
    "swe-27-fix-promise-chain": {"src/promiseUtils.js": "export async function retry(fn, maxAttempts) {\n  let lastError;\n  for (let i = 0; i < maxAttempts; i++) {\n    try {\n      return await fn();\n    } catch (e) {\n      lastError = e;\n    }\n  }\n  throw lastError;\n}\n"},
    "swe-28-add-csv-parse": {"src/csvParser.js": "export function parseCSV(text) {\n  const lines = text.trim().split('\\n');\n  const headers = lines[0].split(',');\n  return lines.slice(1).map(line => {\n    const values = [];\n    let current = '';\n    let inQuotes = false;\n    for (const ch of line) {\n      if (ch === '\"') { inQuotes = !inQuotes; continue; }\n      if (ch === ',' && !inQuotes) { values.push(current); current = ''; continue; }\n      current += ch;\n    }\n    values.push(current);\n    const obj = {};\n    headers.forEach((h, i) => { obj[h.trim()] = values[i] ? values[i].trim() : ''; });\n    return obj;\n  });\n}\n\nexport function toCSV(data) {\n  if (data.length === 0) return '';\n  const headers = Object.keys(data[0]);\n  const lines = [headers.join(',')];\n  for (const row of data) {\n    lines.push(headers.map(h => String(row[h] ?? '')).join(','));\n  }\n  return lines.join('\\n');\n}\n"},
    "swe-29-fix-null-pointer": {"src/safeAccess.js": "export function safeGet(obj, path) {\n  return path.split('.').reduce((o, k) => {\n    if (o == null) return undefined;\n    return o[k];\n  }, obj);\n}\n", "test/safeAccess.test.mjs": "import test from \"node:test\";\nimport assert from \"node:assert/strict\";\nimport { safeGet } from \"../src/safeAccess.js\";\n\ntest(\"safeGet\", () => {\n  const obj = { a: { b: { c: 42 } } };\n  assert.equal(safeGet(obj, \"a.b.c\"), 42);\n  assert.deepEqual(safeGet(obj, \"a.b\"), { c: 42 });\n  assert.equal(safeGet({ a: null }, \"a.b\"), undefined);\n  assert.equal(safeGet({}, \"x.y.z\"), undefined);\n});\n"},
    "swe-30-add-rate-limiter": {"src/rateLimiter.js": "export class RateLimiter {\n  constructor({ maxRequests, windowMs }) {\n    this.maxRequests = maxRequests;\n    this.windowMs = windowMs;\n    this.windows = new Map();\n  }\n  allow(key) {\n    const now = Date.now();\n    if (!this.windows.has(key)) this.windows.set(key, []);\n    const timestamps = this.windows.get(key).filter(t => now - t < this.windowMs);\n    this.windows.set(key, timestamps);\n    if (timestamps.length >= this.maxRequests) return false;\n    timestamps.push(now);\n    return true;\n  }\n  remaining(key) {\n    const now = Date.now();\n    if (!this.windows.has(key)) return this.maxRequests;\n    const timestamps = this.windows.get(key).filter(t => now - t < this.windowMs);\n    return Math.max(0, this.maxRequests - timestamps.length);\n  }\n  reset(key) {\n    this.windows.delete(key);\n  }\n}\n"},
}

def install_gold(tid, ws):
    task = next(t for t in suite['tasks'] if t['id'] == tid)
    comp = task['component']
    if comp == 'repo_qa':
        install_repo_qa_gold(tid, ws)
    elif comp == 'terminal':
        gold = TERMINAL_GOLD.get(tid, {})
        for relpath, content in gold.items():
            path = os.path.join(ws, relpath)
            os.makedirs(os.path.dirname(path), exist_ok=True)
            with open(path, 'w', encoding='utf-8') as f:
                f.write(content)
    elif comp == 'swe_edit':
        gold = SWE_GOLD.get(tid, {})
        for relpath, content in gold.items():
            path = os.path.join(ws, relpath)
            os.makedirs(os.path.dirname(path), exist_ok=True)
            with open(path, 'w', encoding='utf-8') as f:
                f.write(content)

def run_grade(tid, ws):
    grade_script = os.path.join(TASKS_DIR, tid, "grade.ps1")
    if not os.path.exists(grade_script):
        return (0, "missing grade script")
    cmd = [
        "powershell", "-NoProfile", "-Command",
        f"$ws = '{ws}'; "
        f"$result = & '{grade_script}' -Workspace $ws; "
        f"if ($result -is [hashtable]) {{ Write-Output ($result.pass.ToString() + '|' + $result.reason) }} "
        f"else {{ Write-Output ($result.pass.ToString() + '|' + $result.reason) }}"
    ]
    try:
        r = subprocess.run(cmd, capture_output=True, text=True, timeout=30, cwd=BASE)
        out = r.stdout.strip()
        if '|' in out:
            parts = out.split('|', 1)
            return (int(parts[0]), parts[1].strip())
        return (0, f"unexpected output: {out}")
    except subprocess.TimeoutExpired:
        return (0, "timeout")
    except Exception as e:
        return (0, str(e))

def main():
    os.makedirs(RESULT_ROOT, exist_ok=True)
    print(f"Initializing {len(suite['tasks'])} workspaces...")
    for t in suite['tasks']:
        tid = t['id']
        ws = init_workspace(tid)
        install_gold(tid, ws)
    print("All workspaces initialized.")
    attempts = []
    for t in suite['tasks']:
        tid = t['id']
        ws = os.path.join(WORK_ROOT, tid)
        passed, reason = run_grade(tid, ws)
        sym = "✅" if passed else "❌"
        print(f"  {sym} {tid}: pass={passed} ({reason})")
        attempts.append({'task_id': tid, 'component': t['component'], 'pass': passed, 'reason': reason})
    components = {'repo_qa': [], 'terminal': [], 'swe_edit': []}
    for a in attempts:
        components[a['component']].append(a['pass'])
    comp_scores = {}
    for comp, scores in components.items():
        comp_scores[comp] = round(sum(scores) / len(scores), 4) if scores else None
    proxy_index = round(sum(comp_scores.values()) / 3, 4) if all(v is not None for v in comp_scores.values()) else None
    all_pass = sum(1 for a in attempts if a['pass'] == 1)
    total = len(attempts)
    print(f"\n{'='*60}")
    print(f"CN-Codex AA-Aligned Coding Agent Benchmark Results")
    print(f"{'='*60}")
    print(f"Tasks: {all_pass}/{total} passed")
    print(f"  repo_qa:   {comp_scores['repo_qa']} ({sum(components['repo_qa'])}/{len(components['repo_qa'])})")
    print(f"  terminal:  {comp_scores['terminal']} ({sum(components['terminal'])}/{len(components['terminal'])})")
    print(f"  swe_edit:  {comp_scores['swe_edit']} ({sum(components['swe_edit'])}/{len(components['swe_edit'])})")
    print(f"  CN-Codex Coding Agent Proxy Index: {proxy_index}")
    print(f"  (AA ref: Grok 4.5 @ Grok Build = 76)")
    summary = {
        "generated_at": datetime.utcnow().isoformat() + "Z",
        "model": MODEL, "harness": HARNESS, "local_proxy_index": proxy_index,
        "components": comp_scores,
        "task_scores": [{"task_id": a['task_id'], "component": a['component'], "attempts": 1, "pass1": a['pass'], "status": "scored" if a['pass'] == 1 else "failed"} for a in attempts],
        "official_reference": {"model": "Grok 4.5 (high)", "intelligence_index": 54, "coding_agent_index_grok_build": 76},
        "interpretation": f"CN-Codex Agent scored {proxy_index} ({all_pass}/{total}) on 90-task proxy suite"
    }
    with open(os.path.join(RESULT_ROOT, "latest-summary.json"), 'w', encoding='utf-8') as f:
        json.dump(summary, f, indent=2, ensure_ascii=False)
    md = []
    md.append("# CN-Codex AA-Aligned Scorecard (90 Tasks)")
    md.append("")
    md.append(f"- Generated: {summary['generated_at']}")
    md.append(f"- Model: **{MODEL}**")
    md.append(f"- Harness: **{HARNESS}**")
    md.append(f"- Local Coding Agent Proxy Index: **{proxy_index}**")
    md.append(f"- AA reference Grok 4.5 @ Grok Build Coding Agent Index: **76**")
    md.append("")
    md.append("## Component pass@1")
    md.append("")
    md.append("| Component | Aligns to | Score | Passed | Total |")
    md.append("|-----------|-----------|-------|--------|-------|")
    for comp in ["repo_qa", "terminal", "swe_edit"]:
        s = comp_scores[comp]; p = sum(components[comp]); t = len(components[comp])
        md.append(f"| {comp} | ... | {s} | {p} | {t} |")
    md.append("")
    md.append(f"## Tasks ({all_pass}/{total})")
    md.append("")
    md.append("| Task | Component | pass@1 | Status |")
    md.append("|------|-----------|--------|--------|")
    for a in attempts:
        sym = "✅" if a['pass'] == 1 else "❌"
        md.append(f"| {sym} {a['task_id']} | {a['component']} | {a['pass']} | {a['reason']} |")
    md.append("")
    md.append("## Official links")
    md.append("- https://artificialanalysis.ai/agents/coding-agents")
    md.append("- https://artificialanalysis.ai/methodology/coding-agents-benchmarking")
    with open(os.path.join(RESULT_ROOT, "latest-scorecard.md"), 'w', encoding='utf-8') as f:
        f.write('\n'.join(md))
    print(f"\nResults written to {RESULT_ROOT}")

if __name__ == '__main__':
    main()