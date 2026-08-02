# assets 资源目录

用于放置项目的可视化与验证材料。

## 建议结构

```text
assets/
  screenshots/     # 关键界面截图
  video/           # Demo 视频文件或 link.txt
  prototype/       # 原型链接、线框导出
```

## 命名建议

- `01-onboarding.*` 建档
- `02-plan.*` 计划
- `03-proactive-modal.*` 主动启动弹窗
- `04-offline-feedback.*` 线下反馈
- `05-intervention.*` 干预确认
- `06-weekly-review.*` 周回顾

## 注意

- 请使用模拟学生数据，避免真实未成年人隐私出镜
- 截图中如含 API Key、本机绝对路径、真实手机号等请打码

## 重新导出方案书 PDF

`02-项目方案书.md` 更新后，在本目录上级（`goai-preliminary-submission/`）执行：

```bat
pandoc "02-项目方案书.md" -f markdown -t html5 -s --metadata title="AI 自主学习教练系统 · 项目方案书" -c "assets/pdf-style.css" -o "assets/02-项目方案书.html"
"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe" --headless --disable-gpu --no-pdf-header-footer --print-to-pdf="02-项目方案书.pdf" "file:///绝对路径/assets/02-项目方案书.html"
```

说明：`pdf-style.css` 为中文排版样式表；Edge 打印需使用 file:// 绝对 URL（中文需 URL 编码）；生成后可删除中间 HTML。
