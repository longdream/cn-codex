# Task: term-02-log-etl

在当前工作目录完成日志 ETL。

## 输入
- input/app.log：每行 `YYYY-MM-DDTHH:MM:SS LEVEL message`

## 要求
1. 统计每个 LEVEL 出现次数
2. 输出 output/levels.csv，带表头 level,count
3. 输出 output/errors.txt：仅包含 ERROR 行的 message