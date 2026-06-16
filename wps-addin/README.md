# CN-Codex WPS 加载项

WPS Office 加载项，连接到 CN-Codex 桌面应用，让 AI 助手可以直接操控 Word 文档。

## 安装

### 方式一：开发模式（推荐用于调试）

1. 确保已安装 WPS Office 并启用 JS 加载项功能
2. 在 WPS 安装目录找到 `oem.ini`，确认以下配置：
   ```ini
   [Support]
   JsApiPlugin=true
   ```
3. 在 WPS 开发者工具中加载本目录作为加载项:
   - 打开 WPS 文字 → 开发工具 → 加载项管理
   - 选择"从本地加载"，选择本 `wps-addin/` 目录
4. 在 CN-Codex 设置中启动 WPS 服务

### 方式二：使用 wpsjs CLI

```bash
npm install -g wpsjs
cd wps-addin
wpsjs debug
```

## 使用

1. 在 CN-Codex 桌面应用中启动 WPS 服务（默认端口 23300）
2. 打开 WPS 文字，加载项会自动连接到 CN-Codex
3. 功能区会出现 "CN-Codex" 标签页，点击"连接状态"可查看连接情况
4. 在 CN-Codex 中对话时，AI 助手可以通过 WPS 工具直接操作文档

## 支持的命令

### 文档管理
- `document.open` - 打开文档
- `document.save` - 保存
- `document.saveAs` - 另存为
- `document.close` - 关闭文档
- `document.getInfo` - 获取文档信息
- `document.getContent` - 读取全文

### 文本操作
- `text.insert` - 插入文本
- `text.replace` - 查找替换
- `text.delete` - 删除文本

### 格式设置
- `format.setFont` - 设置字体
- `format.setParagraph` - 设置段落格式
- `format.setStyle` - 应用样式

### 书签
- `bookmark.add` / `bookmark.goto` / `bookmark.list` / `bookmark.delete`

### 表格
- `table.insert` - 插入表格
- `table.setCell` / `table.getCell` - 读写单元格

### 批注与修订
- `comment.add` / `comment.list` / `comment.delete`
- `revision.accept` / `revision.reject`

### 页眉页脚
- `header.set` / `footer.set`

### 图片
- `image.insert` - 插入图片

### 模板
- `template.fill` - 填充模板占位符

## 自定义 WebSocket 地址

如果 CN-Codex 使用非默认端口，可在 WPS 加载项控制台中设置：

```javascript
wps.PluginStorage.setItem('ws_url', 'ws://127.0.0.1:12345/wps');
```

## 文件结构

```
wps-addin/
  main.js              - 入口文件，事件注册和初始化
  ribbon.xml           - WPS 功能区配置
  commands/
    document.js        - 文档管理命令
    text.js            - 文本操作命令
    format.js          - 格式设置命令
    bookmark.js        - 书签命令
    table.js           - 表格命令
    comment.js         - 批注/修订命令
    header_footer.js   - 页眉页脚命令
    image.js           - 图片命令
    template.js        - 模板填充命令
  utils/
    ws-client.js       - WebSocket 客户端（自动重连）
    protocol.js        - 消息协议分发
```
