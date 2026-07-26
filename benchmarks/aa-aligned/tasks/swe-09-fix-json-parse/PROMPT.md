# Task: swe-09-fix-json-parse

修复 JSON 解析错误处理。

## 仓库
- src/jsonUtils.js
- test/jsonUtils.test.mjs

## 要求
1. safeParse 在非法输入时应返回 { error, data: null } 而非抛异常
2. 所有测试通过