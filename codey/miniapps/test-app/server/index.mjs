#!/usr/bin/env node
/**
 * MiniApp: 测试小程序 - 供应商管理
 * - MCP stdio server (tools/list + tools/call)
 * - HTTP static server (web/) + REST API
 * - In-memory data store (demo 模式，数据可从宿主机同步)
 */
import http from "node:http";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const ROOT = path.resolve(__dirname, "..");
const WEB_ROOT = path.join(ROOT, "web");
const MANIFEST_PATH = path.join(ROOT, "miniapp.json");

// ─── Manifest ────────────────────────────────────────────────
function readManifest() {
  try {
    return JSON.parse(fs.readFileSync(MANIFEST_PATH, "utf8"));
  } catch {
    return {
      name: "测试小程序",
      slug: "test-app",
      databaseId: process.env.MINIAPP_DATABASE_ID || "",
      pages: [
        { id: "home", title: "首页", path: "/" },
        { id: "entry", title: "供应商录入", path: "/entry.html" },
        { id: "query", title: "供应商查询", path: "/query.html" },
        { id: "stats", title: "供应商统计", path: "/stats.html" },
      ],
    };
  }
}

const manifest = readManifest();
const preferredPort = Number(process.env.MINIAPP_PORT || process.env.PORT || 0);

