---
name: hyperframes
description: 使用 HyperFrames 将 HTML/CSS 渲染为 MP4 视频
tags: [video, html, rendering, animation, hyperframes]
---

# HyperFrames - HTML to Video

HyperFrames 是一个开源框架，可以将 HTML、CSS 和可控动画渲染为确定性 MP4 视频。

## 前提条件

- Node.js 22+
- FFmpeg
- `npm install -g hyperframes`

## 常用命令

```bash
# 创建新视频项目
npx hyperframes init my-video

# 浏览器预览 + 热重载
cd my-video
npx hyperframes preview

# 渲染为 MP4
npx hyperframes render

# 验证 HTML 合法性
npx hyperframes lint

# 检查环境依赖
npx hyperframes doctor

# 列出项目中的 composition
npx hyperframes compositions

# 添加预置组件
npx hyperframes add flash-through-white
npx hyperframes add instagram-follow
npx hyperframes add data-chart
```

## HTML Composition 基本结构

```html
<div id="stage"
  data-composition-id="my-video"
  data-start="0"
  data-width="1920"
  data-height="1080">

  <video class="clip"
    data-start="0"
    data-duration="6"
    data-track-index="0"
    src="intro.mp4"
    muted playsinline></video>

  <h1 id="title" class="clip"
    data-start="1"
    data-duration="4"
    data-track-index="1">Launch day</h1>

  <audio
    data-start="0"
    data-duration="6"
    data-track-index="2"
    data-volume="0.5"
    src="music.wav"></audio>

  <script src="https://cdn.jsdelivr.net/npm/gsap@3/dist/gsap.min.js"></script>
  <script>
    const tl = gsap.timeline({ paused: true });
    tl.from("#title", { opacity: 0, y: 40, duration: 0.8 }, 1);
    window.__timelines = window.__timelines || {};
    window.__timelines["my-video"] = tl;
  </script>
</div>
```

## 关键概念

- **Composition**: 一个带有 `data-composition-id` 的顶级容器
- **Clip**: 带 `data-start` / `data-duration` / `data-track-index` 的媒体或元素
- **Track**: 用 `data-track-index` 分层管理元素叠放
- **Seekable Animation**: 支持 GSAP、CSS、Lottie、Three.js、Anime.js、WAAPI 等

## 文档

- 官方文档: https://hyperframes.heygen.com/introduction
- GitHub: https://github.com/heygen-com/hyperframes
- Catalog: https://hyperframes.heygen.com/catalog
