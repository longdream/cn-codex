---
name: baidu-search-tiequan
description: "在百度中搜索"铁拳教育"相关信息。打开百度首页，输入搜索关键词，执行搜索操作。"
---

# 百度搜索「铁拳教育」

> 此技能基于用户实际录制操作生成。

## 变量
- `keyword` — 搜索关键词，默认值：`铁拳教育`

## 步骤

### 1. 打开百度首页

使用 `browser_run` 导航到百度首页：

```json
{
  "url": "https://www.baidu.com/",
  "actions": [
    {"type": "wait_for_selector", "selector": "#chat-textarea"}
  ]
}
```

### 2. 在输入框中输入搜索关键词

点击输入框并输入关键词：

```json
{
  "url": "https://www.baidu.com/",
  "actions": [
    {"type": "fill", "selector": "#chat-textarea", "text": "{{keyword}}"},
    {"type": "press", "selector": "#chat-textarea", "key": "Enter"}
  ]
}
```

### 3. 等待搜索结果页面加载

```json
{
  "actions": [
    {"type": "wait_for_selector", "selector": "#content_left"},
    {"type": "screenshot", "path": "baidu-search-result.png"}
  ]
}
```

## 注意事项

- 百度搜索可能会触发**图形验证码**（滑块验证），此时需要人工干预完成验证。
- 默认搜索关键词为"铁拳教育"，可以通过修改变量 `keyword` 搜索其他内容。
- 如果遇到验证码，流程中会自动暂停等待人工验证。

## 回放方式

执行此技能时，请先确保 Chrome 浏览器已通过 `recording_control: {"action": "launch_browser"}` 启动，然后按上述步骤依次调用 `browser_run`。