// ─── In-memory data store (mirroring sb_entry_form_type_test) ─
const sampleData = [
  { id: 1,  supplier_name: "华为",     contract_no: "HT-2026-0001",  amount: 1037.50,  quantity: 1,  unit_price: 1037.5,  is_paid: false, status: "draft",  signed_date: "2026-01-01",  signed_time: "09:03:01",  signed_at: "2026-01-01T09:03:01",  remark: "测试合同数据#001：与华为签署金额1037.50", created_ts: "2026-07-20T21:42:56" },
  { id: 2,  supplier_name: "腾讯",     contract_no: "HT-2026-0002",  amount: 2150.00,  quantity: 2,  unit_price: 1075,    is_paid: false, status: "signed", signed_date: "2026-01-02",  signed_time: "10:06:02",  signed_at: "2026-01-02T10:06:02",  remark: "测试合同数据#002：与腾讯签署金额2150.00",  created_ts: "2026-07-20T21:42:56" },
  { id: 3,  supplier_name: "阿里巴巴", contract_no: "HT-2026-0003",  amount: 3337.50,  quantity: 3,  unit_price: 1112.5,  is_paid: true,  status: "closed", signed_date: "2026-01-03",  signed_time: "11:09:03",  signed_at: "2026-01-03T11:09:03",  remark: "测试合同数据#003：与阿里巴巴签署金额3337.50", created_ts: "2026-07-20T21:42:56" },
  { id: 4,  supplier_name: "字节跳动", contract_no: "HT-2026-0004",  amount: 4600.00,  quantity: 4,  unit_price: 1150,    is_paid: false, status: "draft",  signed_date: "2026-01-04",  signed_time: "12:12:04",  signed_at: "2026-01-04T12:12:04",  remark: "测试合同数据#004：与字节跳动签署金额4600.00", created_ts: "2026-07-20T21:42:56" },
  { id: 5,  supplier_name: "百度",     contract_no: "HT-2026-0005",  amount: 5937.50,  quantity: 5,  unit_price: 1187.5,  is_paid: false, status: "signed", signed_date: "2026-01-05",  signed_time: "13:15:05",  signed_at: "2026-01-05T13:15:05",  remark: "测试合同数据#005：与百度签署金额5937.50",  created_ts: "2026-07-20T21:42:56" },
  { id: 6,  supplier_name: "京东",     contract_no: "HT-2026-0006",  amount: 7350.00,  quantity: 6,  unit_price: 1225,    is_paid: true,  status: "closed", signed_date: "2026-01-06",  signed_time: "14:18:06",  signed_at: "2026-01-06T14:18:06",  remark: "测试合同数据#006：与京东签署金额7350.00",  created_ts: "2026-07-20T21:42:57" },
  { id: 7,  supplier_name: "美团",     contract_no: "HT-2026-0007",  amount: 8837.50,  quantity: 7,  unit_price: 1262.5,  is_paid: false, status: "draft",  signed_date: "2026-01-07",  signed_time: "15:21:07",  signed_at: "2026-01-07T15:21:07",  remark: "测试合同数据#007：与美团签署金额8837.50",  created_ts: "2026-07-20T21:42:57" },
  { id: 8,  supplier_name: "小米",     contract_no: "HT-2026-0008",  amount: 10400.00, quantity: 8,  unit_price: 1300,    is_paid: false, status: "signed", signed_date: "2026-01-08",  signed_time: "16:24:08",  signed_at: "2026-01-08T16:24:08",  remark: "测试合同数据#008：与小米签署金额10400.00", created_ts: "2026-07-20T21:42:57" },
  { id: 9,  supplier_name: "网易",     contract_no: "HT-2026-0009",  amount: 12037.50, quantity: 9,  unit_price: 1337.5,  is_paid: true,  status: "closed", signed_date: "2026-01-09",  signed_time: "17:27:09",  signed_at: "2026-01-09T17:27:09",  remark: "测试合同数据#009：与网易签署金额12037.50", created_ts: "2026-07-20T21:42:57" },
  { id: 10, supplier_name: "拼多多",   contract_no: "HT-2026-0010",  amount: 13750.00, quantity: 10, unit_price: 1375,    is_paid: false, status: "draft",  signed_date: "2026-01-10",  signed_time: "08:30:10",  signed_at: "2026-01-10T08:30:10",  remark: "测试合同数据#010：与拼多多签署金额13750.00", created_ts: "2026-07-20T21:42:57" },
  { id: 11, supplier_name: "华为",     contract_no: "HT-2026-0011",  amount: 1412.50,  quantity: 1,  unit_price: 1412.5,  is_paid: false, status: "signed", signed_date: "2026-01-11",  signed_time: "09:33:11",  signed_at: "2026-01-11T09:33:11",  remark: "测试合同数据#011：与华为签署金额1412.50",  created_ts: "2026-07-20T21:42:57" },
  { id: 12, supplier_name: "腾讯",     contract_no: "HT-2026-0012",  amount: 2900.00,  quantity: 2,  unit_price: 1450,    is_paid: true,  status: "closed", signed_date: "2026-01-12",  signed_time: "10:36:12",  signed_at: "2026-01-12T10:36:12",  remark: "测试合同数据#012：与腾讯签署金额2900.00",  created_ts: "2026-07-20T21:42:57" },
  { id: 13, supplier_name: "阿里巴巴", contract_no: "HT-2026-0013",  amount: 4462.50,  quantity: 3,  unit_price: 1487.5,  is_paid: false, status: "draft",  signed_date: "2026-01-13",  signed_time: "11:39:13",  signed_at: "2026-01-13T11:39:13",  remark: "测试合同数据#013：与阿里巴巴签署金额4462.50", created_ts: "2026-07-20T21:42:57" },
  { id: 14, supplier_name: "字节跳动", contract_no: "HT-2026-0014",  amount: 6100.00,  quantity: 4,  unit_price: 1525,    is_paid: false, status: "signed", signed_date: "2026-01-14",  signed_time: "12:42:14",  signed_at: "2026-01-14T12:42:14",  remark: "测试合同数据#014：与字节跳动签署金额6100.00", created_ts: "2026-07-20T21:42:57" },
  { id: 15, supplier_name: "百度",     contract_no: "HT-2026-0015",  amount: 7812.50,  quantity: 5,  unit_price: 1562.5,  is_paid: true,  status: "closed", signed_date: "2026-01-15",  signed_time: "13:45:15",  signed_at: "2026-01-15T13:45:15",  remark: "测试合同数据#015：与百度签署金额7812.50",  created_ts: "2026-07-20T21:42:57" },
  { id: 16, supplier_name: "京东",     contract_no: "HT-2026-0016",  amount: 9600.00,  quantity: 6,  unit_price: 1600,    is_paid: false, status: "draft",  signed_date: "2026-01-16",  signed_time: "14:48:16",  signed_at: "2026-01-16T14:48:16",  remark: "测试合同数据#016：与京东签署金额9600.00",  created_ts: "2026-07-20T21:42:57" },
  { id: 17, supplier_name: "美团",     contract_no: "HT-2026-0017",  amount: 11462.50, quantity: 7,  unit_price: 1637.5,  is_paid: false, status: "signed", signed_date: "2026-01-17",  signed_time: "15:51:17",  signed_at: "2026-01-17T15:51:17",  remark: "测试合同数据#017：与美团签署金额11462.50", created_ts: "2026-07-20T21:42:57" },
  { id: 18, supplier_name: "小米",     contract_no: "HT-2026-0018",  amount: 13400.00, quantity: 8,  unit_price: 1675,    is_paid: true,  status: "closed", signed_date: "2026-01-18",  signed_time: "16:54:18",  signed_at: "2026-01-18T16:54:18",  remark: "测试合同数据#018：与小米签署金额13400.00", created_ts: "2026-07-20T21:42:57" },
  { id: 19, supplier_name: "网易",     contract_no: "HT-2026-0019",  amount: 15412.50, quantity: 9,  unit_price: 1712.5,  is_paid: false, status: "draft",  signed_date: "2026-01-19",  signed_time: "17:57:19",  signed_at: "2026-01-19T17:57:19",  remark: "测试合同数据#019：与网易签署金额15412.50", created_ts: "2026-07-20T21:42:57" },
  { id: 20, supplier_name: "拼多多",   contract_no: "HT-2026-0020",  amount: 17500.00, quantity: 10, unit_price: 1750,    is_paid: false, status: "signed", signed_date: "2026-01-20",  signed_time: "08:00:20",  signed_at: "2026-01-20T08:00:20",  remark: "测试合同数据#020：与拼多多签署金额17500.00", created_ts: "2026-07-20T21:42:57" },
];

