# Task: swe-19-fix-type-coercion

修复类型强制转换 bug。

## 仓库
- src/typeUtils.js
- test/typeUtils.test.mjs

## 要求
1. toNumber 对字符串 '1a' 应返回 NaN 而非 1
2. 所有测试通过