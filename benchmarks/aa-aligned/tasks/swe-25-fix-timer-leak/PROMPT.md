# Task: swe-25-fix-timer-leak

修复定时器内存泄漏。

## 仓库
- src/timer.js
- test/timer.test.mjs

## 要求
1. 重复调用 setInterval 时未清除旧定时器
2. 修复使所有测试通过