let suppliers = [...sampleData];
let nextId = 101;

// ─── Business logic ──────────────────────────────────────────

function nowISO() {
  return new Date().toISOString().replace("T", " ").slice(0, 19);
}
function todayDate() {
  return new Date().toISOString().slice(0, 10);
}

function insertSupplier(args) {
  const { supplier_name, contract_no, amount, quantity, unit_price, is_paid, status, signed_date, signed_time, remark } = args;
  if (!supplier_name || supplier_name.trim() === "") {
    return { ok: false, code: "INVALID_PARAM", message: "供应商名称不能为空" };
  }
  if (amount == null || isNaN(Number(amount)) || Number(amount) < 0) {
    return { ok: false, code: "INVALID_PARAM", message: "金额无效" };
  }
  const record = {
    id: nextId++,
    supplier_name: supplier_name.trim(),
    contract_no: contract_no || null,
    amount: Number(amount),
    quantity: quantity != null ? Number(quantity) : 1,
    unit_price: unit_price != null ? Number(unit_price) : null,
    is_paid: is_paid === true || is_paid === "true" || is_paid === 1,
    status: ["draft", "signed", "closed"].includes(status) ? status : "draft",
    signed_date: signed_date || null,
    signed_time: signed_time || null,
    signed_at: signed_date && signed_time ? `${signed_date}T${signed_time}` : null,
    remark: remark || null,
    created_ts: nowISO(),
  };
  suppliers.push(record);
  return { ok: true, code: "OK", message: "供应商记录已添加", data: { record } };
}

