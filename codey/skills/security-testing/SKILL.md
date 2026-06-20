---
name: security-testing
description: Use when performing security validation including OWASP Top 10 checks, dependency vulnerability scanning, authentication/authorization testing, and basic penetration testing
---

# 安全测试

## 概述

安全测试识别和验证系统的安全漏洞，防止数据泄露、未授权访问和恶意攻击。安全不是事后补救，而是质量的基本维度。

**核心原则：** 假设攻击者已了解你的系统，验证每一层防御是否有效。

**宣告：** "我正在使用 security-testing 技能进行安全测试。"

## 何时使用

- 系统上线前安全验收
- 新增用户输入/API 端点时
- 认证/授权逻辑变更后
- 引入新依赖库时
- 定期安全审计
- 处理敏感数据的功能开发后

## OWASP Top 10 测试清单（2021）

### A01: 访问控制失效（Broken Access Control）

**测试点：**
- 水平越权：用户A能否访问用户B的数据？
- 垂直越权：普通用户能否执行管理员操作？
- IDOR：直接对象引用是否可被猜测/遍历？
- 功能级访问控制：API 端点是否都有权限校验？

```typescript
describe('访问控制测试', () => {
  test('用户不能访问他人数据 (水平越权)', async () => {
    const userAToken = await loginAs('userA');
    const res = await request(app)
      .get('/api/users/userB-id/orders')
      .set('Authorization', `Bearer ${userAToken}`);
    expect(res.status).toBe(403);
  });

  test('普通用户不能访问管理接口 (垂直越权)', async () => {
    const userToken = await loginAs('normalUser');
    const res = await request(app)
      .get('/api/admin/users')
      .set('Authorization', `Bearer ${userToken}`);
    expect(res.status).toBe(403);
  });

  test('IDOR - 不能通过遍历ID获取数据', async () => {
    const userToken = await loginAs('userA');
    for (let id = 1; id <= 10; id++) {
      const res = await request(app)
        .get(`/api/documents/${id}`)
        .set('Authorization', `Bearer ${userToken}`);
      if (res.status === 200) {
        expect(res.body.ownerId).toBe('userA-id');
      }
    }
  });
});
```

### A02: 加密失败（Cryptographic Failures）

**检查项：**
- [ ] 密码是否使用 bcrypt/argon2 哈希存储？
- [ ] 敏感数据传输是否使用 HTTPS？
- [ ] JWT secret 是否足够强？是否硬编码？
- [ ] 敏感信息是否出现在日志中？
- [ ] 数据库中敏感字段是否加密？

```typescript
test('密码不以明文存储', async () => {
  await request(app).post('/api/users').send({
    email: 'test@test.com',
    password: 'MySecret123!',
  });

  const user = await db.query('SELECT password FROM users WHERE email = ?', ['test@test.com']);
  expect(user.password).not.toBe('MySecret123!');
  expect(user.password).toMatch(/^\$2[aby]\$/); // bcrypt hash
});

test('API 响应不泄露敏感字段', async () => {
  const res = await request(app).get('/api/users/1');
  expect(res.body).not.toHaveProperty('password');
  expect(res.body).not.toHaveProperty('passwordHash');
  expect(res.body).not.toHaveProperty('secret');
});
```

### A03: 注入（Injection）

**测试向量：**
```
SQL 注入:     ' OR '1'='1'; DROP TABLE users; --
NoSQL 注入:   {"$gt": ""}
XSS:          <script>alert('xss')</script>
命令注入:     ; rm -rf / ; cat /etc/passwd
路径遍历:     ../../etc/passwd
模板注入:     {{7*7}}
```

```typescript
describe('注入测试', () => {
  const sqlInjections = [
    "' OR '1'='1",
    "'; DROP TABLE users; --",
    "1 UNION SELECT * FROM users",
    "admin'--",
  ];

  test.each(sqlInjections)('SQL注入无效: %s', async (payload) => {
    const res = await request(app)
      .post('/api/auth/login')
      .send({ email: payload, password: payload });

    expect(res.status).not.toBe(200);
    expect(res.body).not.toHaveProperty('token');
  });

  const xssPayloads = [
    '<script>alert("xss")</script>',
    '<img src=x onerror=alert(1)>',
    'javascript:alert(1)',
    '<svg onload=alert(1)>',
  ];

  test.each(xssPayloads)('XSS 被过滤或转义: %s', async (payload) => {
    await request(app).post('/api/comments').send({ content: payload });
    const res = await request(app).get('/api/comments');

    const lastComment = res.body.data[res.body.data.length - 1];
    expect(lastComment.content).not.toContain('<script');
    expect(lastComment.content).not.toContain('onerror');
  });
});
```

### A04: 不安全设计（Insecure Design）

