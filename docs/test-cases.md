# CN-Codex 测试案例

## 测试分类

| 类别 | 范围 | 工具 |
|------|------|------|
| Rust 单元测试 | 后端逻辑 | cargo test |
| 前端单元测试 | React 组件 + Store | vitest + @testing-library/react |
| 集成测试 | Tauri IPC 通信 | cargo test (integration) |
| E2E 测试 | 完整用户流程 | playwright / webapp-testing skill |

---

## 一、Rust 后端测试

### TC-R001 LLM 档位管理器
```
描述: 验证 LlmTiersManager 默认配置和档位解析
场景:
  1. 默认创建包含 low/medium/high 三个档位
  2. 默认档位为 medium
  3. 场景 "chat" 解析到 low
  4. 场景 "architecture" 解析到 high
  5. 未知场景使用 default_tier
预期: 各场景正确解析到对应的模型配置
```

### TC-R002 Token 用量统计
```
描述: 验证 token 用量记录和统计
场景:
  1. 初始用量为 0
  2. 记录 100 tokens 后总量为 100
  3. 多次记录累加正确
预期: AtomicU64 正确累加
```

### TC-R003 错误类型序列化
```
描述: 验证 AppError 可正确序列化为字符串
场景:
  1. AppError::NotInitialized 序列化
  2. AppError::Custom("test") 序列化
  3. AppError::Io 序列化
预期: 序列化为人类可读的错误消息
```

### TC-R004 AppState 初始化
```
描述: 验证 AppState::new() 的默认值
场景:
  1. locale 默认 "zh-CN"
  2. client 默认 None
  3. request_handle 默认 None
  4. current_thread_id 默认 None
预期: 所有字段正确初始化
```

---

## 二、前端测试

### TC-F001 Settings Store
```
描述: 验证 settingsStore 默认值和操作
场景:
  1. 默认 locale 为 "zh-CN"
  2. 默认 theme 为 "dark"
  3. setLocale("en-US") 切换语言
  4. setTheme("light") 切换主题
预期: Zustand store 状态正确更新
```

### TC-F002 国际化加载
```
描述: 验证 react-intl 正确加载中英文消息
场景:
  1. zh-CN 消息包含所有必需 key
  2. en-US 消息包含所有必需 key
  3. 两个语言包 key 一致
预期: 所有翻译 key 正确加载
```

### TC-F003 App 组件渲染
```
描述: 验证 App 根组件正确渲染
场景:
  1. 包含 IntlProvider
  2. 显示 "CN-Codex" 标题
  3. 默认显示中文界面
预期: 组件树正确渲染
```

---

## 三、集成测试

### TC-I001 Greet 命令
```
描述: 验证 greet Tauri 命令正常工作
输入: name = "测试用户"
预期输出: "你好, 测试用户! 欢迎使用 CN-Codex"
```

### TC-I002 Server Status 命令
```
描述: 验证 get_server_status 返回正确状态
场景: 服务未初始化时调用
预期输出: { initialized: false, currentThreadId: null, locale: "zh-CN" }
```

---

## 四、E2E 测试（后续阶段）

### TC-E001 首次启动流程
```
描述: 验证应用完整启动流程
步骤:
  1. 启动应用
  2. 验证窗口标题为 "CN-Codex"
  3. 验证显示中文界面
  4. 验证侧边栏可见
预期: 应用正常启动并显示
```

### TC-E002 语言切换
```
描述: 验证运行时语言切换
步骤:
  1. 默认为中文
  2. 切换到英文
  3. 验证界面文字变为英文
  4. 切换回中文
预期: 语言无缝切换
```