function searchSuppliers(args) {
  const { keyword, supplier_name, status, is_paid, date_from, date_to, amount_min, amount_max, page, page_size } = args;
  let filtered = [...suppliers];

  if (keyword && keyword.trim()) {
    const kw = keyword.trim().toLowerCase();
    filtered = filtered.filter(r =>
      r.supplier_name.toLowerCase().includes(kw) ||
      (r.contract_no && r.contract_no.toLowerCase().includes(kw)) ||
      (r.remark && r.remark.toLowerCase().includes(kw))
    );
  }
  if (supplier_name && supplier_name.trim()) {
    filtered = filtered.filter(r => r.supplier_name === supplier_name.trim());
  }
  if (status) {
    filtered = filtered.filter(r => r.status === status);
  }
  if (is_paid != null && is_paid !== "") {
    const paid = is_paid === true || is_paid === "true" || is_paid === 1;
    filtered = filtered.filter(r => r.is_paid === paid);
  }
  if (date_from) {
    filtered = filtered.filter(r => r.signed_date && r.signed_date >= date_from);
  }
  if (date_to) {
    filtered = filtered.filter(r => r.signed_date && r.signed_date <= date_to);
  }
  if (amount_min != null && !isNaN(Number(amount_min))) {
    filtered = filtered.filter(r => r.amount >= Number(amount_min));
  }
  if (amount_max != null && !isNaN(Number(amount_max))) {
    filtered = filtered.filter(r => r.amount <= Number(amount_max));
  }

  // Sort by id descending (newest first)
  filtered.sort((a, b) => b.id - a.id);

  const total = filtered.length;
  const p = Math.max(1, Number(page) || 1);
  const ps = Math.min(100, Math.max(1, Number(page_size) || 20));
  const offset = (p - 1) * ps;
  const rows = filtered.slice(offset, offset + ps);

  return {
    ok: true,
    code: "OK",
    data: {
      rows,
      total,
      page: p,
      page_size: ps,
      total_pages: Math.ceil(total / ps),
    },
  };
}

function getSupplierStats(args) {
  const { supplier_name } = args || {};
  let filtered = [...suppliers];
  if (supplier_name && supplier_name.trim()) {
    filtered = filtered.filter(r => r.supplier_name === supplier_name.trim());
  }

  // 按供应商统计
  const bySupplier = {};
  for (const r of filtered) {
    if (!bySupplier[r.supplier_name]) {
      bySupplier[r.supplier_name] = { supplier_name: r.supplier_name, count: 0, total_amount: 0, total_quantity: 0 };
    }
    bySupplier[r.supplier_name].count++;
    bySupplier[r.supplier_name].total_amount += r.amount;
    bySupplier[r.supplier_name].total_quantity += (r.quantity || 0);
  }
  const supplierStats = Object.values(bySupplier).sort((a, b) => b.total_amount - a.total_amount);

  // 按状态统计
  const byStatus = { draft: 0, signed: 0, closed: 0 };
  for (const r of filtered) {
    byStatus[r.status] = (byStatus[r.status] || 0) + 1;
  }

  // 按付款状态统计
  const paidCount = filtered.filter(r => r.is_paid).length;
  const unpaidCount = filtered.filter(r => !r.is_paid).length;

  // 汇总
  const totalAmount = filtered.reduce((s, r) => s + r.amount, 0);
  const totalQuantity = filtered.reduce((s, r) => s + (r.quantity || 0), 0);
  const avgAmount = filtered.length > 0 ? totalAmount / filtered.length : 0;

  return {
    ok: true,
    code: "OK",
    data: {
      total_records: filtered.length,
      total_amount: Math.round(totalAmount * 100) / 100,
      total_quantity: totalQuantity,
      avg_amount: Math.round(avgAmount * 100) / 100,
      by_supplier: supplierStats,
      by_status: byStatus,
      by_paid: { paid: paidCount, unpaid: unpaidCount },
      suppliers_list: [...new Set(suppliers.map(r => r.supplier_name))].sort(),
    },
  };
}

function listSupplierNames() {
  const names = [...new Set(suppliers.map(r => r.supplier_name))].sort();
  return { ok: true, code: "OK", data: { suppliers: names } };
}

