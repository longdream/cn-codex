---
name: performance-testing
description: Use when validating system performance under load, including load testing, stress testing, benchmark testing, and performance regression detection
---

# 性能测试

## 概述

性能测试验证系统在预期和极端负载下的行为表现。包括响应时间、吞吐量、资源利用率、稳定性等维度。性能问题往往在上线后才暴露，代价高昂。

**核心原则：** 性能测试不是"能跑就行"，是要建立可量化的基线并持续监控回归。

**宣告：** "我正在使用 performance-testing 技能进行性能测试。"

## 何时使用

- 系统上线前性能验收
- 新功能可能影响性能时
- 用户反馈慢/超时问题
- 容量规划（预估支撑用户量）
- 架构变更后性能回归验证
- 数据库迁移/索引变更后

## 性能测试类型

| 类型 | 目标 | 场景 |
|------|------|------|
| **基准测试** | 建立正常负载下的性能基线 | 单用户/低并发，测量最佳响应时间 |
| **负载测试** | 验证预期负载下系统是否达标 | 模拟正常用户量，持续运行 |
| **压力测试** | 找到系统极限/崩溃点 | 逐步增加负载直到系统降级或崩溃 |
| **浸泡测试** | 检测内存泄漏/资源耗尽 | 正常负载持续长时间（4-24小时） |
| **峰值测试** | 验证突发流量处理能力 | 瞬间大幅增加负载 |

## 性能指标

### 关键指标（必测）

| 指标 | 说明 | 典型要求 |
|------|------|---------|
| **P50 响应时间** | 50%请求的响应时间 | < 200ms |
| **P95 响应时间** | 95%请求的响应时间 | < 1000ms |
| **P99 响应时间** | 99%请求的响应时间 | < 3000ms |
| **吞吐量(RPS)** | 每秒处理请求数 | 依业务而定 |
| **错误率** | 失败请求占比 | < 1% |
| **并发用户数** | 同时活跃连接数 | 依业务而定 |

### 资源指标（监控）

| 指标 | 报警阈值 |
|------|---------|
| CPU 使用率 | > 80% |
| 内存使用率 | > 85% |
| 磁盘 I/O | 接近硬件极限 |
| 网络带宽 | > 70% |
| 数据库连接池 | > 80% 占用 |
| 事件循环延迟(Node) | > 100ms |

## 工具链

### k6（推荐 — 代码优先）

```javascript
// load-test.js
import http from 'k6/http';
import { check, sleep } from 'k6';
import { Rate, Trend } from 'k6/metrics';

const errorRate = new Rate('errors');
const responseTime = new Trend('response_time');

export const options = {
  stages: [
    { duration: '1m', target: 20 },   // 1分钟内升到20并发
    { duration: '3m', target: 20 },   // 保持20并发3分钟
    { duration: '1m', target: 50 },   // 1分钟内升到50并发
    { duration: '3m', target: 50 },   // 保持50并发3分钟
    { duration: '1m', target: 0 },    // 1分钟内降到0
  ],
  thresholds: {
    http_req_duration: ['p(95)<1000', 'p(99)<3000'],
    errors: ['rate<0.01'],
    http_req_failed: ['rate<0.01'],
  },
};

export default function () {
  const res = http.get('http://localhost:3000/api/products');

  check(res, {
    'status is 200': (r) => r.status === 200,
    'response time < 500ms': (r) => r.timings.duration < 500,
    'body has data': (r) => JSON.parse(r.body).data.length > 0,
  });

  errorRate.add(res.status !== 200);
  responseTime.add(res.timings.duration);

  sleep(1);
}
```

**执行命令：**
```bash
# 运行负载测试
k6 run load-test.js

# 输出 JSON 结果
k6 run --out json=results.json load-test.js

# 带 HTML 报告
k6 run --out json=results.json load-test.js && k6-reporter results.json
```

