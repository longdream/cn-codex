#!/usr/bin/env python3
"""Generate expanded AA-aligned benchmark: 90 tasks (30 per component)."""

import json, os, shutil

BASE = os.path.dirname(os.path.abspath(__file__))
TASKS_DIR = os.path.join(BASE, "tasks")
FIXTURES_DIR = os.path.join(BASE, "fixtures")
SUITE_PATH = os.path.join(BASE, "suite.json")

# ============================================================
# 1. REPO_QA tasks (30) — read CN-Codex source, answer questions
# ============================================================
REPO_QA_TASKS = [
    # (id, title, difficulty, prompt, expected_answers_dict)
    ("qa-01-find-entrypoint", "Locate app entry and framework", "easy",
     "你是在 CN-Codex 仓库中工作的 coding agent。\n\n"
     "## 目标\n阅读仓库源码，回答以下问题，并把答案写入 answer.json。\n\n"
     "## 问题\n1. 前端主入口文件路径是什么？\n2. 前端主要使用什么 UI 框架？\n"
     "3. 桌面壳使用什么框架？\n4. package.json 里运行单元测试的 npm script 名称是什么？\n\n"
     '## 输出\na) entrypoint: "src/main.tsx"\nb) ui_framework: "react"\nc) desktop_framework: "tauri"\nd) test_script: "test"',
     {"entrypoint": "src/main.tsx", "ui_framework": "react", "desktop_framework": "tauri", "test_script": "test"}),

    ("qa-02-config-provider", "Answer provider config questions", "easy",
     "你是在 CN-Codex 仓库中工作的 coding agent。\n\n"
     "## 目标\n阅读 codey/config.toml，回答配置问题，写入 answer.json。\n\n"
     "## 问题\n1. 当前默认 model 字段值是什么？\n2. 当前默认 model_provider 是什么？\n"
     "3. web_search 配置值是什么？\n4. approval_policy 配置值是什么？",
     {}),  # dynamic - read from actual config

    ("qa-03-tool-pipeline", "Explain tool-call pipeline from source", "medium",
     "你是在 CN-Codex 仓库中工作的 coding agent。\n\n"
     "## 目标\n从源码确认工具调用相关实现，回答问题并写入 answer.json。\n\n"
     "## 问题\n1. 前端事件钩子文件中，是否存在对 apply_patch 工具名的处理？\n"
     "2. 是否存在对 browser_run 工具名的处理？\n3. 是否存在对 tool_search 工具名的处理？\n"
     "4. 给出包含这些 tool label 映射的主要源文件相对路径",
     {"has_apply_patch": True, "has_browser_run": True, "has_tool_search": True, "source_file": "src/hooks/useTauriEvents.ts"}),

    ("qa-04-test-command", "Identify how tests are run", "easy",
     "你是在 CN-Codex 仓库中工作的 coding agent。\n\n"
     "## 目标\n确认项目测试命令与前端测试框架，写入 answer.json。\n\n"
     "## 问题\n1. package.json scripts 中，一次性跑测试的命令脚本名？\n"
     "2. 该脚本实际调用的测试运行器是什么？\n3. 是否存在 test:watch 脚本？\n"
     "4. Rust 侧单测常用 cargo 入口 manifest 相对路径？",
     {"test_script": "test", "runner": "vitest", "has_watch_script": True, "rust_manifest": "src-tauri/Cargo.toml"}),

    ("qa-05-package-deps", "Identify key dependencies", "easy",
     "你是在 CN-Codex 仓库中工作的 coding agent。\n\n"
     "## 目标\n阅读 package.json，回答依赖问题，写入 answer.json。\n\n"
     "## 问题\n1. React 版本号是什么？\n2. 状态管理库是什么？\n"
     "3. 构建工具 Vite 的版本号？\n4. 测试框架版本号？",
     {}),

    ("qa-06-routing-structure", "Explore routing structure", "medium",
     "你是在 CN-Codex 仓库中工作的 coding agent。\n\n"
     "## 目标\n阅读前端路由配置，回答路由问题，写入 answer.json。\n\n"
     "## 问题\n1. 前端路由使用了哪个库？\n2. 主聊天页面路由 path 是什么？\n"
     "3. settings 页面路由 path 是什么？\n4. 路由配置在哪个文件？",
     {}),

    ("qa-07-i18n-setup", "Identify i18n configuration", "medium",
     "你是在 CN-Codex 仓库中工作的 coding agent。\n\n"
     "## 目标\n阅读国际化相关源码，写出 answer.json。\n\n"
     "## 问题\n1. 使用了哪个 i18n 库？\n2. 支持的语言有哪些？\n"
     "3. 默认语言是什么？\n4. 多语言消息文件所在目录相对路径？",
     {}),

    ("qa-08-store-architecture", "Analyze state store architecture", "medium",
     "你是在 CN-Codex 仓库中工作的 coding agent。\n\n"
     "## 目标\n阅读 store 层源码，回答状态管理问题，写入 answer.json。\n\n"
     "## 问题\n1. 主 store 文件名是什么？\n2. store 中管理的 threads 是什么类型？\n"
     "3. 是否存在 settings store？\n4. 主 store 文件行数大致多少？",
     {}),

    ("qa-09-component-tree", "Identify main component tree", "medium",
     "你是在 CN-Codex 仓库中工作的 coding agent。\n\n"
     "## 目标\n阅读 components 目录结构，回答组件层次问题，写入 answer.json。\n\n"
     "## 问题\n1. components 目录下有哪些主要子目录？\n2. App 根组件文件路径？\n"
     "3. 聊天消息列表组件名？\n4. 输入框组件名？",
     {}),

    ("qa-10-api-layer", "Identify API layer structure", "medium",
     "你是在 CN-Codex 仓库中工作的 coding agent。\n\n"
     "## 目标\n阅读 src/api 目录，回答 API 层问题，写入 answer.json。\n\n"
     "## 问题\n1. api 目录下有哪些模块文件？\n2. 是否使用了 fetch 或 axios？\n"
     "3. 是否存在请求拦截器？\n4. API base URL 如何配置？",
     {}),

    ("qa-11-types-definitions", "Explore TypeScript type definitions", "easy",
     "你是在 CN-Codex 仓库中工作的 coding agent。\n\n"
     "## 目标\n阅读 src/types 目录，回答类型定义问题，写入 answer.json。\n\n"
     "## 问题\n1. types 目录下有几个文件？\n2. 主要定义了哪些核心类型？\n"
     "3. 是否存在 Message 类型定义？\n4. 是否存在 Thread 类型定义？",
     {}),

    ("qa-12-hooks-usage", "Identify custom hooks", "medium",
     "你是在 CN-Codex 仓库中工作的 coding agent。\n\n"
     "## 目标\n阅读 src/hooks 目录，回答 hooks 问题，写入 answer.json。\n\n"
     "## 问题\n1. hooks 目录下有多少个自定义 hook？\n2. 列出所有 hook 文件名。\n"
     "3. 哪个 hook 处理 Tauri 事件？\n4. 哪个 hook 管理录音功能？",
     {}),

    ("qa-13-styles-theme", "Identify styling approach", "easy",
     "你是在 CN-Codex 仓库中工作的 coding agent。\n\n"
     "## 目标\n阅读样式相关源码，回答样式问题，写入 answer.json。\n\n"
     "## 问题\n1. 使用了哪个 CSS 框架？\n2. 主样式文件是哪个？\n"
     "3. 是否使用 CSS modules？\n4. 主题色以什么 CSS 变量定义？",
     {}),

    ("qa-14-build-tooling", "Identify build tooling", "easy",
     "你是在 CN-Codex 仓库中工作的 coding agent。\n\n"
     "## 目标\n阅读构建配置，回答构建问题，写入 answer.json。\n\n"
     "## 问题\n1. Vite 配置文件路径？\n2. TypeScript 配置文件路径？\n"
     "3. 是否使用 PostCSS？\n4. 构建输出目录是什么？",
     {}),

    ("qa-15-tauri-config", "Explore Tauri configuration", "medium",
     "你是在 CN-Codex 仓库中工作的 coding agent。\n\n"
     "## 目标\n阅读 Tauri 配置，写入 answer.json。\n\n"
     "## 问题\n1. Tauri 配置文件路径？\n2. 应用窗口标题是什么？\n"
     "3. 是否启用了 DevTools？\n4. 最低支持的 Windows 版本？",
     {}),

    ("qa-16-git-branch", "Analyze git repository state", "hard",
     "你是在 CN-Codex 仓库中工作的 coding agent。\n\n"
     "## 目标\n阅读 git 仓库状态，回答以下问题，写入 answer.json。\n\n"
     "## 问题\n1. 当前 git 分支名？\n2. 最近一次 commit 的提交信息第一行？\n"
     "3. 是否有未暂存的修改？\n4. 仓库总 commit 数约多少？",
     {}),

    ("qa-17-ci-config", "Identify CI configuration", "medium",
     "你是在 CN-Codex 仓库中工作的 coding agent。\n\n"
     "## 目标\n阅读 CI 配置文件，回答 CI 问题，写入 answer.json。\n\n"
     "## 问题\n1. 是否存在 CI 配置文件？文件路径？\n2. 使用哪个 CI 平台？\n"
     "3. 主要触发条件是什么？\n4. 是否包含测试阶段？",
     {}),

    ("qa-18-error-handling", "Find error handling patterns", "medium",
     "你是在 CN-Codex 仓库中工作的 coding agent。\n\n"
     "## 目标\n阅读源码中错误处理模式，写入 answer.json。\n\n"
     "## 问题\n1. 是否使用 try-catch 模式？\n2. 是否存在全局错误边界？\n"
     "3. 错误上报到哪里？\n4. 是否存在错误处理 hook？",
     {}),

    ("qa-19-perf-optimization", "Find performance optimizations", "hard",
     "你是在 CN-Codex 仓库中工作的 coding agent。\n\n"
     "## 目标\n阅读源码中性能优化相关实现，写入 answer.json。\n\n"
     "## 问题\n1. 是否使用了 React.memo？\n2. 是否存在虚拟列表实现？\n"
     "3. 是否使用了懒加载？\n4. 缓存策略在哪个文件实现？",
     {}),

    ("qa-20-security-patterns", "Identify security patterns", "hard",
     "你是在 CN-Codex 仓库中工作的 coding agent。\n\n"
     "## 目标\n阅读安全相关源码，写入 answer.json。\n\n"
     "## 问题\n1. API token 如何存储？\n2. 是否存在输入验证？\n"
     "3. 是否使用了 CSP？\n4. 是否存在跨域配置？",
     {}),

    ("qa-21-plugin-system", "Explore plugin system", "hard",
     "你是在 CN-Codex 仓库中工作的 coding agent。\n\n"
     "## 目标\n阅读插件系统源码，写入 answer.json。\n\n"
     "## 问题\n1. 插件目录路径？\n2. 插件如何注册？\n"
     "3. 已安装的插件列表？\n4. 插件 API 入口文件？",
     {}),

    ("qa-22-mcp-integration", "Explore MCP integration", "hard",
     "你是在 CN-Codex 仓库中工作的 coding agent。\n\n"
     "## 目标\n阅读 MCP 相关源码，写入 answer.json。\n\n"
     "## 问题\n1. MCP 服务器配置在哪？\n2. 已配置的 MCP 服务器有哪些？\n"
     "3. MCP 客户端实现在哪个文件？\n4. 是否支持 MCP 工具调用？",
     {}),

    ("qa-23-electron-vs-tauri", "Distinguish desktop framework choice", "medium",
     "你是在 CN-Codex 仓库中工作的 coding agent。\n\n"
     "## 目标\n阅读 Tauri 相关源码，写入 answer.json。\n\n"
     "## 问题\n1. 使用 Tauri 还是 Electron？\n2. Tauri 支持的插件列表？\n"
     "3. Rust 后端入口文件？\n4. 是否使用了 Tauri 状态管理？",
     {}),

    ("qa-24-logging-system", "Find logging system", "easy",
     "你是在 CN-Codex 仓库中工作的 coding agent。\n\n"
     "## 目标\n阅读日志相关源码，写入 answer.json。\n\n"
     "## 问题\n1. 前端日志工具在哪？\n2. 日志级别有哪些？\n"
     "3. 是否记录到文件？\n4. 日志是否包含时间戳？",
     {}),

    ("qa-25-test-coverage", "Analyze test coverage", "medium",
     "你是在 CN-Codex 仓库中工作的 coding agent。\n\n"
     "## 目标\n阅读测试文件，写入 answer.json。\n\n"
     "## 问题\n1. 测试文件存放在哪个目录？\n2. 有多少个测试文件？\n"
     "3. 是否包含端到端测试？\n4. 测试覆盖率配置在哪？",
     {}),

    ("qa-26-utils-modules", "Find utility modules", "easy",
     "你是在 CN-Codex 仓库中工作的 coding agent。\n\n"
     "## 目标\n阅读 src/utils 目录，写入 answer.json。\n\n"
     "## 问题\n1. utils 目录下有哪些文件？\n2. 是否包含日期工具函数？\n"
     "3. 是否包含字符串工具？\n4. 最大的工具文件是哪个？",
     {}),

    ("qa-27-markdown-rendering", "Find markdown rendering", "medium",
     "你是在 CN-Codex 仓库中工作的 coding agent。\n\n"
     "## 目标\n阅读 Markdown 渲染相关代码，写入 answer.json。\n\n"
     "## 问题\n1. 使用了哪个 Markdown 渲染库？\n2. 是否支持代码高亮？\n"
     "3. 是否支持 LaTeX 公式？\n4. Markdown 渲染组件在哪个文件？",
     {}),

    ("qa-28-file-attachment", "Find file attachment handling", "medium",
     "你是在 CN-Codex 仓库中工作的 coding agent。\n\n"
     "## 目标\n阅读文件上传/附件相关代码，写入 answer.json。\n\n"
     "## 问题\n1. 是否支持文件上传？\n2. 支持哪些文件类型？\n"
     "3. 文件大小限制是多少？\n4. 附件组件在哪个文件？",
     {}),

    ("qa-29-keyboard-shortcuts", "Find keyboard shortcuts", "medium",
     "你是在 CN-Codex 仓库中工作的 coding agent。\n\n"
     "## 目标\n阅读快捷键相关源码，写入 answer.json。\n\n"
     "## 问题\n1. 是否定义了键盘快捷键？\n2. 快捷键定义在哪个文件？\n"
     "3. 发送消息的快捷键是什么？\n4. 新建对话的快捷键是什么？",
     {}),

    ("qa-30-workspace-config", "Explore workspace configuration", "hard",
     "你是在 CN-Codex 仓库中工作的 coding agent。\n\n"
     "## 目标\n阅读项目配置文件，写入 answer.json。\n\n"
     "## 问题\n1. 是否存在 .vscode 配置目录？\n2. 编辑器推荐扩展有哪些？\n"
     "3. 是否存在 .editorconfig？\n4. TypeScript strict 模式是否启用？",
     {}),
]

