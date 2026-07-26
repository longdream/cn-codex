# Task: swe-15-fix-async-race

修复异步竞态条件。

## 仓库
- src/asyncUtils.js
- test/asyncUtils.test.mjs

## 要求
1. fetchWithTimeout 在超时后仍可能触发回调
2. 修复使所有测试通过