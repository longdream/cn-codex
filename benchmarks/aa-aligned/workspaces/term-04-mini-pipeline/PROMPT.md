# Task: term-04-mini-pipeline

在当前工作目录完成多步骤数据流水线。

## 输入
- input/sales.csv
- input/rates.json

## 要求
1. 只保留 status=paid 的订单
2. 计算税后金额 net = amount * (1 - rate)
3. 按 region 聚合输出 by_region.csv
4. 输出 total.json