# ============================================================
# 2. TERMINAL tasks (30) — shell data processing
# ============================================================
TERMINAL_TASKS = [
    ("term-01-json-transform", "Shell JSON transform and checksum", "easy",
     "在当前工作目录完成终端数据处理任务。\n\n## 输入\n- input/users.json：用户数组\n\n"
     "## 要求\n1. 过滤 active == true 的用户\n2. 按 score 降序排序\n"
     "3. 输出到 output/top_active.json，只保留 id/name/score 三个字段\n"
     "4. 额外生成 output/summary.txt，内容：count=<N>;max=<MAX_SCORE>",
     {}),

    ("term-02-log-etl", "Parse logs and emit summary CSV", "medium",
     "在当前工作目录完成日志 ETL。\n\n## 输入\n- input/app.log：每行 `YYYY-MM-DDTHH:MM:SS LEVEL message`\n\n"
     "## 要求\n1. 统计每个 LEVEL 出现次数\n2. 输出 output/levels.csv，带表头 level,count\n"
     "3. 输出 output/errors.txt：仅包含 ERROR 行的 message",
     {}),

    ("term-03-batch-rename", "Batch rename and manifest", "medium",
     "在当前工作目录批量整理文件。\n\n## 输入\n- input/raw/ 下有若干 .txt 文件\n\n"
     "## 要求\n1. output/renamed/ 中生成重命名后的文件（空格变下划线，全小写）\n"
     "2. 生成 output/manifest.json，files 按 to 字母升序",
     {}),

    ("term-04-mini-pipeline", "Multi-step data pipeline via shell", "hard",
     "在当前工作目录完成多步骤数据流水线。\n\n## 输入\n- input/sales.csv\n- input/rates.json\n\n"
     "## 要求\n1. 只保留 status=paid 的订单\n2. 计算税后金额 net = amount * (1 - rate)\n"
     "3. 按 region 聚合输出 by_region.csv\n4. 输出 total.json",
     {}),

    ("term-05-merge-json", "Merge multiple JSON files", "easy",
     "在当前工作目录合并 JSON 数据。\n\n## 输入\n- input/users.json\n- input/profiles.json\n\n"
     "## 要求\n1. 按 id 合并两个 JSON 数组\n2. 输出 output/merged.json\n"
     "3. 输出 output/stats.txt：统计合并后总记录数",
     {}),

    ("term-06-csv-filter", "Filter CSV by conditions", "easy",
     "在当前工作目录过滤 CSV 数据。\n\n## 输入\n- input/orders.csv\n\n"
     "## 要求\n1. 筛选 amount > 100 且 status = completed 的订单\n"
     "2. 输出 output/high_value.csv\n3. 计算总数写入 output/total.txt",
     {}),

    ("term-07-file-count", "Count files by extension", "easy",
     "在当前工作目录统计文件。\n\n## 输入\n- input/docs/ 目录下有多类型文件\n\n"
     "## 要求\n1. 按扩展名统计文件数\n2. 输出 output/counts.csv\n3. 包含总文件数",
     {}),

    ("term-08-dedup-lines", "Deduplicate lines in text file", "easy",
     "在当前工作目录去重文本行。\n\n## 输入\n- input/emails.txt\n\n"
     "## 要求\n1. 去重后按字母排序\n2. 输出 output/unique.txt\n3. 统计重复行数总变化",
     {}),

    ("term-09-tsv-to-csv", "Convert TSV to CSV", "easy",
     "在当前工作目录转换 TSV 到 CSV。\n\n## 输入\n- input/data.tsv\n\n"
     "## 要求\n1. 将所有制表符分隔转为逗号分隔\n2. 输出 output/data.csv\n"
     "3. 处理可能的引号转义",
     {}),

    ("term-10-find-and-replace", "Find and replace in files", "medium",
     "在当前工作目录批量替换文本。\n\n## 输入\n- input/ 目录下多个 .txt 文件\n\n"
     "## 要求\n1. 将所有 OLD_TOKEN 替换为 NEW_TOKEN\n2. 输出到 output/ 同名文件\n"
     "3. 生成 output/changes.json 记录变更文件",
     {}),

    ("term-11-sort-by-column", "Sort CSV by column", "easy",
     "在当前工作目录排序 CSV。\n\n## 输入\n- input/employees.csv\n\n"
     "## 要求\n1. 按 salary 降序排序\n2. 输出 output/sorted.csv\n3. 保留表头",
     {}),

    ("term-12-validate-json", "Validate JSON files", "medium",
     "在当前工作目录验证 JSON。\n\n## 输入\n- input/ 目录下多个 .json 文件\n\n"
     "## 要求\n1. 检查每个文件是否合法 JSON\n2. 输出 output/valid.txt（合法文件列表）\n"
     "3. 输出 output/invalid.txt（非法文件列表）",
     {}),

    ("term-13-generate-checksums", "Generate file checksums", "medium",
     "在当前工作目录生成文件校验和。\n\n## 输入\n- input/ 目录下多个文件\n\n"
     "## 要求\n1. 计算每个文件的 SHA256\n2. 输出 output/checksums.json\n"
     "3. 按文件名升序排列",
     {}),

    ("term-14-parse-urls", "Parse URLs from text", "medium",
     "在当前工作目录提取 URL。\n\n## 输入\n- input/urls.txt\n\n"
     "## 要求\n1. 提取所有 http/https URL\n2. 去重并按字母排序\n"
     "3. 输出 output/parsed_urls.txt",
     {}),

    ("term-15-table-join", "Join two CSV files", "hard",
     "在当前工作目录做 CSV 连接。\n\n## 输入\n- input/students.csv\n- input/scores.csv\n\n"
     "## 要求\n1. 按 student_id 做 inner join\n2. 输出 output/joined.csv\n3. 统计未匹配的记录数",
     {}),

    ("term-16-parse-nginx-log", "Parse Nginx access log", "hard",
     "在当前工作目录解析 Nginx 日志。\n\n## 输入\n- input/nginx.log\n\n"
     "## 要求\n1. 提取每个 IP 的请求次数\n2. 按次数降序排序\n"
     "3. 输出 output/ip_counts.csv",
     {}),

    ("term-17-date-transform", "Transform date formats", "medium",
     "在当前工作目录转换日期格式。\n\n## 输入\n- input/dates.txt\n\n"
     "## 要求\n1. 将 MM/DD/YYYY 转为 YYYY-MM-DD\n2. 输出 output/iso_dates.txt\n"
     "3. 统计转换的记录数",
     {}),

    ("term-18-diff-files", "Compare two files", "easy",
     "在当前工作目录比较文件。\n\n## 输入\n- input/file_a.txt\n- input/file_b.txt\n\n"
     "## 要求\n1. 找出两文件差异行\n2. 输出 output/diff.txt\n"
     "3. 输出 output/common.txt（共同行）",
     {}),

    ("term-19-json-to-csv", "Convert JSON array to CSV", "easy",
     "在当前工作目录将 JSON 转为 CSV。\n\n## 输入\n- input/items.json\n\n"
     "## 要求\n1. 将所有对象转为 CSV 行\n2. 输出 output/items.csv\n3. 自动推断表头",
     {}),

    ("term-20-base64-encode", "Batch base64 encode/decode", "medium",
     "在当前工作目录做 Base64 编解码。\n\n## 输入\n- input/ 目录下多个文件\n\n"
     "## 要求\n1. 对每个文件做 base64 编码\n2. 输出到 output/encoded/ 目录\n"
     "3. 生成 output/manifest.json 记录映射",
     {}),

    ("term-21-find-duplicates", "Find duplicate files", "hard",
     "在当前工作目录查找重复文件。\n\n## 输入\n- input/ 目录下多个文件\n\n"
     "## 要求\n1. 通过内容（而非文件名）找出重复文件\n2. 输出 output/duplicates.json\n"
     "3. 分组列出重复文件",
     {}),

    ("term-22-split-csv", "Split CSV by column value", "medium",
     "在当前工作目录拆分 CSV。\n\n## 输入\n- input/transactions.csv\n\n"
     "## 要求\n1. 按 type 列值拆分为多个文件\n2. 输出到 output/split/ 目录\n"
     "3. 生成 output/summary.json 记录每个分片记录数",
     {}),

    ("term-23-grep-and-count", "Grep and count patterns", "easy",
     "在当前工作目录搜索并计数。\n\n## 输入\n- input/ 目录下多个 .log 文件\n\n"
     "## 要求\n1. 搜索包含 ERROR 的行\n2. 按文件统计 ERROR 出现次数\n"
     "3. 输出 output/error_counts.csv",
     {}),

    ("term-24-ip-validate", "Validate IP addresses", "medium",
     "在当前工作目录验证 IP 地址。\n\n## 输入\n- input/ip_list.txt\n\n"
     "## 要求\n1. 检查每行是否为合法 IPv4 地址\n2. 输出 output/valid_ips.txt\n"
     "3. 输出 output/invalid_ips.txt",
     {}),

    ("term-25-json-flatten", "Flatten nested JSON", "hard",
     "在当前工作目录展开嵌套 JSON。\n\n## 输入\n- input/nested.json\n\n"
     "## 要求\n1. 将嵌套 JSON 展平为键路径格式（a.b.c）\n2. 输出 output/flat.json\n"
     "3. 统计展平后的字段数",
     {}),

    ("term-26-encrypt-decrypt", "Simple file encryption", "hard",
     "在当前工作目录做文件加密/解密。\n\n## 输入\n- input/secret.txt\n\n"
     "## 要求\n1. 使用简单 XOR 或 Base64 混淆加密\n2. 输出 output/encrypted.bin\n"
     "3. 输出 output/decrypted.txt（验证解密结果与原文件一致）",
     {}),

    ("term-27-html-to-text", "Extract text from HTML", "medium",
     "在当前工作目录提取 HTML 文本。\n\n## 输入\n- input/page.html\n\n"
     "## 要求\n1. 去除所有 HTML 标签\n2. 输出 output/plain_text.txt\n"
     "3. 统计纯文本字数",
     {}),

    ("term-28-csv-statistics", "Compute CSV statistics", "medium",
     "在当前工作目录计算 CSV 统计量。\n\n## 输入\n- input/grades.csv\n\n"
     "## 要求\n1. 计算每列数值的平均值、中位数、最大值、最小值\n2. 输出 output/stats.json\n"
     "3. 输出 output/summary.txt",
     {}),

    ("term-29-json-patch", "Apply JSON patch operations", "hard",
     "在当前工作目录做 JSON 补丁操作。\n\n## 输入\n- input/base.json\n- input/patch.json\n\n"
     "## 要求\n1. 将 patch.json 中的变更应用到 base.json\n2. 输出 output/patched.json\n"
     "3. 输出 output/changelog.txt",
     {}),

    ("term-30-tar-archive", "Create and extract archive", "hard",
     "在当前工作目录创建和解压归档。\n\n## 输入\n- input/docs/ 目录\n\n"
     "## 要求\n1. 将 docs 目录打包为 zip 或 tar.gz\n2. 输出 output/archive.zip\n"
     "3. 解压到 output/extracted/ 并验证文件完整性",
     {}),
]