// ─── HTTP server ─────────────────────────────────────────────
function contentType(filePath) {
  const ext = path.extname(filePath).toLowerCase();
  const map = {
    ".html": "text/html; charset=utf-8",
    ".js": "text/javascript; charset=utf-8",
    ".css": "text/css; charset=utf-8",
    ".json": "application/json; charset=utf-8",
    ".png": "image/png",
    ".jpg": "image/jpeg",
    ".jpeg": "image/jpeg",
    ".gif": "image/gif",
    ".svg": "image/svg+xml",
    ".ico": "image/x-icon",
  };
  return map[ext] || "application/octet-stream";
}

function sendJson(res, status, body) {
  const raw = JSON.stringify(body);
  res.writeHead(status, {
    "Content-Type": "application/json; charset=utf-8",
    "Content-Length": Buffer.byteLength(raw),
    "Access-Control-Allow-Origin": "*",
  });
  res.end(raw);
}

function parseBody(req) {
  return new Promise((resolve) => {
    const chunks = [];
    req.on("data", (c) => chunks.push(c));
    req.on("end", () => {
      try {
        resolve(JSON.parse(Buffer.concat(chunks).toString("utf8")));
      } catch {
        resolve({});
      }
    });
  });
}

const server = http.createServer(async (req, res) => {
  // CORS preflight
  if (req.method === "OPTIONS") {
    res.writeHead(204, {
      "Access-Control-Allow-Origin": "*",
      "Access-Control-Allow-Methods": "GET,POST,OPTIONS",
      "Access-Control-Allow-Headers": "Content-Type",
    });
    res.end();
    return;
  }

  const url = new URL(req.url || "/", "http://127.0.0.1");

  // ─── REST API ──────────────────────────────────────────
  if (url.pathname === "/api/status" && req.method === "GET") {
    sendJson(res, 200, {
      ok: true,
      code: "OK",
      data: {
        name: manifest.name,
        slug: manifest.slug,
        databaseId: manifest.databaseId || process.env.MINIAPP_DATABASE_ID || "",
        port: boundPort,
        pages: manifest.pages || [],
        record_count: suppliers.length,
      },
    });
    return;
  }

  if (url.pathname === "/api/supplier/insert" && req.method === "POST") {
    const body = await parseBody(req);
    const result = insertSupplier(body);
    sendJson(res, result.ok ? 200 : 400, result);
    return;
  }

  if (url.pathname === "/api/supplier/search" && req.method === "POST") {
    const body = await parseBody(req);
    const result = searchSuppliers(body);
    sendJson(res, 200, result);
    return;
  }

  if (url.pathname === "/api/supplier/stats" && req.method === "POST") {
    const body = await parseBody(req);
    const result = getSupplierStats(body);
    sendJson(res, 200, result);
    return;
  }

  if (url.pathname === "/api/supplier/names" && req.method === "GET") {
    const result = listSupplierNames();
    sendJson(res, 200, result);
    return;
  }

  // ─── Static files ──────────────────────────────────────
  let rel = decodeURIComponent(url.pathname);
  if (rel === "/") rel = "/index.html";
  const filePath = path.normalize(path.join(WEB_ROOT, rel.replace(/^\/+/, "")));
  if (!filePath.startsWith(WEB_ROOT)) {
    sendJson(res, 403, { ok: false, code: "FORBIDDEN", message: "Forbidden" });
    return;
  }
  fs.readFile(filePath, (err, data) => {
    if (err) {
      res.writeHead(404, { "Content-Type": "text/plain; charset=utf-8" });
      res.end("Not Found");
      return;
    }
    res.writeHead(200, { "Content-Type": contentType(filePath) });
    res.end(data);
  });
});

function listen(port) {
  return new Promise((resolve, reject) => {
    const onError = (err) => reject(err);
    server.once("error", onError);
    server.listen(port, "127.0.0.1", () => {
      server.off("error", onError);
      const address = server.address();
      resolve(typeof address === "object" && address ? address.port : port);
    });
  });
}

