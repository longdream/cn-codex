# Task: qa-01-find-entrypoint

你是在 CN-Codex 仓库中工作的 coding agent。

## 目标
阅读仓库源码，回答以下问题，并把答案写入 answer.json。

## 问题
1. 前端主入口文件路径是什么？
2. 前端主要使用什么 UI 框架？
3. 桌面壳使用什么框架？
4. package.json 里运行单元测试的 npm script 名称是什么？

## 输出
a) entrypoint: "src/main.tsx"
b) ui_framework: "react"
c) desktop_framework: "tauri"
d) test_script: "test"

## 输出格式（必须严格）
在工作目录创建 answer.json 包含上述问题的答案。
完成后即可结束。