### autocannon（Node.js 快速基准）

```bash
# 快速基准测试：10并发，30秒
npx autocannon -c 10 -d 30 http://localhost:3000/api/products

# 带请求体的POST测试
npx autocannon -c 20 -d 60 -m POST \
  -H "Content-Type: application/json" \
  -b '{"email":"test@test.com","password":"test123"}' \
  http://localhost:3000/api/auth/login
```

### 代码内基准测试

```typescript
// benchmark.test.ts (Vitest)
import { bench, describe } from 'vitest';

describe('数据处理性能', () => {
  bench('处理1000条记录', () => {
    processRecords(generateRecords(1000));
  });

  bench('处理10000条记录', () => {
    processRecords(generateRecords(10000));
  });
});
```

## 测试执行流程

### 1. 环境准备
- 使用与生产环境相似的配置
- 预热（warmup）：先运行少量请求让 JIT/连接池稳定
- 准备测试数据（接近生产数据量级）
- 确保监控工具已就绪

### 2. 基线建立
```bash
# 单用户基准
k6 run --vus 1 --duration 1m baseline.js
```
记录：P50、P95、P99 响应时间、RPS

### 3. 负载测试
逐步增加负载，观察性能变化：
```
阶段1: 10 VU × 3min → 记录指标
阶段2: 30 VU × 3min → 记录指标
阶段3: 50 VU × 3min → 记录指标
阶段4: 100 VU × 3min → 记录指标
```

### 4. 压力测试
找到崩溃点：
```
持续增加 VU 直到错误率 > 5% 或 P95 > 5s
记录系统崩溃点的并发数
```

### 5. 结果分析

**性能达标判断：**
```
✅ 通过条件（全部满足）：
  - P95 响应时间 < 目标值
  - 错误率 < 1%
  - 无内存泄漏（浸泡测试）
  - 资源使用率在安全范围内

❌ 不通过需排查：
  - 哪个端点最慢？
  - 瓶颈在哪层？（网络/应用/数据库）
  - 是否有 N+1 查询？
  - 是否缺少缓存/索引？
```

## 性能回归检测

在 CI 中加入性能回归检测：

```yaml
# 每次 PR 运行快速性能检查
performance-check:
  script:
    - k6 run --duration 30s --vus 5 smoke-test.js
  thresholds:
    # 不允许 P95 比基线慢超过 20%
    http_req_duration_p95: < baseline_p95 * 1.2
```

## 输出格式

性能测试结果记录在测试报告中：

```markdown
## 性能测试结果

### 测试环境
- 机器配置：[CPU/内存/磁盘]
- 网络：[本地/局域网/公网]
- 数据量：[数据库记录数]

### 基准数据
| 端点 | P50 | P95 | P99 | RPS | 错误率 |
|------|-----|-----|-----|-----|--------|
| GET /api/products | 45ms | 120ms | 250ms | 850 | 0% |
| POST /api/orders | 89ms | 340ms | 890ms | 420 | 0.1% |

### 负载测试（50 VU）
| 指标 | 结果 | 目标 | 状态 |
|------|------|------|------|
| P95 响应时间 | 680ms | < 1000ms | PASS |
| 错误率 | 0.3% | < 1% | PASS |
| 吞吐量 | 320 RPS | > 200 RPS | PASS |

### 压力测试
- 系统崩溃点：约 200 并发用户
- 降级表现：P95 > 5s，错误率 > 10%
- 瓶颈：数据库连接池耗尽

### 建议
1. [优化建议]
2. [扩容建议]
```

## 验证清单

- [ ] 关键 API 端点都有性能基线
- [ ] 负载测试覆盖预期用户量
- [ ] 压力测试确定了系统极限
- [ ] P95/P99 响应时间达标
- [ ] 错误率在可接受范围
- [ ] 无明显内存泄漏
- [ ] 性能瓶颈已定位
- [ ] 优化建议已记录
