# CN-Codex

<p align="center">
  <img src="app-icon.svg" alt="CN-Codex Logo" width="128" height="128">
</p>

<h3 align="center">AI駆動のプログラミングアシスタント デスクトップアプリ</h3>

<p align="center">
  チャットだけではありません — コードを書き、コマンドを実行し、タスクを自動完了する本格的なAIワークベンチ
</p>

<p align="center">
  <a href="README.md">中文</a> |
  <a href="README.en.md">English</a> |
  <a href="README.ja.md">日本語</a> |
  <a href="README.fr.md">Français</a> |
  <a href="README.de.md">Deutsch</a>
</p>

<p align="center">
  <a href="http://47.113.221.244:8081/">公式サイト</a> •
  <a href="http://47.113.221.244:8081/usage.html">使用ガイド</a> •
  <a href="https://github.com/longdream/cn-codex">GitHub</a>
</p>

---

## なぜ CN-Codex を選ぶのか？

従来のAIコーディングアシスタントは会話しかできません。CN-Codex は**完全なAIワークベンチ**です。コードの読み書き、Shellコマンドの実行、ブラウザの操作、サブエージェントの並列管理、さらにはスマートフォンからリモートでAIの実行進捗を監視することまでできます。

- **40以上の組み込みツール** — ファイル操作、コマンド実行、ブラウザ自動化、サブエージェント、MCPプロトコルなど
- **12以上のLLMプロバイダー** — OpenAI、Anthropic、Google、DeepSeek、火山エンジン、通義千問、智譜、Moonshot、SiliconFlow、百川、Ollama、LM Studio
- **ゴールモード** — 目標を設定すれば、AIが自律的にマルチステップタスクを計画・実行
- **プラグイン + スキル + ロボット** — 拡張可能な自動化システムで独自のAIワークフローを構築
- **スマートフォンQRコード同期** — LAN直接接続またはパブリックリレー、どこからでもAIアシスタントを制御
- **超軽量設計** — インストールパッケージわずか約20MB、サブ秒起動、Rustネイティブ性能でゼロラグ
- **すぐに使える** — ダウンロードしてダブルクリックで起動。CLIインストールや環境変数設定は不要

---

## インターフェースプレビュー

<p align="center">
  <img src="docs/screenshot-main.png" alt="メインインターフェース" width="800">
</p>
<p align="center"><em>初回起動 — 左側にプロジェクト管理、中央にチャットエリアを配置したクリーンなダークインターフェース</em></p>

<br>

<p align="center">
  <img src="docs/screenshot-chat.png" alt="チャットインターフェース" width="800">
</p>
<p align="center"><em>スマートチャット — ストリーミング出力、モデル切り替え、自動承認、一目でわかるステータスバー</em></p>

<br>

<p align="center">
  <img src="docs/screenshot-project.png" alt="プロジェクトモード" width="800">
</p>
<p align="center"><em>プロジェクトモード — チャット/ゴールのデュアルモード切替、ロボットセレクター、プロジェクトディレクトリでAIが作業</em></p>

<br>

<p align="center">
  <img src="docs/screenshot-provider.png" alt="プロバイダー設定" width="800">
</p>
<p align="center"><em>プロバイダー設定 — 複数のLLMプロバイダーをビジュアルGUIで管理、モデルリストとビジョン機能タグをサポート</em></p>

---

## コア機能一覧

| 機能 | 説明 |
|------|------|
| スマートチャット | マルチターンコンテキスト、ストリーミング出力、Markdownレンダリング、セッション検索とフォーク |
| ツール呼び出し | ファイルI/O、Shell、ブラウザ自動化、サブエージェント、メモリ、MCPなど40以上のツール |
| ゴールモード | AI自律マルチステップ実行、トークンバジェット制御とステータス監視をサポート |
| プラグインシステム | Browser、Computer Use、Documents、Presentations、Spreadsheets、Sites、Superpowers |
| ロボット | AIが自動的にスキルとワークフローに紐付けた専門的役割を作成 |
| モバイル | 組み込みWebサーバー + WebSocket、LAN直接接続とパブリックリレーをサポート |
| ターミナルパネル | xterm.js組み込みターミナル、マルチタブ、AIと並行して作業 |
| Hooks | エージェントライフサイクル全体にわたるイベント駆動自動化フック |

> 完全なドキュメントとガイドについては **[使用ガイド](http://47.113.221.244:8081/usage.html)** をご覧ください

---

## クイックスタート

### 1. アプリを起動

`CN-Codex.exe` をダブルクリックして起動。初回実行時に同じディレクトリに `codey/` ランタイムフォルダが作成されます。

### 2. プロバイダーを設定

**設定** → **モデルプロバイダー** → **プロバイダーを追加** → プリセットを選択（例：DeepSeek、OpenAI）→ API KeyとBase URLを入力 → **保存** → **有効化**

### 3. プロジェクトを追加して開始

サイドバーの**フォルダ+**ボタンをクリックしてコードディレクトリを追加し、モデルを選択してチャットを開始。AIがプロジェクトディレクトリ内ですべての操作を実行します。

> 詳細な手順と高度な設定については **[使用ガイド](http://47.113.221.244:8081/usage.html)** をご覧ください

---

## 技術スタック

| レイヤー | 技術 |
|----------|------|
| デスクトップフレームワーク | Tauri v2 |
| バックエンド | Rust (Edition 2024) |
| フロントエンド | React 18 + TypeScript + Vite 6 |
| スタイリング | Tailwind CSS 4 |
| 状態管理 | Zustand |
| 国際化 | react-intl |
| データベース | SQLite |

---

## リンク

| リンク | 説明 |
|--------|------|
| [公式サイト](http://47.113.221.244:8081/) | 製品紹介とダウンロード |
| [使用ガイド](http://47.113.221.244:8081/usage.html) | 初回起動から高度な機能までの完全ドキュメント |
| [GitHub](https://github.com/longdream/cn-codex) | ソースコードとイシュートラッカー |

---

## ライセンス

Apache License 2.0

---

<p align="center">
  <strong>CN-Codex</strong> — AIをあなたのプログラミングパートナーに
</p>
