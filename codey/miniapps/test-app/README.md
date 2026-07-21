# 测试小程序 — 供应商合同管理

Slug: `test-app`

Database: `测试数据库` (52207d89-cb58-4d6e-bc03-0f8f4aec1ea7)

## 功能说明

对供应商合同信息进行**录入、查询、统计分析**。

### 页面

| 页面 | 路径 | 说明 |
|------|------|------|
| 首页 | `/` | 导航至各功能模块 |
| 供应商录入 | `/entry.html` | 新增供应商合同记录（供应商名、合同号、金额、数量、状态、日期等） |
| 供应商查询 | `/query.html` | 多维度筛选：关键词、供应商名、状态、付款状态、日期区间、金额区间，支持分页 |
| 供应商统计 | `/stats.html` | 汇总卡片、按状态分布、按付款状态、按供应商排名（含金额占比柱状图） |

### MCP Tools

| Tool | 说明 |
|------|------|
| `list_pages` | 列出所有页面 |
| `get_status` | 健康检查，返回端口、数据库绑定、记录数 |
| `open_page` | 打开指定页面 |
| `supplier_insert` | 新增供应商合同记录 |
| `supplier_search` | 按条件查询，支持分页 |
| `supplier_stats` | 统计数据汇总 |

### 数据表

映射数据库表 `sb_entry_form_type_test`，字段包括：
- 供应商名称、合同编号、金额、数量、单价
- 付款状态、合同状态（draft/signed/closed）
- 签署日期、签署时间、备注

## 运行

```bash
../../node/node.exe server/index.mjs
```

端口由环境变量 `MINIAPP_PORT` 指定或自动分配。