const boundPort = await listen(Number.isFinite(preferredPort) ? preferredPort : 0);
process.env.MINIAPP_BOUND_PORT = String(boundPort);
console.error(`[miniapp] listening on 127.0.0.1:${boundPort}`);

// ─── MCP tools definition ────────────────────────────────────
const TOOLS = [
  {
    name: "list_pages",
    description: "列出小程序所有页面",
    inputSchema: { type: "object", properties: {} },
  },
  {
    name: "get_status",
    description: "返回小程序运行状态、端口、数据库绑定信息",
    inputSchema: { type: "object", properties: {} },
  },
  {
    name: "open_page",
    description: "打开指定页面，返回页面 URL 供宿主显示",
    inputSchema: {
      type: "object",
      properties: {
        page: { type: "string", description: "页面 id 或路径（如 home、entry、query、stats）" },
        known: { type: "object", description: "可选预填值" },
      },
    },
  },
  {
    name: "supplier_insert",
    description: "新增一条供应商合同记录",
    inputSchema: {
      type: "object",
      properties: {
        supplier_name: { type: "string", description: "供应商名称（必填）" },
        contract_no: { type: "string", description: "合同编号" },
        amount: { type: "number", description: "合同金额（必填）" },
        quantity: { type: "number", description: "数量，默认1" },
        unit_price: { type: "number", description: "单价" },
        is_paid: { type: "boolean", description: "是否已付款" },
        status: { type: "string", description: "状态：draft/signed/closed，默认draft" },
        signed_date: { type: "string", description: "签署日期，格式 YYYY-MM-DD" },
        signed_time: { type: "string", description: "签署时间，格式 HH:mm:ss" },
        remark: { type: "string", description: "备注" },
      },
      required: ["supplier_name", "amount"],
    },
  },
  {
    name: "supplier_search",
    description: "按条件查询供应商合同记录，支持分页",
    inputSchema: {
      type: "object",
      properties: {
        keyword: { type: "string", description: "通用关键词（匹配供应商名、合同号、备注）" },
        supplier_name: { type: "string", description: "按供应商名称精确筛选" },
        status: { type: "string", description: "按状态筛选：draft/signed/closed" },
        is_paid: { type: "boolean", description: "按付款状态筛选" },
        date_from: { type: "string", description: "签署日期范围起始，YYYY-MM-DD" },
        date_to: { type: "string", description: "签署日期范围截止，YYYY-MM-DD" },
        amount_min: { type: "number", description: "金额范围下限" },
        amount_max: { type: "number", description: "金额范围上限" },
        page: { type: "number", description: "页码，默认1" },
        page_size: { type: "number", description: "每页条数，默认20" },
      },
    },
  },
  {
    name: "supplier_stats",
    description: "获取供应商合同统计信息（按供应商、状态、付款统计）",
    inputSchema: {
      type: "object",
      properties: {
        supplier_name: { type: "string", description: "可选，按供应商筛选统计" },
      },
    },
  },
];

function toolResult(payload) {
  return {
    content: [{ type: "text", text: JSON.stringify(payload, null, 2) }],
    structuredContent: payload,
  };
}

