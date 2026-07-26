# Task: term-01-json-transform

在当前工作目录完成终端数据处理任务。

## 输入
- input/users.json：用户数组

## 要求
1. 过滤 active == true 的用户
2. 按 score 降序排序
3. 输出到 output/top_active.json，只保留 id/name/score 三个字段
4. 额外生成 output/summary.txt，内容：count=<N>;max=<MAX_SCORE>