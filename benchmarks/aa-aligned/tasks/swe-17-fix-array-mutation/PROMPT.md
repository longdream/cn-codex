# Task: swe-17-fix-array-mutation

修复数组副作用 bug。

## 仓库
- src/arrayUtils.js
- test/arrayUtils.test.mjs

## 要求
1. removeFalsy 误修改了原数组
2. 修复为纯函数不修改原数组
3. 所有测试通过