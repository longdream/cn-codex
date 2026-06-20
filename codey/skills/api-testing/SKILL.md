---
name: api-testing
description: Use when testing REST/GraphQL APIs, including endpoint validation, contract testing, error handling, authentication flows, and mock service setup
---

# API/接口测试

## 概述

API 测试是集成测试的核心组成部分，验证接口的正确性、健壮性、安全性和性能。API 是前后端的契约边界，其质量直接影响系统整体可靠性。

**核心原则：** 测试 API 的行为（输入→输出），而不是内部实现细节。

**宣告：** "我正在使用 api-testing 技能进行接口测试。"

## 何时使用

- 新增或修改 API 端点
- 验证 API 契约（前后端协议一致性）
- 第三方 API 集成测试
- API 安全性验证（认证、授权、输入校验）
- API 性能基线建立

## 测试维度

### 1. 功能正确性

验证 API 在正常输入下返回正确结果。

```typescript
describe('GET /api/users/:id', () => {
  test('返回存在用户的完整信息', async () => {
    const res = await request(app).get('/api/users/1');

    expect(res.status).toBe(200);
    expect(res.body).toMatchObject({
      id: 1,
      email: expect.any(String),
      name: expect.any(String),
      createdAt: expect.any(String),
    });
  });

  test('不存在的用户返回 404', async () => {
    const res = await request(app).get('/api/users/99999');

    expect(res.status).toBe(404);
    expect(res.body.error).toBe('User not found');
  });
});
```

### 2. 输入验证

验证 API 对无效输入的处理。

**必须覆盖的输入场景：**
| 场景 | 示例 | 预期响应 |
|------|------|---------|
| 缺少必填字段 | `{}` | 400 + 字段错误信息 |
| 类型错误 | `{age: "abc"}` | 400 + 类型错误 |
| 超出范围 | `{age: -1}` | 400 + 范围错误 |
| 超长字符串 | 10000字符的name | 400 + 长度限制 |
| SQL/XSS 注入 | `'; DROP TABLE--` | 400 或安全转义 |
| 特殊字符 | `\0`, `\n`, unicode | 正确处理 |

```typescript
describe('POST /api/users - 输入验证', () => {
  test.each([
    [{ name: '' }, '名称不能为空'],
    [{ name: 'a'.repeat(256) }, '名称不能超过255字符'],
    [{ email: 'invalid' }, '邮箱格式不正确'],
    [{ age: -1 }, '年龄必须为正整数'],
    [{ age: 'abc' }, '年龄必须为数字'],
  ])('无效输入 %j 返回错误: %s', async (body, expectedError) => {
    const res = await request(app)
      .post('/api/users')
      .send(body);

    expect(res.status).toBe(400);
    expect(res.body.message).toContain(expectedError);
  });
});
```

### 3. 认证与授权

```typescript
describe('API 认证测试', () => {
  test('无 token 返回 401', async () => {
    const res = await request(app).get('/api/protected');
    expect(res.status).toBe(401);
  });

  test('过期 token 返回 401', async () => {
    const expiredToken = generateToken({ exp: Date.now() / 1000 - 3600 });
    const res = await request(app)
      .get('/api/protected')
      .set('Authorization', `Bearer ${expiredToken}`);
    expect(res.status).toBe(401);
  });

  test('权限不足返回 403', async () => {
    const userToken = generateToken({ role: 'user' });
    const res = await request(app)
      .delete('/api/admin/users/1')
      .set('Authorization', `Bearer ${userToken}`);
    expect(res.status).toBe(403);
  });

  test('有效 admin token 允许操作', async () => {
    const adminToken = generateToken({ role: 'admin' });
    const res = await request(app)
      .delete('/api/admin/users/1')
      .set('Authorization', `Bearer ${adminToken}`);
    expect(res.status).toBe(200);
  });
});
```

### 4. 响应格式验证

```typescript
test('响应结构符合契约', async () => {
  const res = await request(app).get('/api/products');

  expect(res.headers['content-type']).toMatch(/json/);
  expect(res.body).toHaveProperty('data');
  expect(res.body).toHaveProperty('pagination');
  expect(res.body.pagination).toMatchObject({
    page: expect.any(Number),
    pageSize: expect.any(Number),
    total: expect.any(Number),
  });

  res.body.data.forEach((item: any) => {
    expect(item).toHaveProperty('id');
    expect(item).toHaveProperty('name');
    expect(item).toHaveProperty('price');
    expect(typeof item.price).toBe('number');
  });
});
```

### 5. 幂等性与并发

```typescript
describe('幂等性测试', () => {
  test('PUT 请求多次执行结果一致', async () => {
    const payload = { name: 'Updated Name' };

    const res1 = await request(app).put('/api/users/1').send(payload);
    const res2 = await request(app).put('/api/users/1').send(payload);

    expect(res1.body).toEqual(res2.body);
  });

  test('DELETE 已删除资源返回 404', async () => {
    await request(app).delete('/api/users/99');
    const res = await request(app).delete('/api/users/99');
    expect(res.status).toBe(404);
  });
});
```

## HTTP 状态码验证清单

| 状态码 | 含义 | 触发条件 |
|--------|------|---------|
| 200 | 成功 | GET/PUT/PATCH 正常 |
| 201 | 创建成功 | POST 新资源 |
| 204 | 无内容 | DELETE 成功 |
| 400 | 请求无效 | 参数校验失败 |
| 401 | 未认证 | 缺少或无效 token |
| 403 | 无权限 | 权限不足 |
| 404 | 不存在 | 资源未找到 |
| 409 | 冲突 | 重复创建 |
| 422 | 不可处理 | 业务规则违反 |
| 429 | 限流 | 请求过频繁 |
| 500 | 服务器错误 | 未处理异常 |

## 契约测试

验证 API 实现与文档/前端期望一致：

```typescript
import { validateSchema } from './helpers';

test('GET /api/users 响应符合 OpenAPI Schema', async () => {
  const res = await request(app).get('/api/users');
  const errors = validateSchema(res.body, 'UserListResponse');
  expect(errors).toHaveLength(0);
});
```

## Mock 服务（测试第三方依赖）

当 API 依赖第三方服务时，使用 Mock 隔离：

```typescript
import nock from 'nock';

beforeEach(() => {
  nock('https://api.payment.com')
    .post('/charge')
    .reply(200, { transactionId: 'mock-tx-001', status: 'success' });
});

afterEach(() => {
  nock.cleanAll();
});

test('支付接口正确处理第三方响应', async () => {
  const res = await request(app)
    .post('/api/orders/1/pay')
    .send({ amount: 100 });

  expect(res.status).toBe(200);
  expect(res.body.transactionId).toBe('mock-tx-001');
});
```

## 执行流程

1. **环境准备** — 启动测试数据库，seed 基础数据
2. **执行测试** — `npm test -- --testPathPattern=api`
3. **验证结果** — 检查通过率和覆盖率
4. **清理环境** — 重置数据库状态

## 验证清单

- [ ] 所有端点的 CRUD 操作已覆盖
- [ ] 所有必填字段的缺失/无效输入已测试
- [ ] 认证（无token/过期token/无效token）已测试
- [ ] 授权（不同角色权限边界）已测试
- [ ] 分页、排序、筛选参数已验证
- [ ] 错误响应格式一致且有意义
- [ ] 响应结构符合 API 文档/契约
- [ ] 幂等性（PUT/DELETE）已验证
- [ ] 并发安全性已考虑