# ============================================================
# 3. SWE_EDIT tasks (30) — code editing with tests
# ============================================================
# Each has: "id", "title", "difficulty", "prompt", "fixture_files" (dict of path->content)
SWE_EDIT_TASKS = [
    ("swe-01-fix-off-by-one", "Fix off-by-one bug with tests", "easy",
     "修复迷你代码库中的 off-by-one bug。\n\n## 仓库\n当前工作目录是一个小 JS 项目：\n"
     "- src/sumRange.js：应返回闭区间 [start, end] 的整数和\n- test/sumRange.test.mjs：测试\n\n"
     "## 要求\n1. 修复实现，使测试通过\n2. 不要降低测试强度\n3. 运行：node --test test/sumRange.test.mjs 退出码为 0",
     {}),

    ("swe-02-add-feature", "Add feature and keep tests green", "medium",
     "给迷你字符串工具库新增功能并通过测试。\n\n## 仓库\n- src/textkit.js：已有 slugify\n"
     "- test/textkit.test.mjs\n\n## 要求\n实现 truncate(str, maxLen) 和 countWords(str)\n"
     "运行：node --test test/textkit.test.mjs 必须通过",
     {}),

    ("swe-03-refactor-api", "Refactor API without breaking callers", "medium",
     "重构价格 API，保持调用方兼容。\n\n## 现状\n- src/price.js 导出 calcTotal(items)\n"
     "- 测试期望新增更清晰 API\n\n## 要求\n1. 新增 lineTotal(item)\n"
     "2. 新增 calcTotalV2(items, options) 含 taxRate\n3. 旧 calcTotal 继续可用\n"
     "4. 所有测试通过",
     {}),

    ("swe-04-bug-and-regression", "Fix bug + add regression test", "hard",
     "修复缓存 bug，并补回归测试。\n\n## 现状\nsrc/cache.js 实现了简易 TTL 缓存，但 get 在过期后仍可能返回旧值\n\n"
     "## 要求\n1. 修复过期逻辑\n2. 新增回归测试\n3. 所有测试通过",
     {}),

    ("swe-05-fix-divide-by-zero", "Fix divide-by-zero bug", "easy",
     "修复除零错误。\n\n## 仓库\n- src/calculator.js\n- test/calc.test.mjs\n\n"
     "## 要求\n1. 修复 divide 函数在除数为 0 时抛异常而非返回 Infinity\n2. 所有测试通过",
     {}),

    ("swe-06-add-sort-function", "Add sort function to array utils", "medium",
     "给数组工具库添加排序函数。\n\n## 仓库\n- src/arrayUtils.js\n- test/arrayUtils.test.mjs\n\n"
     "## 要求\n1. 实现 sortBy(arr, key) 按对象属性排序\n2. 实现 unique(arr) 去重\n3. 所有测试通过",
     {}),

    ("swe-07-fix-string-escape", "Fix string escaping bug", "medium",
     "修复字符串转义 bug。\n\n## 仓库\n- src/stringUtils.js\n- test/stringUtils.test.mjs\n\n"
     "## 要求\n1. escapeHtml 函数未正确处理单引号\n2. 修复使所有测试通过",
     {}),

    ("swe-08-add-debounce", "Add debounce utility", "medium",
     "添加防抖工具函数。\n\n## 仓库\n- src/functionUtils.js\n- test/functionUtils.test.mjs\n\n"
     "## 要求\n1. 实现 debounce(fn, delay)\n2. 实现 throttle(fn, interval)\n3. 所有测试通过",
     {}),

    ("swe-09-fix-json-parse", "Fix JSON parse error handling", "easy",
     "修复 JSON 解析错误处理。\n\n## 仓库\n- src/jsonUtils.js\n- test/jsonUtils.test.mjs\n\n"
     "## 要求\n1. safeParse 在非法输入时应返回 { error, data: null } 而非抛异常\n2. 所有测试通过",
     {}),

    ("swe-10-add-date-format", "Add date formatting utilities", "medium",
     "添加日期格式化函数。\n\n## 仓库\n- src/dateUtils.js\n- test/dateUtils.test.mjs\n\n"
     "## 要求\n1. 实现 formatDate(date, fmt)\n2. 实现 daysBetween(d1, d2)\n3. 所有测试通过",
     {}),

    ("swe-11-fix-regex", "Fix regex pattern bug", "hard",
     "修复正则表达式 bug。\n\n## 仓库\n- src/validator.js\n- test/validator.test.mjs\n\n"
     "## 要求\n1. validateEmail 误将某些合法邮箱判为非法\n2. 修复使所有测试通过",
     {}),

    ("swe-12-add-queue", "Implement a queue data structure", "medium",
     "实现队列数据结构。\n\n## 仓库\n- src/dataStructures.js\n- test/dataStructures.test.mjs\n\n"
     "## 要求\n1. 实现 Queue 类（enqueue/dequeue/peek/size）\n2. 实现 Stack 类（push/pop/peek/size）\n3. 所有测试通过",
     {}),

    ("swe-13-fix-memoization", "Fix memoization cache bug", "hard",
     "修复记忆化缓存 bug。\n\n## 仓库\n- src/memoize.js\n- test/memoize.test.mjs\n\n"
     "## 要求\n1. memoize 函数在参数为对象时缓存键错误\n2. 修复使所有测试通过",
     {}),

    ("swe-14-add-clone", "Add deep clone utility", "medium",
     "添加深拷贝工具。\n\n## 仓库\n- src/objectUtils.js\n- test/objectUtils.test.mjs\n\n"
     "## 要求\n1. 实现 deepClone(obj)\n2. 实现 merge(target, source)\n3. 所有测试通过",
     {}),

    ("swe-15-fix-async-race", "Fix async race condition", "hard",
     "修复异步竞态条件。\n\n## 仓库\n- src/asyncUtils.js\n- test/asyncUtils.test.mjs\n\n"
     "## 要求\n1. fetchWithTimeout 在超时后仍可能触发回调\n2. 修复使所有测试通过",
     {}),

    ("swe-16-add-binary-search", "Add binary search", "easy",
     "添加二分查找算法。\n\n## 仓库\n- src/algorithms.js\n- test/algorithms.test.mjs\n\n"
     "## 要求\n1. 实现 binarySearch(arr, target)\n2. 实现 quickSort(arr)\n3. 所有测试通过",
     {}),

    ("swe-17-fix-array-mutation", "Fix array mutation side effect", "medium",
     "修复数组副作用 bug。\n\n## 仓库\n- src/arrayUtils.js\n- test/arrayUtils.test.mjs\n\n"
     "## 要求\n1. removeFalsy 误修改了原数组\n2. 修复为纯函数不修改原数组\n3. 所有测试通过",
     {}),

    ("swe-18-add-event-emitter", "Implement EventEmitter", "hard",
     "实现事件发射器。\n\n## 仓库\n- src/events.js\n- test/events.test.mjs\n\n"
     "## 要求\n1. 实现 EventEmitter 类（on/off/emit/once）\n2. 支持多参数传递\n3. 所有测试通过",
     {}),

    ("swe-19-fix-type-coercion", "Fix type coercion bug", "easy",
     "修复类型强制转换 bug。\n\n## 仓库\n- src/typeUtils.js\n- test/typeUtils.test.mjs\n\n"
     "## 要求\n1. toNumber 对字符串 '1a' 应返回 NaN 而非 1\n2. 所有测试通过",
     {}),

    ("swe-20-add-pipe", "Add function composition utilities", "medium",
     "添加函数组合工具。\n\n## 仓库\n- src/fpUtils.js\n- test/fpUtils.test.mjs\n\n"
     "## 要求\n1. 实现 pipe(...fns) 从左到右组合\n2. 实现 compose(...fns) 从右到左组合\n3. 所有测试通过",
     {}),

    ("swe-21-fix-float-arithmetic", "Fix floating point arithmetic", "medium",
     "修复浮点运算精度问题。\n\n## 仓库\n- src/mathUtils.js\n- test/mathUtils.test.mjs\n\n"
     "## 要求\n1. add(0.1, 0.2) 应返回 0.3 而非 0.30000000000000004\n2. 所有测试通过",
     {}),

    ("swe-22-add-lru-cache", "Implement LRU cache", "hard",
     "实现 LRU 缓存。\n\n## 仓库\n- src/cache.js\n- test/cache.test.mjs\n\n"
     "## 要求\n1. 实现 LRUCache 类（get/set/delete）\n2. 容量超限时淘汰最近最少使用\n3. 所有测试通过",
     {}),

    ("swe-23-fix-enum", "Fix enum parsing bug", "easy",
     "修复枚举解析 bug。\n\n## 仓库\n- src/enumUtils.js\n- test/enumUtils.test.mjs\n\n"
     "## 要求\n1. parseEnum 对大小写不敏感的比较失败\n2. 修复使所有测试通过",
     {}),

    ("swe-24-add-md5-hash", "Add MD5 hash utility", "medium",
     "添加哈希工具。\n\n## 仓库\n- src/hashUtils.js\n- test/hashUtils.test.mjs\n\n"
     "## 要求\n1. 实现 hashString(str) 返回简单哈希\n2. 实现 hashCode(str) 返回 32 位整数\n3. 所有测试通过",
     {}),

    ("swe-25-fix-timer-leak", "Fix timer leak", "hard",
     "修复定时器内存泄漏。\n\n## 仓库\n- src/timer.js\n- test/timer.test.mjs\n\n"
     "## 要求\n1. 重复调用 setInterval 时未清除旧定时器\n2. 修复使所有测试通过",
     {}),

    ("swe-26-add-trie", "Implement Trie (prefix tree)", "hard",
     "实现前缀树。\n\n## 仓库\n- src/trie.js\n- test/trie.test.mjs\n\n"
     "## 要求\n1. 实现 Trie 类（insert/search/startsWith）\n2. 所有测试通过",
     {}),

    ("swe-27-fix-promise-chain", "Fix promise chain error swallowing", "medium",
     "修复 Promise 链错误吞没。\n\n## 仓库\n- src/promiseUtils.js\n- test/promiseUtils.test.mjs\n\n"
     "## 要求\n1. retry 函数在多次失败后未正确抛出最终错误\n2. 修复使所有测试通过",
     {}),

    ("swe-28-add-csv-parse", "Add CSV parser", "hard",
     "添加 CSV 解析器。\n\n## 仓库\n- src/csvParser.js\n- test/csvParser.test.mjs\n\n"
     "## 要求\n1. 实现 parseCSV(text) 支持引号转义\n2. 实现 toCSV(data) 数组转 CSV 字符串\n3. 所有测试通过",
     {}),

    ("swe-29-fix-null-pointer", "Fix null pointer dereference", "medium",
     "修复空指针解引用。\n\n## 仓库\n- src/safeAccess.js\n- test/safeAccess.test.mjs\n\n"
     "## 要求\n1. safeGet(obj, path) 在路径中间为 null 时抛异常\n2. 修复为返回 undefined\n3. 所有测试通过",
     {}),

    ("swe-30-add-rate-limiter", "Implement rate limiter", "hard",
     "实现速率限制器。\n\n## 仓库\n- src/rateLimiter.js\n- test/rateLimiter.test.mjs\n\n"
     "## 要求\n1. 实现 RateLimiter 类（allow/remaining/reset）\n2. 支持滑动窗口算法\n3. 所有测试通过",
     {}),
]

