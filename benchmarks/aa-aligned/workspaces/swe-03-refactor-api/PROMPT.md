# Task: swe-03-refactor-api

重构价格 API，保持调用方兼容。

## 现状
- src/price.js 导出 calcTotal(items)
- 测试期望新增更清晰 API

## 要求
1. 新增 lineTotal(item)
2. 新增 calcTotalV2(items, options) 含 taxRate
3. 旧 calcTotal 继续可用
4. 所有测试通过