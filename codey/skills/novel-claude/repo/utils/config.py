import os
import json
import sqlite3
import threading
from pathlib import Path

try:
    import tomllib
except ModuleNotFoundError:
    import tomli as tomllib  # Python < 3.11 fallback

# ---------------------------------------------------------------------------
# LLM 配置完全继承自 cn-codex 的运行时状态。
#
# 本 skill 不维护独立的 LLM 配置，不要在此文件或 config.json 中
# 添加任何 API key / base_url / model 的硬编码或覆盖。
# 用户在 cn-codex 设置里选了什么 provider + model，这里就用什么。
#
# 读取优先级：
#   1. usage.db SQLite (cn-codex 运行时的真实 active provider/model)
#   2. config.toml (回退，可能与运行时状态不同步)
#
# Path: repo is at codey/skills/novel-claude/repo/, codey/ is 3 levels up
# ---------------------------------------------------------------------------
_REPO_ROOT = Path(__file__).parent.parent
_CODEY_DIR = _REPO_ROOT.parent.parent.parent  # codey/
_CN_CODEX_CONFIG = _CODEY_DIR / "config.toml"
_USAGE_DB = _CODEY_DIR / "usage.db"

_toml_config: dict = {}
if _CN_CODEX_CONFIG.exists():
    with open(_CN_CODEX_CONFIG, "rb") as f:
        _toml_config = tomllib.load(f)

# ---------------------------------------------------------------------------
# Load config.json for workspace settings (novel-specific, not LLM config)
# ---------------------------------------------------------------------------
_config_path = _REPO_ROOT / "config.json"
_config = {}
if _config_path.exists():
    with open(_config_path, 'r', encoding='utf-8') as f:
        _config = json.load(f)


def _read_db_state() -> dict:
    """Read active provider info from cn-codex's usage.db SQLite store."""
    if not _USAGE_DB.exists():
        return {}
    try:
        conn = sqlite3.connect(str(_USAGE_DB))
        cur = conn.cursor()
        cur.execute(
            "SELECT key, value FROM app_state "
            "WHERE key IN ('active-provider', 'active-model', 'providers')"
        )
        result = {k: v for k, v in cur.fetchall()}
        conn.close()
        return result
    except Exception:
        return {}


def _resolve_from_db() -> tuple[str, str, str] | None:
    """Try to resolve api_key, base_url, model from the SQLite runtime state.

    Returns (api_key, base_url, model) or None if unavailable.
    """
    db = _read_db_state()
    active_id = db.get("active-provider")
    providers_json = db.get("providers")
    if not active_id or not providers_json:
        return None
    try:
        providers = json.loads(providers_json)
    except (json.JSONDecodeError, TypeError):
        return None

    provider = next((p for p in providers if p.get("id") == active_id), None)
    if not provider:
        return None

    api_key = provider.get("apiKey", "")
    base_url = provider.get("baseUrl", "")
    if not api_key or not base_url:
        return None

    active_model_raw = db.get("active-model", "")
    if ":" in active_model_raw:
        model = active_model_raw.split(":", 1)[1]
    else:
        models = provider.get("models", [])
        model = models[0]["id"] if models else ""

    return api_key, base_url, model


def _resolve_from_toml() -> tuple[str, str, str]:
    """Fallback: resolve from config.toml."""
    provider_id = _toml_config.get("model_provider", "openai")
    providers = _toml_config.get("model_providers", {})
    provider = providers.get(provider_id, {})
    api_key = provider.get("experimental_bearer_token", "") or os.getenv("OPENAI_API_KEY", "")
    base_url = provider.get("base_url", "https://api.openai.com/v1")
    model = _toml_config.get("model", "gpt-4")
    return api_key, base_url, model


_db_result = _resolve_from_db()
if _db_result:
    _api_key, _base_url, _model_id = _db_result
else:
    _api_key, _base_url, _model_id = _resolve_from_toml()

MINIMAX_API_KEY = _api_key
MINIMAX_BASE_URL = _base_url
MODEL_ID = _model_id
FLASH_MODEL_ID = MODEL_ID

