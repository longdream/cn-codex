# Task: swe-21-fix-float-arithmetic

修复浮点运算精度问题。

## 仓库
- src/mathUtils.js
- test/mathUtils.test.mjs

## 要求
1. add(0.1, 0.2) 应返回 0.3 而非 0.30000000000000004
2. 所有测试通过