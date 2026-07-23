---
name: 在 Z-Library 搜索小说并获取适合手机阅读的版本
description: 在 Z-Library 上搜索指定小说，提取适合手机阅读的 EPUB/MOBI 版本链接
tags: ["在 Z-Library 下载小说", "搜索小说 手机阅读", "Z-Library 找书", "下载电子书 手机版"]
---

# 在 Z-Library 搜索小说并获取适合手机阅读的版本

在 Z-Library 上搜索指定小说，提取适合手机阅读的 EPUB/MOBI 版本链接

这是一个结构化 Workflow，请严格按以下节点顺序执行。每完成一个节点后确认输出符合预期再继续下一个节点。如果任何节点失败，停止并报告问题。

## 变量

- `{{novelName}}`: 要搜索的小说名称 (默认: 剑来)
- `{{zLibraryUrl}}`: Z-Library 当前可访问的域名 (默认: https://z-library.sk)
- `{{searchTimeout}}`: 搜索操作超时时间（毫秒） (默认: 90000)

## 执行节点

### Node 1: 打开 Z-Library 网站并验证可访问性

- **目标**: 打开 Z-Library 网站并验证可访问性
- **工具**: browser_run
- **browser_run.actions**: `[{"action":"goto","url":"{{zLibraryUrl}}"},{"action":"wait_for_timeout","timeout":5000},{"action":"eval","script":"document.title"}]`
- **期望输出**: 浏览器成功加载 Z-Library 首页，title 包含 'Z-Library'
- **Token 预算**: 3000

#### ⚠️ 已知陷阱

**陷阱 1**: web_fetch 请求失败：error following redirect for url (https://z-library.sk/)
- 原因: Z-Library 域名可能被屏蔽或需要更强大的浏览器环境
- 正确做法: 改用 browser_run 工具，它能处理更复杂的重定向和 JavaScript 渲染


### Node 2: 在 Z-Library 搜索指定小说

- **目标**: 在 Z-Library 搜索指定小说
- **工具**: browser_run
- **browser_run.actions**: `[{"action":"eval","script":"let input = document.querySelector('input[type=\"search\"], input[name=\"q\"], input[placeholder*=\"search\" i], input[placeholder*=\"Search\" i]'); if(input) { input.value = '{{novelName}}'; document.querySelector('form').dispatchEvent(new Event('submit', {cancelable: true, bubbles: true})); } else { throw new Error('未找到搜索输入框'); }"},{"action":"wait_for_timeout","timeout":5000},{"action":"eval","script":"document.title"}]`
- **browser_run.timeout_ms**: `{{searchTimeout}}`
- **期望输出**: 页面跳转到搜索结果显示页，title 包含 '{{novelName}}: search on Z-Library'
- **Token 预算**: 5000

#### ⚠️ 已知陷阱

**陷阱 1**: 浏览器操作超时 (WEBVIEW_RUN_TIMEOUT) 60秒
- 原因: 搜索提交后页面加载过慢或表单提交未触发正确跳转
- 正确做法: 增加 searchTimeout 至 90 秒，确保等待足够；改用 eval 直接提交表单，而非使用 fill + press 组合

**陷阱 2**: 误搜索到 '剑条'（URL 编码错误）
- 原因: URL 手动构造时字符编码错误，导致搜索词不同
- 正确做法: 始终使用浏览器内的表单提交方式，避免手动拼接 URL

**陷阱 3**: 搜索后页面未改变（仍停留在首页）
- 原因: 输入框选择器不匹配或表单提交事件未触发
- 正确做法: 使用 eval 脚本动态查找所有可能的搜索输入框，并用 dispatchEvent 触发 submit 事件


### Node 3: 从搜索结果中提取适合手机阅读的书籍版本（EPUB/MOBI）链接

- **目标**: 从搜索结果中提取适合手机阅读的书籍版本（EPUB/MOBI）链接
- **工具**: browser_run
- **browser_run.actions**: `[{"action":"eval","script":"let links = []; document.querySelectorAll('a').forEach(a => { let text = a.innerText.trim(); if (text.includes('{{novelName}}') && (text.toLowerCase().includes('epub') || text.toLowerCase().includes('mobi') || text.toLowerCase().includes('多看'))) { links.push({ text: text, href: a.href }); } }); JSON.stringify(links);"}]`
- **期望输出**: 返回包含书籍名称和链接的 JSON 数组，至少包含一个适合手机阅读的版本
- **Token 预算**: 4000

#### ⚠️ 已知陷阱

**陷阱 1**: 提取链接时浏览器超时（WEBVIEW_RUN_TIMEOUT）
- 原因: eval 脚本遍历所有链接时性能开销大，或页面元素过多
- 正确做法: 缩小选择范围，只遍历包含 'book' 或 'card' 等 class 的容器内的链接；或分批次提取

**陷阱 2**: 提取到的结果为空
- 原因: 搜索结果页面结构变化，未找到包含小说名称的链接
- 正确做法: 先截图查看页面布局，或使用更通用的选择器，如 document.querySelectorAll('.resItem a, .bookRow a, .book-item a')


## 执行规则

- 按节点顺序逐个执行，不要跳过
- 每个节点只使用该节点列出的工具
- 将变量 `{{...}}` 替换为用户提供的实际值（或使用默认值）
- 如果 `scripts/` 下已有对应脚本，优先直接复用，避免重复编写同类脚本
- 保持输出简洁，节约 token