function handleToolCall(name, args = {}) {
  if (name === "list_pages") {
    return toolResult({
      ok: true,
      code: "OK",
      data: { pages: manifest.pages || [] },
    });
  }

  if (name === "get_status") {
    return toolResult({
      ok: true,
      code: "OK",
      data: {
        name: manifest.name,
        slug: manifest.slug,
        port: boundPort,
        databaseId: manifest.databaseId || process.env.MINIAPP_DATABASE_ID || "",
        status: "running",
        record_count: suppliers.length,
      },
    });
  }

  if (name === "open_page") {
    const pageId = typeof args.page === "string" ? args.page : "home";
    const pages = Array.isArray(manifest.pages) ? manifest.pages : [];
    const page =
      pages.find((p) => p.id === pageId || p.path === pageId || p.path === `/${pageId}`) ||
      pages[0] ||
      { id: "home", path: "/" };
    const pathPart = page.path?.startsWith("/") ? page.path : `/${page.path || ""}`;
    return toolResult({
      ok: true,
      code: "OK",
      message: "page ready",
      data: {
        pageId: page.id || pageId,
        url: `http://127.0.0.1:${boundPort}${pathPart || "/"}`,
        known: args.known || {},
      },
      ui: { action: "open_page", pageId: page.id || pageId },
    });
  }

  if (name === "supplier_insert") {
    const result = insertSupplier(args);
    return toolResult({
      ...result,
      ui: result.ok ? { action: "close_page", pageId: "entry" } : undefined,
    });
  }

  if (name === "supplier_search") {
    const result = searchSuppliers(args);
    return toolResult(result);
  }

  if (name === "supplier_stats") {
    const result = getSupplierStats(args);
    return toolResult(result);
  }

  return toolResult({
    ok: false,
    code: "UNKNOWN_TOOL",
    message: `Unknown tool: ${name}`,
  });
}

// ─── MCP stdio JSON-RPC ──────────────────────────────────────
function sendRpc(message) {
  const raw = JSON.stringify(message);
  process.stdout.write(`Content-Length: ${Buffer.byteLength(raw, "utf8")}\r\n\r\n${raw}`);
}

function handleRpc(msg) {
  if (!msg || typeof msg !== "object") return;
  const { id, method, params } = msg;
  if (method === "initialize") {
    sendRpc({
      jsonrpc: "2.0",
      id,
      result: {
        protocolVersion: "2024-11-05",
        capabilities: { tools: {} },
        serverInfo: { name: manifest.slug || "miniapp", version: "0.1.0" },
      },
    });
    return;
  }
  if (method === "notifications/initialized" || method === "initialized") {
    return;
  }
  if (method === "tools/list") {
    sendRpc({ jsonrpc: "2.0", id, result: { tools: TOOLS } });
    return;
  }
  if (method === "tools/call") {
    const name = params?.name;
    const args = params?.arguments || {};
    sendRpc({ jsonrpc: "2.0", id, result: handleToolCall(name, args) });
    return;
  }
  if (method === "ping") {
    sendRpc({ jsonrpc: "2.0", id, result: {} });
    return;
  }
  if (id !== undefined) {
    sendRpc({
      jsonrpc: "2.0",
      id,
      error: { code: -32601, message: `Method not found: ${method}` },
    });
  }
}

// Support both Content-Length framed MCP and newline-delimited JSON.
let buffer = Buffer.alloc(0);
process.stdin.on("data", (chunk) => {
  buffer = Buffer.concat([buffer, chunk]);
  while (true) {
    const headerEnd = buffer.indexOf("\r\n\r\n");
    if (headerEnd >= 0) {
      const header = buffer.slice(0, headerEnd).toString("utf8");
      const match = /Content-Length:\s*(\d+)/i.exec(header);
      if (!match) {
        buffer = buffer.slice(headerEnd + 4);
        continue;
      }
      const len = Number(match[1]);
      const total = headerEnd + 4 + len;
      if (buffer.length < total) break;
      const body = buffer.slice(headerEnd + 4, total).toString("utf8");
      buffer = buffer.slice(total);
      try {
        handleRpc(JSON.parse(body));
      } catch (err) {
        console.error("[miniapp] invalid rpc", err);
      }
      continue;
    }
    // newline delimited fallback
    const nl = buffer.indexOf("\n");
    if (nl < 0) break;
    const line = buffer.slice(0, nl).toString("utf8").trim();
    buffer = buffer.slice(nl + 1);
    if (!line) continue;
    try {
      handleRpc(JSON.parse(line));
    } catch {
      // ignore non-json noise
    }
  }
});

process.stdin.on("end", () => {
  server.close(() => process.exit(0));
});

// Keep process alive even if stdin is not a TTY pipe yet.
setInterval(() => {}, 1 << 30);