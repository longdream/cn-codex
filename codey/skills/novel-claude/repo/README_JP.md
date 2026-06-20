# 🚀 Novel-Claude V3: エージェント型小説自動生成フレームワーク

> [!WARNING]
> **本プロジェクトは現在テスト段階にあり、多くの機能が未完成です。慎重に使用してください。**

Novel-Claude は、大規模言語モデル（例：智譜 GLM-4）に基づいて構築された「全自動長編小説生成パイプライン」です。V3 バージョンでは、従来の線形スクリプト・パイプラインから、極めて高い拡張性を持つ **マイクロカーネル ＋ プラグイン・アーキテクチャ (Microkernel & Plugin Architecture)** へと進化しました。

基盤となる `EventBus` イベント・エンジンと動的な `PluginManager` を通じて、非常に複雑なコミュニティ・プラグイン・エコシステム（Skills）と、ReAct によるマルチターンの対話型エージェント（Agents）をサポートしています。

## ✨ 主な特徴

- **マイクロカーネル・プラグイン・システム**: RAG メモリ検索や戦闘描写の合理性チェックなどの付加機能はすべて、メイン・タイムラインから剥離された「プラグイン（Skills）」として管理されます。ホットリロード（Hot-Reload）とエラー分離（Fault Tolerance）をサポートしており、単一のプラグインがクラッシュしても、数時間に及ぶ生成プロセスが中断されることはありません。
- **高度なエージェント支援**:
  - 🖋️ **Editor Agent (辛口編集者エージェント)**: ReAct ループを用いてドラフトを厳格に校閲し、視点のブレや文脈の断絶を自動的に修正します。
  - 🤖 **Skill Builder Agent (メタ生成器)**: CLI で自然言語による要望を入力するだけで、システムが**自動的に有効なプラグイン（Skill）コードを生成・実装**します。
- **コスト削減と効率化 (Batch API)**: 智譜/OpenAI 形式の Batch API にネイティブ対応。オフラインで大量の章を 50% の低コストで並列生成し、完了後に自動で結合・同期します。

---

## 🏗️ アーキテクチャ概要

生成パイプラインは 3 つのコア・エンジンに分割されており、`NovelContext` による共有状態管理と `EventBus` によるイベント・ブロードキャストによって統合されています。

1. `world_builder.py` (世界創造者): 一行の創意（Logline）から、厳密な JSON 形式の背景設定、陣営、キャラクター・リストを構築します。
2. `volume_planner.py` (プロット・プランナー): 全 10 巻の概要を作成し、各巻をシーン単位のプロット（Beats）に分解。独自のアルゴリズムにより、1 章あたり正確に 5,000 字の出力を強制・正規化します。
3. `scene_writer.py` (執筆ワークショップ): 各シーンのタスクを実行するサブエージェントを生成し、最終的に Editor Agent による定稿を行います。

```text
novel_claude/
├── core/                       # マイクロカーネル・エンジン
│   ├── event_bus.py            # グローバル・サービス・バス（障害耐性）
│   ├── plugin_manager.py       # 動的プラグイン・スキャナー & ローダー
│   ├── base_skill.py           # V3 標準プラグイン基底クラス
│   ├── novel_context.py        # 共有ライフサイクル・コンテキスト
│   └── agents/                 # 推論エージェント
│       ├── editor_agent.py     # ReAct 編集者エージェント
│       └── skill_builder_agent.py  # メタ生成エージェント
├── skills/                     # プラグイン・ディレクトリ
│   └── core_memory_rag/        # 標準 RAG メモリ検索プラグイン
├── world_builder.py            # エンジン 1: 世界観構築
├── volume_planner.py           # エンジン 2: プロット分割
├── scene_writer.py             # エンジン 3: シーン執筆と結合
├── cli.py                      # ターミナル・エントリー
└── utils/                      # 設定 & LLM クライアント
```

---

## 🛠️ インストールと使用方法

### 1. 環境構築
Python >= 3.10 が必要です。
```bash
# 依存関係のインストール
uv pip install -r requirements.txt
```

### 2. CLI の使用例