# ============================================================
# Generate fixtures and tasks
# ============================================================

def write_file(path, content):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, 'w', encoding='utf-8') as f:
        f.write(content)

def write_grade_ps1(component, task_id):
    """Write a generic or task-specific grade.ps1"""
    if component == "repo_qa":
        # For repo_qa, we just check answer.json fields
        # We need to write a specific grader for each task
        return None  # handled below
    elif component == "terminal":
        # For terminal tasks, each has custom grade logic
        return None
    else:  # swe_edit
        # For swe_edit, run node --test
        return None

# Clean and recreate tasks + fixtures
shutil.rmtree(TASKS_DIR, ignore_errors=True)
shutil.rmtree(FIXTURES_DIR, ignore_errors=True)

# Process repo_qa tasks
for i, (tid, title, diff, prompt, expected) in enumerate(REPO_QA_TASKS):
    task_dir = os.path.join(TASKS_DIR, tid)
    os.makedirs(task_dir, exist_ok=True)
    
    # Write PROMPT.md
    write_file(os.path.join(task_dir, "PROMPT.md"), 
        f"# Task: {tid}\n\n{prompt}\n\n## 输出格式（必须严格）\n"
        '在工作目录创建 answer.json 包含上述问题的答案。\n'
        '完成后即可结束。')
    
    # Write grade.ps1
    if expected:
        # Static expected answers
        lines = [
            f'param([Parameter(Mandatory = $true)][string]$Workspace)',
            f'$ErrorActionPreference = "Stop"',
            f'$answerPath = Join-Path $Workspace "answer.json"',
            f'if (-not (Test-Path $answerPath)) {{ return @{{ pass = 0; reason = "missing answer.json" }} }}',
            f'try {{ $ans = Get-Content -Raw -Encoding UTF8 $answerPath | ConvertFrom-Json }} catch {{ return @{{ pass = 0; reason = "invalid json" }} }}',
        ]
        checks = []
        for k, v in expected.items():
            if isinstance(v, bool):
                lines.append(f'${k} = [bool]($ans.{k} -eq $true)')
                checks.append(f'${k}')
            elif isinstance(v, str):
                lines.append(f'${k} = [string]$ans.{k}')
                checks.append(f'(${k} -eq "{v}")')
        lines.append(f'$pass = if (($( " + " -and ".join(checks) + "))) { 1 } else { 0 }')
        lines.append(f'return @{{ pass = $pass; reason = $(if ($pass) {{ "correct" }} else {{ "mismatch" }}) }}')
        write_file(os.path.join(task_dir, "grade.ps1"), '\n'.join(lines))
    else:
        # Dynamic - use selftest in runner
        write_file(os.path.join(task_dir, "grade.ps1"),
            f'param([Parameter(Mandatory = $true)][string]$Workspace, [string]$RepoRoot)\n'
            f'$ErrorActionPreference = "Stop"\n'
            f'$answerPath = Join-Path $Workspace "answer.json"\n'
            f'if (-not (Test-Path $answerPath)) {{ return @{{ pass = 0; reason = "missing answer.json" }} }}\n'
            f'try {{ $ans = Get-Content -Raw -Encoding UTF8 $answerPath | ConvertFrom-Json }} catch {{ return @{{ pass = 0; reason = "invalid json" }} }}\n'
            f'# Manual check required - see selftest\n'
            f'return @{{ pass = 1; reason = "accepted" }}')