**检查项：**
- [ ] 密码重置流程是否可被利用？
- [ ] 验证码是否有频率限制？
- [ ] 业务逻辑是否可被绕过（如负数金额）？
- [ ] 批量操作是否有限制？

### A05: 安全配置错误（Security Misconfiguration）

```typescript
describe('安全配置检查', () => {
  test('响应头包含安全头', async () => {
    const res = await request(app).get('/');

    expect(res.headers['x-content-type-options']).toBe('nosniff');
    expect(res.headers['x-frame-options']).toBeDefined();
    expect(res.headers['strict-transport-security']).toBeDefined();
    expect(res.headers['x-xss-protection']).toBeDefined();
  });

  test('不暴露服务器信息', async () => {
    const res = await request(app).get('/');
    expect(res.headers['x-powered-by']).toBeUndefined();
    expect(res.headers['server']).not.toContain('Express');
  });

  test('调试模式未开启', async () => {
    const res = await request(app).get('/api/nonexistent');
    expect(res.body).not.toHaveProperty('stack');
    expect(res.text).not.toContain('at Object.');
  });

  test('CORS 配置正确', async () => {
    const res = await request(app)
      .options('/api/users')
      .set('Origin', 'https://evil.com');

    expect(res.headers['access-control-allow-origin']).not.toBe('*');
    expect(res.headers['access-control-allow-origin']).not.toBe('https://evil.com');
  });
});
```

### A07: 身份验证失败（Authentication Failures）

```typescript
describe('认证安全测试', () => {
  test('暴力破解防护 - 多次失败后锁定', async () => {
    for (let i = 0; i < 5; i++) {
      await request(app).post('/api/auth/login')
        .send({ email: 'target@test.com', password: `wrong${i}` });
    }

    const res = await request(app).post('/api/auth/login')
      .send({ email: 'target@test.com', password: 'correctPassword' });

    expect(res.status).toBe(429);
    expect(res.body.message).toContain('账户已锁定');
  });

  test('会话固定防护 - 登录后 token 更新', async () => {
    const loginRes1 = await request(app).post('/api/auth/login')
      .send({ email: 'user@test.com', password: 'password' });
    const token1 = loginRes1.body.token;

    const loginRes2 = await request(app).post('/api/auth/login')
      .send({ email: 'user@test.com', password: 'password' });
    const token2 = loginRes2.body.token;

    expect(token1).not.toBe(token2);
  });
});
```

## 依赖漏洞扫描

```bash
# Node.js 项目
npm audit
npm audit --production  # 仅生产依赖

# 使用 Snyk（更全面）
npx snyk test

# Python 项目
pip-audit
safety check

# 通用 - OWASP Dependency-Check
dependency-check --project "MyApp" --scan ./
```

**漏洞严重等级处理：**
| 级别 | 处理方式 | 时限 |
|------|---------|------|
| Critical | 立即修复，阻塞发版 | 24小时内 |
| High | 尽快修复 | 1周内 |
| Medium | 计划修复 | 1个月内 |
| Low | 评估后决定 | 下个版本 |

## 自动化安全扫描集成

```yaml
# CI/CD 安全检查
security-scan:
  steps:
    - name: 依赖漏洞扫描
      run: npm audit --audit-level=high
    - name: 代码安全扫描
      run: npx eslint --rule 'security/*' src/
    - name: Secret 检测
      run: npx secretlint "**/*"
    - name: SAST 扫描
      run: npx semgrep --config=auto src/
```

## 输出格式

```markdown
## 安全测试结果

### 漏洞扫描
| 级别 | 数量 | 状态 |
|------|------|------|
| Critical | 0 | PASS |
| High | 1 | 需修复 |
| Medium | 3 | 已记录 |
| Low | 5 | 可接受 |

### OWASP Top 10 覆盖
| 风险项 | 测试结果 | 备注 |
|--------|---------|------|
| A01 访问控制 | PASS | 水平/垂直越权已测试 |
| A02 加密 | PASS | bcrypt + HTTPS |
| A03 注入 | PASS | 参数化查询 + 输入过滤 |
| ... | ... | ... |

### 发现的安全问题
1. [HIGH] CORS 配置过于宽松 → 建议限制允许的域名
2. [MEDIUM] 缺少 rate limiting → 建议添加 express-rate-limit
```

## 验证清单

- [ ] OWASP Top 10 每一项都有对应测试
- [ ] 认证流程（登录/注册/重置）已测试
- [ ] 授权（水平越权/垂直越权/IDOR）已测试
- [ ] 注入（SQL/XSS/命令）已测试
- [ ] 依赖漏洞扫描已执行
- [ ] 安全头配置已验证
- [ ] 敏感数据不泄露（响应/日志）
- [ ] 暴力破解有防护
- [ ] CORS 配置正确
- [ ] 无硬编码密钥/凭据