# Legacy aliases for backward compatibility
ANTHROPIC_API_KEY = MINIMAX_API_KEY
ANTHROPIC_BASE_URL = MINIMAX_BASE_URL

# Workspace Settings - read from config.json first, fallback to env
_noval_name_from_config = _config.get("workspace", {}).get("novel_name", "")
NOVEL_NAME = _noval_name_from_config if _noval_name_from_config else os.getenv("NOVEL_NAME", "").strip()
NOVEL_DIR = f".novel_{NOVEL_NAME}" if NOVEL_NAME else ".novel"

SETTINGS_DIR = os.path.join(NOVEL_DIR, "settings")
VOLUMES_DIR = os.path.join(NOVEL_DIR, "volumes")
MANUSCRIPTS_DIR = os.path.join(NOVEL_DIR, "manuscripts")
MEMORY_DIR = os.path.join(NOVEL_DIR, "memory")
BATCH_DIR = os.path.join(NOVEL_DIR, "batch_jobs")

# Ensure base directories exist
for d in [NOVEL_DIR, SETTINGS_DIR, VOLUMES_DIR, MANUSCRIPTS_DIR, MEMORY_DIR, BATCH_DIR]:
    os.makedirs(d, exist_ok=True)


def reload_workspace():
    """Reload workspace settings from config.json (call after modifying config)"""
    global NOVEL_NAME, NOVEL_DIR, SETTINGS_DIR, VOLUMES_DIR, MANUSCRIPTS_DIR, MEMORY_DIR, BATCH_DIR

    # Re-read config.json
    if _config_path.exists():
        with open(_config_path, 'r', encoding='utf-8') as f:
            _config = json.load(f)

    _noval_name_from_config = _config.get("workspace", {}).get("novel_name", "")
    NOVEL_NAME = _noval_name_from_config if _noval_name_from_config else os.getenv("NOVEL_NAME", "").strip()
    NOVEL_DIR = f".novel_{NOVEL_NAME}" if NOVEL_NAME else ".novel"

    SETTINGS_DIR = os.path.join(NOVEL_DIR, "settings")
    VOLUMES_DIR = os.path.join(NOVEL_DIR, "volumes")
    MANUSCRIPTS_DIR = os.path.join(NOVEL_DIR, "manuscripts")
    MEMORY_DIR = os.path.join(NOVEL_DIR, "memory")
    BATCH_DIR = os.path.join(NOVEL_DIR, "batch_jobs")

    # Ensure directories exist
    for d in [NOVEL_DIR, SETTINGS_DIR, VOLUMES_DIR, MANUSCRIPTS_DIR, MEMORY_DIR, BATCH_DIR]:
        os.makedirs(d, exist_ok=True)


# Thread management for graceful shutdown
_active_threads = []

def register_background_task(target, *args, **kwargs):
    """
    Register and start a background thread that will be tracked for graceful shutdown.
    """
    thread = threading.Thread(target=target, args=args, kwargs=kwargs)
    thread.daemon = True
    _active_threads.append(thread)
    thread.start()
    return thread

def wait_for_background_tasks():
    """
    Wait for all registered background threads to finish.
    Useful at the end of the CLI lifecycle to ensure data like ChromaDB embeddings is written.
    """
    if not _active_threads:
        return

    try:
        from rich.console import Console
        console = Console()
        console.print(f"[bold yellow][INFO] 正在同步本地记忆库 (共有 {len(_active_threads)} 个后台任务)，请稍候...[/bold yellow]")
    except ImportError:
        print(f"[INFO] 正在同步本地记忆库 (共有 {len(_active_threads)} 个后台任务)，请稍候...")

    for thread in _active_threads:
        if thread.is_alive():
            thread.join()

    _active_threads.clear()

    try:
        from rich.console import Console
        Console().print("[bold green][✓] 所有后台任务同步完毕，系统安全退出。[/bold green]")
    except ImportError:
        print("[✓] 所有后台任务同步完毕，系统安全退出。")