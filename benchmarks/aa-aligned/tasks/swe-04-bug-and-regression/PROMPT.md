# Task: swe-04-bug-and-regression

修复缓存 bug，并补回归测试。

## 现状
src/cache.js 实现了简易 TTL 缓存，但 get 在过期后仍可能返回旧值

## 要求
1. 修复过期逻辑
2. 新增回归测试
3. 所有测试通过