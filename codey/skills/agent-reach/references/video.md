# 视频/播客

YouTube、B站、小宇宙播客的字幕和转录。

## YouTube (yt-dlp)

### 获取视频元数据

```bash
yt-dlp --dump-json "URL"
```

### 下载字幕

```bash
# 下载字幕 (不下载视频)
yt-dlp --write-sub --write-auto-sub --sub-lang "zh-Hans,zh,en" --skip-download -o "/tmp/%(id)s" "URL"

# 然后读取 .vtt 文件
cat /tmp/VIDEO_ID.*.vtt
```

### 获取评论

```bash
# 提取评论（best-effort，不保证完整）
yt-dlp --write-comments --skip-download --write-info-json \
  --extractor-args "youtube:max_comments=20" \
  -o "/tmp/%(id)s" "URL"
```

### 搜索视频

```bash
yt-dlp --dump-json "ytsearch5:query"
```

### 无字幕兜底：Whisper 音频转写

```bash
agent-reach transcribe "https://www.youtube.com/watch?v=VIDEO_ID"
agent-reach transcribe ./local_audio.mp3 -o /tmp/transcript.txt
```

## B站 / Bilibili（bili-cli 为主，OpenCLI 补字幕）

不要用 yt-dlp 读 B站，优先 bili-cli / OpenCLI。

```bash
bili video BVxxx
bili search "query" --type video -n 5
bili hot -n 10
bili rank -n 10
bili audio BVxxx
opencli bilibili subtitle BVxxx
opencli bilibili search "query" -f yaml
opencli bilibili video BVxxx -f yaml
```

## 小宇宙播客 / Xiaoyuzhou Podcast

```bash
~/.agent-reach/tools/xiaoyuzhou/transcribe.sh --polish "https://www.xiaoyuzhoufm.com/episode/EPISODE_ID"
```

### 检查状态

```bash
agent-reach doctor
```

## 选择指南

| 场景 | 推荐工具 |
|-----|---------|
| YouTube 字幕 | yt-dlp |
| B站视频详情/搜索 | bili-cli |
| B站字幕 | opencli bilibili subtitle |
| 播客转录 | 小宇宙 transcribe.sh |
| 无字幕音视频 | agent-reach transcribe（B站音频先 `bili audio`） |
