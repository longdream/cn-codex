# Task: swe-27-fix-promise-chain

修复 Promise 链错误吞没。

## 仓库
- src/promiseUtils.js
- test/promiseUtils.test.mjs

## 要求
1. retry 函数在多次失败后未正确抛出最终错误
2. 修复使所有测试通过