#### 基本的な生成フロー
```bash
# 段階 1：世界観の初期化
uv run python cli.py init "サイバーパンクな世界で魔術をハッキングする修仙物語"

# 段階 2：全 10 巻のメイン・プロット作成
uv run python cli.py plan

# 段階 2.5：第 1 巻の 50 章分の詳細プロット作成
uv run python cli.py plan --volume 1

# 段階 3：執筆エージェントを起動して第 1 巻の 1〜5 章を生成
uv run python cli.py write --volume 1 --chapters "1-5"
```

#### Batch API フロー
```bash
# JSONL リクエストファイルを構築
uv run python cli.py batch-build --volume 1 --chapters "1-50"

# 非同期タスクを提交（Batch ID が返されます、必ず保存してください）
uv run python cli.py batch-submit .batch/vol_01_ch_1_50_req.jsonl

# 同期してマージ（ステータスをポーリングし自動ダウンロード）
uv run python cli.py batch-sync <batch_id>
```

#### V3 プラグイン管理コマンド
```bash
# プラグイン一覧を表示
uv run python cli.py skills list

# 特定のプラグインを無効化/有効化
uv run python cli.py skills disable ext_gold_finger
uv run python cli.py skills enable ext_gold_finger

# 全プラグインのホットリロード
uv run python cli.py skills reload
```

---

## 🔌 V3 プラグイン・システム

### プラグインの作成方法（手動）

1. `skills/` 内にフォルダを作成（例：`skills/my_awesome_skill/`）。
2. `skill.py` を作成：
```python
from core.base_skill import BaseSkill

class MyAwesomeSkill(BaseSkill):
    def __init__(self, context):
        super().__init__(context)
        self.name = "MyAwesomeSkill"

    def on_init(self):
        print(f"[{self.name}] プラグインが初期化されました！")

    def on_before_scene_write(self, prompt_payload, beat_data):
        # 執筆開始前にプロンプトを注入
        prompt_payload.append("\n[システム注入] 注意：主人公の行動は常に冷徹かつ理性的である必要があります。")
        return prompt_payload
```

### ライフサイクル・フック一覧

| メソッド | トリガー | 用途 |
|---------|---------|------|
| `on_init()` | ロード時 | リソース初期化 |
| `on_volume_planning()` | プロット時 | アウトライン干渉・修正 |
| `on_before_scene_write()` | 執筆前 | コンテキスト・記憶注入 |
| `on_after_scene_write()` | 執筆後 | 統計・データベース保存 |
| `on_chapter_render()` | 章の最終描画時 | プレースホルダ置換 |
| `get_llm_tools()` | LLM 呼び出し時 | ツール登録 |

### アクティブ・ツール・コーリング

V3 プラグインは、コンテキストの「注入」だけでなく、AI が**能動的に操作できるツール**を提供できます。

**例：Gold Finger (状態パネル)**
`skills/ext_gold_finger/` で実装：
- **受動的**: 各章の冒頭で主人公のステータス（銀両、スキル）を注入。
- **能動的**: `simplify_skill` ツールを提供。AI は物語の中で「銀両」を消費してスキルを「簡略化」する決定ができます。

### プラグイン・トグル・メカニズム

`.disabled` タグファイルをプラグインフォルダに生成することでトグルを実現。CLI の `skills enable/disable` で切り替え可能。

### 自動生成（メタ生成）
```bash
uv run python cli.py skills build "戦闘描写の合理性をチェックするプラグインを書いて"
```
システムは自動的に標準に準拠したコードを生成し、ホットリロードで適用します。

---

## 📂 ディレクトリ構造

| パス | 説明 |
|------|------|
| `cli.py` | CLI ターミナル・エントリー |
| `world_builder.py` | 世界観初期化エンジン |
| `volume_planner.py` | 分巻大纲計画エンジン |
| `scene_writer.py` | シーン執筆・マージエンジン |
| `core/` | マイクロカーネル・コア・モジュール |
| `core/agents/` | エージェント実装 |
| `skills/` | プラグイン・ディレクトリ |
| `utils/` | ユーティリティ（LLMクライアント、設定等）|
| `docs/CLI_COMMANDS.md` | 完全な CLI コマンド文書 |

---

## 📜 組み込みプラグイン

| プラグイン | 説明 |
|------|------|
| `ext_gold_finger` | 金手指プラグイン |
| `ext_world_highlight_system` | 世界ハイライトシステム |
| `ext_handsome_protagonist` | 主人公光环プラグイン |
| `core_memory_rag` | コアメモリ RAG システム |