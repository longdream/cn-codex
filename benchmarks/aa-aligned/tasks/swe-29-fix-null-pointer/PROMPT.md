# Task: swe-29-fix-null-pointer

修复空指针解引用。

## 仓库
- src/safeAccess.js
- test/safeAccess.test.mjs

## 要求
1. safeGet(obj, path) 在路径中间为 null 时抛异常
2. 修复为返回 undefined
3. 所有测试通过