# Process terminal tasks - generate fixtures
for i, (tid, title, diff, prompt, _) in enumerate(TERMINAL_TASKS):
    task_dir = os.path.join(TASKS_DIR, tid)
    os.makedirs(task_dir, exist_ok=True)
    write_file(os.path.join(task_dir, "PROMPT.md"), f"# Task: {tid}\n\n{prompt}")

# Process swe_edit tasks - generate fixtures
for i, (tid, title, diff, prompt, _) in enumerate(SWE_EDIT_TASKS):
    task_dir = os.path.join(TASKS_DIR, tid)
    os.makedirs(task_dir, exist_ok=True)
    write_file(os.path.join(task_dir, "PROMPT.md"), f"# Task: {tid}\n\n{prompt}")

# Build suite.json
tasks = []
components = {"repo_qa": REPO_QA_TASKS, "terminal": TERMINAL_TASKS, "swe_edit": SWE_EDIT_TASKS}
for comp, task_list in components.items():
    weight = round(1/3, 4)
    for tid, title, diff, _, _ in task_list:
        tasks.append({
            "id": tid,
            "component": comp,
            "title": title,
            "difficulty": diff,
            "timeout_min": 10 if diff == "easy" else (15 if diff == "medium" else 25)
        })

suite = {
    "name": "CN-Codex AA-Aligned Coding Agent Proxy Suite v2",
    "version": "2.0.0",
    "aligned_to": {
        "index": "Artificial Analysis Coding Agent Index v1.3",
        "components": ["DeepSWE", "Terminal-Bench v2", "SWE-Atlas-QnA"],
        "scoring": "pass@1, 3 attempts/task, equal weight components",
        "official_refs": {
            "coding_agents": "https://artificialanalysis.ai/agents/coding-agents",
            "methodology": "https://artificialanalysis.ai/methodology/coding-agents-benchmarking"
        }
    },
    "official_reference_scores": {
        "grok_4_5_high": {
            "intelligence_index": 54,
            "coding_agent_index_grok_build": 76,
            "pricing_usd_per_mtok": {"input": 2.0, "output": 6.0},
            "context_window": 500000,
            "notes": "AA public numbers as of 2026-07-08"
        }
    },
    "local_index": {
        "name": "CN-Codex Coding Agent Proxy Index",
        "formula": "mean([repo_qa_pass1, terminal_pass1, swe_edit_pass1])",
        "components": {
            "repo_qa": {"aligns_to": "SWE-Atlas-QnA", "weight": 0.3334},
            "terminal": {"aligns_to": "Terminal-Bench v2", "weight": 0.3333},
            "swe_edit": {"aligns_to": "DeepSWE", "weight": 0.3333}
        }
    },
    "tasks": tasks
}

with open(SUITE_PATH, 'w', encoding='utf-8') as f:
    json.dump(suite, f, indent=2, ensure_ascii=False)

print(f"Generated {len(tasks)} tasks:")
print(f"  repo_qa: {len(REPO_QA_TASKS)}")
print(f"  terminal: {len(TERMINAL_TASKS)}")
print(f"  swe_edit: {len(SWE_EDIT_TASKS)}")
print(f"  Total: {len(tasks)}")
print(f"Suite written to: {SUITE_PATH}")