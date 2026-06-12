#!/usr/bin/env python3
from __future__ import annotations

"""
本地文档解析与检索流水线。

职责：
1) 在本地解析 doc/docx/pdf 为 markdown；
2) 当发现图片时调用服务端 OCR 接口并回填到 markdown；
3) 按阈值决定是否拆分；
4) 执行分层检索（先定位章节 -> 再在章节内检索细节 -> 可选全局搜索）；
5) 产出 chunks_json / retrieval_hits_json 以及 artifacts 文件。
"""

import hashlib
import json
import mimetypes
import os
import re
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path
from typing import Any, Dict, List, Sequence

import fitz
import olefile
from docx import Document
from docx.opc.exceptions import PackageNotFoundError


# 小文档阈值：小于等于该值时直接整文提交，不走召回策略。
TOKEN_THRESHOLD = 10000
# 检索入口切换阈值：超过该字符数优先按章节 chunks 检索。
SOURCE_TO_CHUNK_THRESHOLD = 50000

# 默认候选关键词。真实执行时会与用户指令关键词合并。
DEFAULT_KEYWORDS = [
    "项目名称",
    "招标编号",
    "保证金",
    "支付方式",
    "支付账号",
    "银行账号",
    "开户行",
    "一票否决",
    "无效投标",
    "不予受理",
    "废标情形",
    "否决投标",
    "投标截止时间",
    "开标时间",
    "投标地点",
    "企业资质",
    "人员资质",
    "技术参数",
    "商务条款",
    "评分标准",
    "分值占比",
    "权重",
    "标书结构",
    "密封要求",
    "法务",
    "BC",
]

KEY_INFO_CATEGORY_CONFIG = [
    {"name": "项目名称", "keywords": ["项目名称", "项目概况", "采购项目", "招标项目"]},
    {"name": "招标编号", "keywords": ["招标编号", "项目编号", "采购编号", "文件编号"]},
    {"name": "保证金信息", "keywords": ["保证金", "投标担保", "金额", "支付方式", "支付账号", "银行账号", "开户行", "户名"]},
    {"name": "废标项", "keywords": ["一票否决", "无效投标", "不予受理", "废标情形", "否决投标", "废标"]},
    {"name": "投标截止时间", "keywords": ["投标截止时间", "递交截止时间", "投标截止", "截止时间"]},
    {"name": "开标时间", "keywords": ["开标时间", "开标日期", "唱标时间"]},
    {"name": "投标地点", "keywords": ["投标地点", "递交地点", "开标地点", "提交地点"]},
    {"name": "企业资质要求", "keywords": ["企业资质", "营业执照", "资质证书", "资格条件"]},
    {"name": "人员资质要求", "keywords": ["项目经理", "人员要求", "执业资格", "证书", "团队成员"]},
    {"name": "技术参数要求", "keywords": ["技术参数", "技术要求", "技术指标", "性能要求"]},
    {"name": "商务条款", "keywords": ["商务条款", "付款条件", "履约", "违约责任", "合同条款"]},
    {"name": "评分标准", "keywords": ["评分标准", "评审标准", "分值", "评分办法", "权重", "分值占比"]},
    {"name": "标书结构要求", "keywords": ["投标文件组成", "编制要求", "目录", "格式要求", "装订要求"]},
    {"name": "密封要求", "keywords": ["密封", "封装", "封袋", "签字盖章", "骑缝章"]},
    {"name": "法务及BC重点关注信息", "keywords": ["合规", "法律责任", "处罚", "黑名单", "BC", "反商业贿赂", "廉洁"]},
]


OCR_TEXT_KEYS = (
    "text",
    "rec_text",
    "rec_texts",
    "recTexts",
    "ocr_text",
    "content",
)

JSON_REQUIRED_ENDPOINTS: set[str] = set()
OPENAPI_TIMEOUT_SEC = 15
CONTRACT_UNKNOWN = "unknown"
CONTRACT_JSON = "application/json"
CONTRACT_MULTIPART = "multipart/form-data"
_OPENAPI_CONTRACT_CACHE: Dict[str, Dict[str, str]] = {}
MANIFEST_FILE_NAME = "manifest.json"
CHUNKS_JSON_FILE_NAME = "chunks.json"
PREPARED_PAYLOAD_FILE_NAME = "prepared_payload.json"
QUERY_ROUTE_FILE_NAME = "query_route.json"
OCR_MAX_RETRIES = 2
OCR_RETRY_DELAY_SEC = 1.0
# Skill 内统一配置文件名：用于集中维护远程 OCR 服务地址，避免散落在各项目 .env 中。
SKILL_ENV_FILENAME = ".skill.env"
# 明确禁止的本地地址集合：命中这些 host 说明并未走远程服务链路。
LOCAL_ONLY_HOSTS = {"127.0.0.1", "localhost", "0.0.0.0", "::", "::1"}

# .doc 解析时优先读取的 OLE 文本流顺序。
# 说明：WordDocument 通常承载正文；其余流常包含结构/目录/样式元数据，
# 如不做筛选直接拼接会把噪声写入 source.md，导致乱码污染检索证据。
DOC_STREAM_PRIORITY = ("WordDocument", "1Table", "0Table", "Data")

# legacy .doc 的候选解码顺序。
# 说明：不同文档历史编码不一致，按优先级尝试后由质量评分选最优结果，
# 避免把多个编码结果混合（这是乱码放大的根因之一）。
DOC_CANDIDATE_ENCODINGS = ("gb18030", "utf-16le", "utf-8")

# doc 文本片段提取正则：保留中文、英文、数字及常见中文标点。
DOC_FRAGMENT_PATTERN = re.compile(r"[\u4e00-\u9fffA-Za-z0-9，。；：、“”‘’（）()【】\[\]、/\-_%]{2,}")

# 典型元数据噪声标记（目录锚点、OOXML路径、域代码等）。
DOC_NOISE_MARKERS = (
    "_toc",
    "hyperlink",
    "pageref",
    "[content_types]",
    "word/_rels",
    "drs/",
    ".xml",
    "_rels",
)

# 评分关键词：用于判断片段是否更像招投标正文语义。
DOC_HINT_KEYWORDS = (
    "招标",
    "投标",
    "项目",
    "文件",
    "标包",
    "采购",
    "合同",
    "保证金",
    "履约",
    "评标",
    "评分",
    "资格",
    "资质",
    "承诺",
    "供应商",
    "开标",
    "截止",
    "地点",
    "时间",
    "招标人",
    "投标人",
    "代理",
    "编号",
    "目录",
    "封面",
    "法定代表人",
    "营业执照",
    "证书",
    "条款",
    "格式",
    "附件",
    "金额",
    "报价",
    "分册",
    "须知",
    "一览表",
    "偏离表",
)

# 常见中文功能字：用于区分“自然中文语句”与“随机解码噪声”。
# 说明：乱码片段通常几乎不含这些高频字。
DOC_COMMON_CHARS = set("的一是在和有为不本及与或由将对按于须应可等中人年月日时分号")

DOC_TOC_PATTERN = re.compile(r"^_Toc\d+$", flags=re.IGNORECASE)
DOC_ASCII_METADATA_PATTERN = re.compile(r"^[A-Za-z0-9_./\\:-]{2,128}$")
DOC_REPEAT_PATTERN = re.compile(r"^([\u4e00-\u9fff]{1,2})\1{1,}$")
# 目录行常见页码尾缀（例如“（P2200）”）：仅清理“尾缀定位噪声”，尽量保留正文描述。
TOC_PAGE_BRACKET_SUFFIX_PATTERN = re.compile(r"\s*[（(]\s*[Pp]\s*\d{1,6}\s*[)）]\s*$")
# 目录点线+页码（例如“……2200”“.....2200”）：在目录扫描件中非常常见。
TOC_DOT_PAGE_SUFFIX_PATTERN = re.compile(r"\s*[.。·…_]{2,}\s*\d{1,6}\s*$")
# 单独的页码标记行（例如“P2200”“(P2201)”）：这类行不应进入条款候选。
TOC_PAGE_ONLY_PATTERN = re.compile(r"^[（(]?\s*[Pp]\s*\d{1,6}\s*[)）]?$")


@dataclass
class ParseOutput:
    """文档解析输出。"""

    markdown_text: str
    units: List[Dict[str, Any]]
    image_count: int
    document_type: str


def safe_slug(text: str) -> str:
    """将名称转换为安全目录名。"""
    normalized = re.sub(r"[^0-9A-Za-z\u4e00-\u9fff._-]+", "_", text.strip())
    normalized = normalized.strip("._")
    return normalized or "unknown"


def now_stamp() -> str:
    """生成时间戳目录名。"""
    return datetime.now().strftime("%Y%m%d_%H%M%S_%f")


def estimate_tokens(text: str) -> int:
    """
    token 估算函数。

    按当前需求使用字符长度作为估算值，避免引入模型侧 tokenizer 依赖。
    """
    return len(text or "")


def _normalize_resolved_path(file_path: Path) -> str:
    """标准化绝对路径字符串。"""
    return str(file_path.expanduser().resolve())


def build_file_fingerprint(file_path: Path) -> Dict[str, Any]:
    """构建文件指纹，用于解析结果复用校验。"""
    resolved = file_path.expanduser().resolve()
    stat = resolved.stat()
    hasher = hashlib.sha256()
    with resolved.open("rb") as handler:
        while True:
            chunk = handler.read(1024 * 1024)
            if not chunk:
                break
            hasher.update(chunk)
    return {
        "path": _normalize_resolved_path(resolved),
        "size_bytes": int(stat.st_size),
        "mtime_ns": int(getattr(stat, "st_mtime_ns", int(stat.st_mtime * 1_000_000_000))),
        "sha256": hasher.hexdigest(),
    }


def _manifest_matches_file(manifest: Dict[str, Any], current_fingerprint: Dict[str, Any]) -> bool:
    """判断 manifest 是否匹配当前文件。"""
    manifest_fingerprint = manifest.get("file_fingerprint")
    if isinstance(manifest_fingerprint, dict):
        if str(manifest_fingerprint.get("sha256", "")).strip() != current_fingerprint["sha256"]:
            return False
        if int(manifest_fingerprint.get("size_bytes", -1)) != int(current_fingerprint["size_bytes"]):
            return False
        return True

    manifest_file_path = str(manifest.get("file_path", "")).strip()
    if not manifest_file_path:
        return False
    return _normalize_resolved_path(Path(manifest_file_path)).lower() == str(current_fingerprint["path"]).lower()


def _safe_read_json(path: Path) -> Dict[str, Any] | List[Any] | None:
    """读取 JSON 文件，失败时返回 None。"""
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except Exception:  # noqa: BLE001
        return None


def _parse_chunk_markdown(chunk_md_path: Path) -> Dict[str, Any] | None:
    """
    解析 chunk markdown（兼容历史产物）。

    约定格式来自 _save_chunks_markdown：
    # chunk_0001
    - 章节: xxx
    - 小节: xxx

    正文...
    """
    lines = chunk_md_path.read_text(encoding="utf-8").splitlines()
    if len(lines) < 5:
        return None
    chunk_id = lines[0].replace("#", "").strip() or chunk_md_path.stem

    chapter = "未分章"
    section = "未分节"
    for line in lines[1:4]:
        if line.startswith("- 章节:"):
            chapter = line.replace("- 章节:", "").strip() or chapter
        elif line.startswith("- 小节:"):
            section = line.replace("- 小节:", "").strip() or section

    content = "\n".join(lines[5:]).strip()
    if not content:
        return None
    return {
        "chunk_id": chunk_id,
        "chapter": chapter,
        "section": section,
        "content": content,
    }


def _load_chunks_from_artifact_dir(artifact_dir: Path) -> List[Dict[str, Any]]:
    """优先加载 chunks.json，兼容回退到 chunks/*.md。"""
    chunks_json_path = artifact_dir / CHUNKS_JSON_FILE_NAME
    if chunks_json_path.exists():
        payload = _safe_read_json(chunks_json_path)
        if isinstance(payload, list):
            normalized = [item for item in payload if isinstance(item, dict)]
            if normalized:
                return normalized

    chunk_dir = artifact_dir / "chunks"
    if not chunk_dir.exists():
        return []
    chunk_files = sorted([path for path in chunk_dir.iterdir() if path.is_file() and path.suffix.lower() == ".md"])
    parsed_chunks: List[Dict[str, Any]] = []
    for chunk_file in chunk_files:
        chunk = _parse_chunk_markdown(chunk_file)
        if chunk:
            parsed_chunks.append(chunk)
    return parsed_chunks


def _build_retrieval_result(
    chunks: Sequence[Dict[str, Any]],
    token_estimate: int,
    instruction: str | None,
    enable_global_search: bool,
    custom_keywords: Sequence[str] | None,
) -> tuple[List[Dict[str, Any]], bool, str]:
    """基于 chunks 计算检索命中与策略。"""
    retrieval_hits: List[Dict[str, Any]] = []
    need_global = False
    strategy = "single_direct_llm"

    if token_estimate > TOKEN_THRESHOLD:
        strategy = "chapter_scope_then_keyword_recall"
        keywords = build_keywords(instruction=instruction, custom_keywords=custom_keywords)
        # 先做章节定位，再在定位章节中检索细节。
        if not retrieval_hits:
            retrieval_hits = chapter_scoped_search(chunks=chunks, keywords=keywords)
        # 若章节定位仍无结果，再回退到全局关键词召回。
        if not retrieval_hits:
            retrieval_hits = keyword_recall(chunks=chunks, keywords=keywords)
        if not retrieval_hits:
            if enable_global_search:
                strategy = "global_search"
                retrieval_hits = global_search(chunks=chunks, keywords=keywords)
            else:
                need_global = True
                strategy = "wait_global_confirm"

    return retrieval_hits, need_global, strategy


def _select_preferred_search_target(source_char_count: int) -> tuple[str, str]:
    """根据 source 文本长度建议检索入口。"""
    if source_char_count > SOURCE_TO_CHUNK_THRESHOLD:
        return (
            "chunks",
            f"source_char_count={source_char_count} > threshold={SOURCE_TO_CHUNK_THRESHOLD}",
        )
    return (
        "source",
        f"source_char_count={source_char_count} <= threshold={SOURCE_TO_CHUNK_THRESHOLD}",
    )


def _build_query_route_payload(
    *,
    artifact_dir: Path,
    source_char_count: int,
    preferred_search_target: str,
    preferred_reason: str,
    generated_at: str,
) -> Dict[str, Any]:
    """构建独立检索路由文件内容。"""
    return {
        "mode": preferred_search_target,
        "source_char_count": source_char_count,
        "threshold": SOURCE_TO_CHUNK_THRESHOLD,
        "preferred_search_target": preferred_search_target,
        "preferred_reason": preferred_reason,
        "source_md_path": str(artifact_dir / "source.md"),
        "chunks_json_path": str(artifact_dir / CHUNKS_JSON_FILE_NAME),
        "chunk_dir_path": str(artifact_dir / "chunks"),
        "generated_at": generated_at,
    }


def _write_query_route_file(artifact_dir: Path, route_payload: Dict[str, Any]) -> Path:
    """写入 query_route.json。"""
    route_file = artifact_dir / QUERY_ROUTE_FILE_NAME
    route_file.write_text(json.dumps(route_payload, ensure_ascii=False, indent=2), encoding="utf-8")
    return route_file


def _resolve_source_char_count(artifact_dir: Path, fallback: int = 0) -> int:
    """读取 source.md 字符数，失败时回退。"""
    source_md_path = artifact_dir / "source.md"
    if source_md_path.exists():
        try:
            return len(source_md_path.read_text(encoding="utf-8"))
        except Exception:  # noqa: BLE001
            pass
    return max(0, int(fallback))


def _find_cached_artifact_dir(
    file_path: Path,
    skill_root: Path,
    role: str,
    suffix: str,
    current_fingerprint: Dict[str, Any],
) -> Path | None:
    """按最新时间倒序查找匹配文件的缓存目录。"""
    project_slug = safe_slug(file_path.stem)
    role_root = skill_root / "artifacts" / project_slug / role / suffix
    if not role_root.exists():
        return None

    run_dirs = sorted([path for path in role_root.iterdir() if path.is_dir()], key=lambda p: p.name, reverse=True)
    for run_dir in run_dirs:
        manifest_path = run_dir / MANIFEST_FILE_NAME
        if not manifest_path.exists():
            continue
        manifest_payload = _safe_read_json(manifest_path)
        if not isinstance(manifest_payload, dict):
            continue
        if _manifest_matches_file(manifest_payload, current_fingerprint):
            return run_dir
    return None


def _load_cached_payload(
    file_path: Path,
    skill_root: Path,
    role: str,
    instruction: str | None,
    enable_global_search: bool,
    custom_keywords: Sequence[str] | None,
) -> Dict[str, Any] | None:
    """加载并复用已解析产物。"""
    suffix = file_path.suffix.lower().lstrip(".")
    current_fingerprint = build_file_fingerprint(file_path=file_path)
    artifact_dir = _find_cached_artifact_dir(
        file_path=file_path,
        skill_root=skill_root,
        role=role,
        suffix=suffix,
        current_fingerprint=current_fingerprint,
    )
    if not artifact_dir:
        return None

    manifest_path = artifact_dir / MANIFEST_FILE_NAME
    manifest_payload = _safe_read_json(manifest_path)
    if not isinstance(manifest_payload, dict):
        return None

    chunks = _load_chunks_from_artifact_dir(artifact_dir=artifact_dir)
    if not chunks:
        return None

    token_estimate = int(manifest_payload.get("token_estimate") or 0)
    if token_estimate <= 0:
        token_estimate = sum(len(str(item.get("content", ""))) for item in chunks)
    source_char_count = int(manifest_payload.get("source_char_count") or 0)
    if source_char_count <= 0:
        source_char_count = _resolve_source_char_count(artifact_dir=artifact_dir, fallback=token_estimate)
    preferred_search_target, preferred_reason = _select_preferred_search_target(source_char_count)
    route_payload = _build_query_route_payload(
        artifact_dir=artifact_dir,
        source_char_count=source_char_count,
        preferred_search_target=preferred_search_target,
        preferred_reason=preferred_reason,
        generated_at=now_stamp(),
    )
    route_file = _write_query_route_file(artifact_dir=artifact_dir, route_payload=route_payload)
    retrieval_hits, need_global, strategy = _build_retrieval_result(
        chunks=chunks,
        token_estimate=token_estimate,
        instruction=instruction,
        enable_global_search=enable_global_search,
        custom_keywords=custom_keywords,
    )

    merged_manifest = dict(manifest_payload)
    merged_manifest["file_fingerprint"] = current_fingerprint
    merged_manifest["cache_hit"] = True
    merged_manifest["reuse_checked_at"] = now_stamp()
    merged_manifest["token_estimate"] = token_estimate
    merged_manifest["source_char_count"] = source_char_count
    merged_manifest["source_to_chunk_threshold"] = SOURCE_TO_CHUNK_THRESHOLD
    merged_manifest["preferred_search_target"] = preferred_search_target
    merged_manifest["preferred_reason"] = preferred_reason
    merged_manifest["chunk_split_threshold"] = SOURCE_TO_CHUNK_THRESHOLD
    merged_manifest["query_route_file"] = str(route_file)
    merged_manifest["strategy"] = strategy
    merged_manifest["need_global_search_confirmation"] = need_global
    merged_manifest["retrieval_hit_count"] = len(retrieval_hits)
    merged_manifest["reused_from_artifact_dir"] = str(artifact_dir)

    # 兼容历史产物：首次命中旧缓存时补写可复用结构化文件。
    chunks_json_path = artifact_dir / CHUNKS_JSON_FILE_NAME
    if not chunks_json_path.exists():
        chunks_json_path.write_text(json.dumps(chunks, ensure_ascii=False, indent=2), encoding="utf-8")
    (artifact_dir / MANIFEST_FILE_NAME).write_text(json.dumps(merged_manifest, ensure_ascii=False, indent=2), encoding="utf-8")
    (artifact_dir / PREPARED_PAYLOAD_FILE_NAME).write_text(
        json.dumps(
            {
                "document_name": file_path.name,
                "document_type": str(manifest_payload.get("document_type", suffix)).strip() or suffix,
                "token_estimate": token_estimate,
                "source_char_count": source_char_count,
                "source_to_chunk_threshold": SOURCE_TO_CHUNK_THRESHOLD,
                "preferred_search_target": preferred_search_target,
                "preferred_reason": preferred_reason,
                "query_route_file": str(route_file),
                "query_route": route_payload,
                "strategy": strategy,
                "chunks_json": chunks,
                "retrieval_hits_json": retrieval_hits,
                "need_global_search_confirmation": need_global,
                "file_fingerprint": current_fingerprint,
                "cache_hit": True,
            },
            ensure_ascii=False,
            indent=2,
        ),
        encoding="utf-8",
    )

    return {
        "artifact_dir": str(artifact_dir),
        "document_name": file_path.name,
        "document_type": str(manifest_payload.get("document_type", suffix)).strip() or suffix,
        "token_estimate": token_estimate,
        "source_char_count": source_char_count,
        "source_to_chunk_threshold": SOURCE_TO_CHUNK_THRESHOLD,
        "preferred_search_target": preferred_search_target,
        "preferred_reason": preferred_reason,
        "query_route_file": str(route_file),
        "query_route": route_payload,
        "strategy": strategy,
        "chunks_json": chunks,
        "retrieval_hits_json": retrieval_hits,
        "need_global_search_confirmation": need_global,
        "suggested_question": "当前未命中有效片段，是否执行全局搜索？",
        "manifest": merged_manifest,
        "cache_hit": True,
        # 明确标注本次是否请求强制刷新（命中缓存路径下固定为 False）。
        "force_refresh_requested": False,
        # 命中缓存路径说明并未跳过缓存读取。
        "cache_lookup_skipped": False,
    }


def _read_dotenv_file(env_path: Path) -> Dict[str, str]:
    """读取简单 .env 文件（仅 key=value）。"""
    values: Dict[str, str] = {}
    if not env_path.exists():
        return values
    for raw_line in env_path.read_text(encoding="utf-8").splitlines():
        line = raw_line.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        key, value = line.split("=", 1)
        values[key.strip()] = value.strip()
    return values


def _runtime_env_value(
    key: str,
    default: str = "",
    dotenv_candidates: Sequence[Path] | None = None,
    prefer_dotenv: bool = False,
) -> str:
    """按优先级读取运行时配置：系统环境变量 > 候选 .env 文件。"""
    candidates: List[Path] = []
    seen: set[str] = set()
    for item in list(dotenv_candidates or []) + [Path.cwd() / ".env"]:
        normalized = str(item.resolve())
        if normalized in seen:
            continue
        seen.add(normalized)
        candidates.append(item)

    def _from_dotenv() -> str:
        for env_path in candidates:
            value = _read_dotenv_file(env_path).get(key)
            if value is not None and value != "":
                return value
        return ""

    if prefer_dotenv:
        dotenv_value = _from_dotenv()
        if dotenv_value:
            return dotenv_value

    current = os.getenv(key)
    if current is not None and current != "":
        return current

    if not prefer_dotenv:
        dotenv_value = _from_dotenv()
        if dotenv_value:
            return dotenv_value

    return default


def _build_api_base_env_candidates(dotenv_candidates: Sequence[Path] | None = None) -> List[Path]:
    """
    构建 API 地址读取候选 .env 列表（按优先级）。

    优先级设计：
    1) Skill 目录下的 .skill.env（便于在技能内部统一维护地址）；
    2) 调用方传入的候选 .env（兼容历史脚本调用）；
    3) 其余候选由 _runtime_env_value() 自动追加（如当前工作目录 .env）。
    """
    skill_root = Path(__file__).resolve().parents[1]
    skill_env = skill_root / SKILL_ENV_FILENAME
    candidates: List[Path] = [skill_env]
    candidates.extend(list(dotenv_candidates or []))
    return candidates


def _normalize_api_base(api_base: str) -> str:
    """
    规范化 API Base URL。

    处理规则：
    - 未带协议时默认补全为 http://；
    - 仅允许 http/https；
    - 保留可选路径，去除末尾多余斜杠，确保后续拼接 endpoint 时行为稳定。
    """
    normalized_input = api_base.strip()
    if not re.match(r"^https?://", normalized_input, flags=re.IGNORECASE):
        normalized_input = f"http://{normalized_input}"

    parsed = urllib.parse.urlsplit(normalized_input)
    scheme = parsed.scheme.lower()
    if scheme not in {"http", "https"}:
        raise ValueError(f"仅支持 http/https 协议，当前为: {parsed.scheme or '空'}")

    host = (parsed.hostname or "").strip().lower()
    if not host:
        raise ValueError("服务地址缺少主机名（host），请检查 BID_API_BASE 配置。")

    try:
        port_part = f":{parsed.port}" if parsed.port else ""
    except ValueError as exc:
        raise ValueError(f"服务地址端口非法: {normalized_input}") from exc

    normalized = f"{scheme}://{host}{port_part}"
    if parsed.path and parsed.path != "/":
        normalized = f"{normalized}{parsed.path.rstrip('/')}"
    return normalized.rstrip("/")


def resolve_api_base(raw_api_base: str | None = None, dotenv_candidates: Sequence[Path] | None = None) -> str:
    """
    解析服务端地址。

    规则：
    1. CLI --api-base（若传入）；
    2. Skill 内 .skill.env 的 BID_API_BASE（推荐，便于集中维护）；
    3. 运行时环境变量/其他 .env 的 BID_API_BASE（兼容）；
    4. 不再回退本地地址，缺失时直接报错。
    """
    api_base = (raw_api_base or "").strip()
    if not api_base:
        api_base = _runtime_env_value(
            "BID_API_BASE",
            "",
            dotenv_candidates=_build_api_base_env_candidates(dotenv_candidates),
            # 这里显式优先读取 .env（尤其是 skill/.skill.env），方便用户在 Skill 内直改地址。
            prefer_dotenv=True,
        ).strip()

    if not api_base:
        skill_env_path = Path(__file__).resolve().parents[1] / SKILL_ENV_FILENAME
        raise ValueError(
            "未配置远程服务地址。请在 "
            f"{skill_env_path} 中设置 BID_API_BASE，"
            "或在命令中显式传入 --api-base。"
        )

    normalized = _normalize_api_base(api_base)
    parsed = urllib.parse.urlsplit(normalized)
    host = (parsed.hostname or "").strip().lower()
    # 强制远程：禁止回落到任何本地监听地址，避免误连 127.0.0.1:5000。
    if host in LOCAL_ONLY_HOSTS:
        raise ValueError(
            f"检测到本地地址 {normalized}。Skill 仅允许远程服务地址，"
            "请改为远程 BID_API_BASE（例如 http://10.136.0.123:10104）。"
        )
    return normalized.rstrip("/")


def _normalize_endpoint_path(endpoint: str) -> str:
    """标准化接口路径，保证以 / 开头。"""
    endpoint_path = endpoint.strip()
    if not endpoint_path.startswith("/"):
        endpoint_path = f"/{endpoint_path}"
    return endpoint_path


def _detect_request_contract(api_base: str, endpoint: str) -> str:
    """
    从 openapi 读取接口 requestBody 的 content-type。

    返回值示例：
    - application/json
    - multipart/form-data
    - unknown
    """
    normalized_endpoint = _normalize_endpoint_path(endpoint)
    cache_key = api_base.rstrip("/")
    endpoint_cache = _OPENAPI_CONTRACT_CACHE.setdefault(cache_key, {})
    cached = endpoint_cache.get(normalized_endpoint)
    if cached:
        return cached

    contract = CONTRACT_UNKNOWN
    openapi_url = f"{cache_key}/openapi.json"
    try:
        req = urllib.request.Request(url=openapi_url, method="GET")
        with urllib.request.urlopen(req, timeout=OPENAPI_TIMEOUT_SEC) as resp:
            raw = resp.read().decode("utf-8", errors="replace")
        openapi_payload = json.loads(raw)
        endpoint_info = ((openapi_payload.get("paths") or {}).get(normalized_endpoint) or {}).get("post") or {}
        content = ((endpoint_info.get("requestBody") or {}).get("content") or {})
        content_types = [str(key).strip() for key in content.keys() if str(key).strip()]
        if CONTRACT_JSON in content_types:
            contract = CONTRACT_JSON
        elif CONTRACT_MULTIPART in content_types:
            contract = CONTRACT_MULTIPART
        elif content_types:
            contract = ",".join(content_types)
    except Exception:  # noqa: BLE001
        contract = CONTRACT_UNKNOWN

    endpoint_cache[normalized_endpoint] = contract
    return contract


def _iter_ocr_text_fragments(value: Any) -> List[str]:
    """递归提取 OCR 结果里的文本字段。"""
    fragments: List[str] = []
    if isinstance(value, dict):
        for key in OCR_TEXT_KEYS:
            current = value.get(key)
            if isinstance(current, str) and current.strip():
                fragments.append(current.strip())
            elif isinstance(current, list):
                for item in current:
                    if isinstance(item, str) and item.strip():
                        fragments.append(item.strip())
        for nested in value.values():
            fragments.extend(_iter_ocr_text_fragments(nested))
    elif isinstance(value, list):
        for nested in value:
            fragments.extend(_iter_ocr_text_fragments(nested))
    return fragments


def normalize_ocr_text(raw_value: Any) -> str:
    """
    规范 OCR 返回值，只输出纯文本。

    兼容场景：
    - 正常字符串；
    - JSON 字符串（例如 '{"text":"..."}'）；
    - 字典/数组结构。
    """
    if isinstance(raw_value, str):
        text = raw_value.strip()
        if not text:
            return ""
        if text.startswith("{") or text.startswith("["):
            try:
                parsed = json.loads(text)
            except Exception:  # noqa: BLE001
                return text
            parts = _iter_ocr_text_fragments(parsed)
            if parts:
                return "\n".join(list(dict.fromkeys(parts))).strip()
            return text
        return text

    if isinstance(raw_value, (dict, list)):
        parts = _iter_ocr_text_fragments(raw_value)
        if parts:
            return "\n".join(list(dict.fromkeys(parts))).strip()
        return ""

    return str(raw_value or "").strip()


def _build_multipart_body(file_path: Path, field_name: str = "image") -> tuple[bytes, str]:
    """构造 multipart/form-data 请求体。"""
    boundary = f"----BidOcrBoundary{uuid.uuid4().hex}"
    mime_type = mimetypes.guess_type(file_path.name)[0] or "application/octet-stream"
    body = bytearray()
    body.extend(f"--{boundary}\r\n".encode("utf-8"))
    body.extend(
        (
            f'Content-Disposition: form-data; name="{field_name}"; filename="{file_path.name}"\r\n'
            f"Content-Type: {mime_type}\r\n\r\n"
        ).encode("utf-8")
    )
    body.extend(file_path.read_bytes())
    body.extend(b"\r\n")
    body.extend(f"--{boundary}--\r\n".encode("utf-8"))
    return bytes(body), boundary


def _extension_from_content_type(content_type: str, fallback: str = ".png") -> str:
    """根据 content-type 获取文件后缀。"""
    mapping = {
        "image/jpeg": ".jpg",
        "image/jpg": ".jpg",
        "image/png": ".png",
        "image/gif": ".gif",
        "image/webp": ".webp",
        "image/bmp": ".bmp",
        "image/tiff": ".tiff",
        "image/tif": ".tif",
    }
    return mapping.get(content_type.lower(), fallback)


def normalize_image_for_ocr(
    image_bytes: bytes,
    content_type: str,
    output_prefix: Path,
) -> Path:
    """
    归一化图片文件后再发 OCR。

    主要用于处理 tiff 等格式，统一转成 png，减少上游解析失败概率。
    """
    ext = _extension_from_content_type(content_type=content_type, fallback=".png")
    if ext in {".tif", ".tiff"}:
        try:
            doc = fitz.open(stream=image_bytes, filetype="tiff")
            page = doc.load_page(0)
            pixmap = page.get_pixmap(alpha=False)
            normalized_path = output_prefix.with_suffix(".png")
            pixmap.save(str(normalized_path))
            doc.close()
            return normalized_path
        except Exception as exc:  # noqa: BLE001
            raise RuntimeError(f"TIFF图片归一化失败: {exc}") from exc

    normalized_path = output_prefix.with_suffix(ext)
    normalized_path.write_bytes(image_bytes)
    return normalized_path


def _append_ocr_failure_log(
    *,
    log_dir: Path,
    image_path: str,
    error: str,
    stage: str,
    api_base: str,
    attempt: int,
    total_attempts: int,
) -> None:
    """记录 OCR 失败日志；任何日志异常都不影响主流程。"""
    log_path = log_dir / "ocr_failures.log"
    payload = {
        "timestamp": datetime.now().isoformat(timespec="seconds"),
        "stage": stage,
        "image_path": image_path,
        "api_base": api_base,
        "attempt": attempt,
        "total_attempts": total_attempts,
        "error": error,
    }
    try:
        log_dir.mkdir(parents=True, exist_ok=True)
        with log_path.open("a", encoding="utf-8") as handler:
            handler.write(json.dumps(payload, ensure_ascii=False) + "\n")
    except Exception:  # noqa: BLE001
        return


def call_server_ocr(
    api_base: str,
    image_path: Path,
    timeout_sec: int = 600,
    max_retries: int = OCR_MAX_RETRIES,
    retry_delay_sec: float = OCR_RETRY_DELAY_SEC,
) -> str:
    """
    调用服务端 OCR 接口。

    失败时执行有限重试，最终失败仅记录日志并返回空字符串，不中断主流程。
    """
    body, boundary = _build_multipart_body(file_path=image_path, field_name="image")
    request_url = f"{api_base.rstrip('/')}/api/bid/ocr/image"
    total_attempts = max(1, int(max_retries) + 1)
    last_error = ""

    for attempt in range(1, total_attempts + 1):
        req = urllib.request.Request(
            url=request_url,
            data=body,
            method="POST",
        )
        req.add_header("Content-Type", f"multipart/form-data; boundary={boundary}")
        req.add_header("Content-Length", str(len(body)))
        try:
            with urllib.request.urlopen(req, timeout=timeout_sec) as resp:
                payload = json.loads(resp.read().decode("utf-8"))
            if payload.get("code") == 0:
                return normalize_ocr_text(payload.get("data", {}).get("text", ""))
            last_error = f"OCR接口返回失败: {json.dumps(payload, ensure_ascii=False)}"
        except urllib.error.HTTPError as exc:
            detail = exc.read().decode("utf-8", errors="replace")
            last_error = f"OCR接口失败[{image_path}] HTTP {exc.code}: {detail}"
        except Exception as exc:  # noqa: BLE001
            last_error = f"OCR接口调用失败[{image_path}]: {exc}"

        if attempt < total_attempts:
            if retry_delay_sec > 0:
                time.sleep(retry_delay_sec)
            continue

        _append_ocr_failure_log(
            log_dir=image_path.parent,
            image_path=str(image_path),
            error=last_error or "OCR未知错误",
            stage="ocr_request",
            api_base=api_base,
            attempt=attempt,
            total_attempts=total_attempts,
        )
        print(
            f"[OCR警告] 识别失败并已跳过: {image_path}，详情见 {image_path.parent / 'ocr_failures.log'}",
            file=sys.stderr,
        )
        return ""

    return ""


def _extract_docx(
    file_path: Path,
    image_dir: Path,
    api_base: str,
) -> ParseOutput:
    """解析 docx，提取文本并对图片走服务端 OCR。"""
    try:
        doc = Document(str(file_path))
    except PackageNotFoundError as exc:
        raise RuntimeError("文件已加密，请解密后重新上传") from exc
    except Exception as exc:  # noqa: BLE001
        raise RuntimeError("文件损坏，无法解析，请检查文件后重新上传") from exc

    from docx.oxml.ns import qn as _qn
    from docx.table import Table as DocxTable
    from docx.text.paragraph import Paragraph

    lines: List[str] = []
    units: List[Dict[str, Any]] = []
    chapter = "全文"
    section = "全文"
    image_count = 0
    ocr_cache: Dict[str, str] = {}

    def _collect_image_rids(element: Any) -> List[str]:
        rids: List[str] = []
        for blip in element.findall(".//" + _qn("a:blip")):
            rid = blip.get(_qn("r:embed"))
            if rid:
                rids.append(rid)
        return list(dict.fromkeys(rids))

    def _append_image_ocr_by_rids(rids: Sequence[str], section_name: str) -> None:
        nonlocal image_count
        for rid in rids:
            rel = doc.part.rels.get(rid)
            if rel is None or "image" not in rel.reltype:
                continue
            image_bytes = rel.target_part.blob
            content_type = str(rel.target_part.content_type or "image/png")
            image_hash = hashlib.sha256(image_bytes).hexdigest()
            image_count += 1
            output_prefix = image_dir / f"docx_image_{image_count:03d}"
            try:
                image_path = normalize_image_for_ocr(
                    image_bytes=image_bytes,
                    content_type=content_type,
                    output_prefix=output_prefix,
                )
            except Exception as exc:  # noqa: BLE001
                _append_ocr_failure_log(
                    log_dir=image_dir,
                    image_path=str(output_prefix),
                    error=f"图片归一化失败: {exc}",
                    stage="normalize",
                    api_base=api_base,
                    attempt=1,
                    total_attempts=1,
                )
                print(
                    f"[OCR警告] 图片归一化失败并已跳过: {output_prefix}",
                    file=sys.stderr,
                )
                continue
            if image_hash in ocr_cache:
                ocr_text = ocr_cache[image_hash]
            else:
                ocr_text = _sanitize_business_text_block(call_server_ocr(api_base=api_base, image_path=image_path))
                if ocr_text:
                    ocr_cache[image_hash] = ocr_text
            lines.append(f"![docx_image_{image_count}]({image_path.as_posix()})")
            if ocr_text:
                lines.append(f"> OCR结果: {ocr_text}")
                units.append(
                    {
                        "chapter": chapter,
                        "section": f"{section_name}_图片OCR",
                        "content": ocr_text,
                    }
                )
            else:
                lines.append("> OCR结果: ")

    for child in doc.element.body:
        tag = child.tag
        if tag == _qn("w:p"):
            para = Paragraph(child, doc)
            text = _sanitize_business_line(para.text or "")
            if text:
                style_name = (para.style.name or "") if para.style else ""
                if "Heading 1" in style_name or "标题 1" in style_name:
                    chapter = text
                    section = text
                    lines.append(f"# {text}")
                elif "Heading 2" in style_name or "标题 2" in style_name:
                    section = text
                    lines.append(f"## {text}")
                else:
                    lines.append(text)
                    units.append({"chapter": chapter, "section": section, "content": text})
            para_rids = _collect_image_rids(child)
            if para_rids:
                _append_image_ocr_by_rids(para_rids, section)
        elif tag == _qn("w:tbl"):
            table = DocxTable(child, doc)
            for row in table.rows:
                cleaned_cells: List[str] = []
                for cell in row.cells:
                    cleaned_cell = _sanitize_business_line(cell.text or "")
                    if cleaned_cell:
                        cleaned_cells.append(cleaned_cell)
                row_text = " | ".join(cleaned_cells)
                row_text = _sanitize_business_line(row_text)
                if not row_text:
                    row_rids = _collect_image_rids(row._tr)
                    if row_rids:
                        _append_image_ocr_by_rids(row_rids, section)
                    continue
                lines.append(row_text)
                units.append({"chapter": chapter, "section": section, "content": row_text})
                row_rids = _collect_image_rids(row._tr)
                if row_rids:
                    _append_image_ocr_by_rids(row_rids, section)

    return ParseOutput(
        markdown_text="\n".join(lines).strip(),
        units=units,
        image_count=image_count,
        document_type="docx",
    )


def _extract_pdf(
    file_path: Path,
    image_dir: Path,
    api_base: str,
) -> ParseOutput:
    """解析 pdf，逐页提取文本和图片 OCR。"""
    try:
        pdf_doc = fitz.open(str(file_path))
    except Exception as exc:  # noqa: BLE001
        raise RuntimeError("文件损坏，无法解析，请检查文件后重新上传") from exc

    if pdf_doc.needs_pass:
        pdf_doc.close()
        raise RuntimeError("文件已加密，请解密后重新上传")

    lines: List[str] = []
    units: List[Dict[str, Any]] = []
    image_count = 0
    chapter = "PDF正文"
    ocr_cache: Dict[str, str] = {}

    try:
        for page_index in range(pdf_doc.page_count):
            page_no = page_index + 1
            page = pdf_doc.load_page(page_index)
            page_text = _sanitize_business_text_block(page.get_text("text") or "")
            lines.append(f"# 第{page_no}页")
            if page_text:
                lines.append(page_text)
                units.append(
                    {
                        "chapter": chapter,
                        "section": f"第{page_no}页",
                        "content": page_text,
                    }
                )

            for image_idx, image in enumerate(page.get_images(full=True), start=1):
                xref = image[0]
                image_dict = pdf_doc.extract_image(xref)
                image_bytes = image_dict.get("image")
                if not image_bytes:
                    continue
                image_count += 1
                image_ext = str(image_dict.get("ext", "png")).lower()
                if image_ext == "jpg":
                    image_ext = "jpeg"
                image_hash = hashlib.sha256(image_bytes).hexdigest()
                output_prefix = image_dir / f"pdf_p{page_no:04d}_{image_idx:03d}"
                try:
                    image_path = normalize_image_for_ocr(
                        image_bytes=image_bytes,
                        content_type=f"image/{image_ext}",
                        output_prefix=output_prefix,
                    )
                except Exception as exc:  # noqa: BLE001
                    _append_ocr_failure_log(
                        log_dir=image_dir,
                        image_path=str(output_prefix),
                        error=f"图片归一化失败: {exc}",
                        stage="normalize",
                        api_base=api_base,
                        attempt=1,
                        total_attempts=1,
                    )
                    print(
                        f"[OCR警告] PDF图片归一化失败并已跳过: {output_prefix}",
                        file=sys.stderr,
                    )
                    continue
                if image_hash in ocr_cache:
                    ocr_text = ocr_cache[image_hash]
                else:
                    ocr_text = _sanitize_business_text_block(call_server_ocr(api_base=api_base, image_path=image_path))
                    if ocr_text:
                        ocr_cache[image_hash] = ocr_text
                lines.append(f"![pdf_page_{page_no}_{image_idx}]({image_path.as_posix()})")
                if ocr_text:
                    lines.append(f"> OCR结果: {ocr_text}")
                    units.append(
                        {
                            "chapter": chapter,
                            "section": f"第{page_no}页_图片OCR",
                            "content": ocr_text,
                        }
                    )
                else:
                    lines.append("> OCR结果: ")
            lines.append("")
    finally:
        pdf_doc.close()

    return ParseOutput(
        markdown_text="\n".join(lines).strip(),
        units=units,
        image_count=image_count,
        document_type="pdf",
    )


def _normalize_doc_fragment(fragment: str) -> str:
    """规范化 doc 片段：压缩空白并裁剪首尾空格。"""
    return re.sub(r"\s+", " ", str(fragment or "")).strip()


def _strip_toc_page_suffix(line: str) -> str:
    """
    去掉目录型页码尾缀，避免“条款文本 + (P2200)”被当作真实条款内容。

    说明：
    - 只移除尾缀，不改动正文主体；
    - 允许多轮去尾缀，兼容“……2200(P2200)”这类叠加噪声。
    """
    cleaned = str(line or "")
    while True:
        stripped = TOC_PAGE_BRACKET_SUFFIX_PATTERN.sub("", cleaned)
        stripped = TOC_DOT_PAGE_SUFFIX_PATTERN.sub("", stripped)
        if stripped == cleaned:
            break
        cleaned = stripped
    return cleaned


def _sanitize_business_line(line: str) -> str:
    """
    规范化单行业务文本，专门拦截目录页码噪声。

    处理规则：
    1) 统一空白；
    2) 去掉目录页码尾缀；
    3) 丢弃“仅页码标记”的孤立行（如 P2200）。
    """
    normalized = _normalize_doc_fragment(line)
    if not normalized:
        return ""
    normalized = _normalize_doc_fragment(_strip_toc_page_suffix(normalized))
    if not normalized:
        return ""
    if TOC_PAGE_ONLY_PATTERN.fullmatch(normalized):
        return ""
    return normalized


def _sanitize_business_text_block(text: str) -> str:
    """
    规范化多行文本块，逐行清洗目录页码噪声后再回拼。

    这样可以在不破坏段落结构的前提下，统一处理 PDF 正文与 OCR 回填内容。
    """
    cleaned_lines: List[str] = []
    for raw_line in str(text or "").splitlines():
        cleaned = _sanitize_business_line(raw_line)
        if cleaned:
            cleaned_lines.append(cleaned)
    return "\n".join(cleaned_lines).strip()


def _line_has_toc_page_marker(line: str) -> bool:
    """
    判断单行是否携带目录型页码标记。

    该函数用于召回阶段做“命中去噪”，避免目录页签行被误识别为条款证据。
    """
    normalized = _normalize_doc_fragment(line)
    if not normalized:
        return False
    if TOC_PAGE_ONLY_PATTERN.fullmatch(normalized):
        return True
    if TOC_PAGE_BRACKET_SUFFIX_PATTERN.search(normalized):
        return True
    if TOC_DOT_PAGE_SUFFIX_PATTERN.search(normalized):
        return True
    return False


def _is_toc_noise_chunk(chunk: Dict[str, Any]) -> bool:
    """
    判断 chunk 是否主要由目录页码噪声组成。

    设计目标：
    - 章节兜底时优先回避“目录区/页码索引区”；
    - 若 chunk 清洗后几乎无有效文本，视为噪声 chunk。
    """
    content = str(chunk.get("content", "")).strip()
    if not content:
        return True
    cleaned_block = _sanitize_business_text_block(content)
    if not cleaned_block:
        return True

    lines = [line.strip() for line in content.splitlines() if line.strip()]
    if not lines:
        return True
    noise_hits = sum(1 for line in lines if _line_has_toc_page_marker(line))
    return noise_hits >= max(1, len(lines) // 2)


def _is_likely_doc_noise_fragment(line: str) -> bool:
    """
    判断片段是否更像 OLE/样式元数据噪声而非正文。

    设计目的：
    - 屏蔽 _Toc、xml 路径、域代码、纯 ASCII 样式键等常见垃圾片段；
    - 尽量不误伤中文正文，避免“过滤过度导致漏提取”。
    """
    normalized = _normalize_doc_fragment(line)
    if not normalized:
        return True

    lower_line = normalized.lower()
    if DOC_TOC_PATTERN.match(normalized):
        return True
    if DOC_REPEAT_PATTERN.match(normalized):
        return True
    if DOC_ASCII_METADATA_PATTERN.fullmatch(normalized):
        return True
    if lower_line in {"xml", "rels", "pk", "word", "times", "roman", "calibri", "arial"}:
        return True
    if any(marker in lower_line for marker in DOC_NOISE_MARKERS):
        return True
    return False


def _doc_fragment_quality_score(line: str) -> float:
    """
    为 doc 片段打“正文可信度”分数，分值越高越接近自然正文。

    评分规则要点：
    - 正向：招投标关键词、常见中文功能字、中文标点、日期/编号特征；
    - 负向：低中文语义密度、疑似噪声特征。
    """
    normalized = _normalize_doc_fragment(line)
    if not normalized:
        return -10.0

    chinese_chars = re.findall(r"[\u4e00-\u9fff]", normalized)
    if not chinese_chars:
        # .doc 场景的业务正文以中文为主，纯 ASCII 片段通常是样式/索引噪声。
        return -2.0

    keyword_hits = sum(1 for keyword in DOC_HINT_KEYWORDS if keyword in normalized)
    common_hits = sum(1 for char in chinese_chars if char in DOC_COMMON_CHARS)
    common_ratio = common_hits / max(len(chinese_chars), 1)
    punctuation_hits = len(re.findall(r"[，。；：、“”‘’（）()【】\[\]、]", normalized))
    has_datetime_or_no = bool(re.search(r"\d{4}年|\d+月|\d+日|\d+时\d+分|招标编号", normalized))

    # 对“没有关键词、没有常见功能字、没有时间编号特征”的片段强降权，
    # 这类片段在乱码样本中占比很高。
    if keyword_hits == 0 and common_hits == 0 and not has_datetime_or_no:
        return -1.0
    if keyword_hits == 0 and common_hits <= 1 and len(normalized) <= 12 and not has_datetime_or_no:
        return -0.5
    if keyword_hits == 0 and common_ratio < 0.30 and punctuation_hits == 0 and not has_datetime_or_no:
        return -0.5

    score = keyword_hits * 4.0 + common_hits * 0.5 + punctuation_hits * 0.4
    if has_datetime_or_no:
        score += 1.5
    return score


def _extract_doc_fragments_from_stream(stream_data: bytes) -> List[str]:
    """
    从单个 OLE 流中提取“质量最优”的文本片段集合。

    关键策略：
    - 同一流只保留一个最佳编码结果，避免多编码混合导致乱码倍增；
    - 每个候选编码先做噪声过滤，再按正文分数累计，选总分最高者。
    """
    best_lines: List[str] = []
    best_score = float("-inf")

    for encoding in DOC_CANDIDATE_ENCODINGS:
        try:
            decoded = stream_data.decode(encoding, errors="ignore")
        except Exception:  # noqa: BLE001
            continue

        raw_fragments = DOC_FRAGMENT_PATTERN.findall(decoded)
        if not raw_fragments:
            continue

        filtered_lines: List[str] = []
        cumulative_score = 0.0
        for fragment in raw_fragments:
            line = _normalize_doc_fragment(fragment)
            if len(line) < 4:
                continue
            if _is_likely_doc_noise_fragment(line):
                continue
            line_score = _doc_fragment_quality_score(line)
            if line_score <= 0:
                continue
            filtered_lines.append(line)
            cumulative_score += line_score

        if not filtered_lines:
            continue

        # 额外给“命中业务关键词”的编码结果加分，优先选择最像正文的解码。
        keyword_bonus = sum(
            1
            for sample in filtered_lines[:500]
            if any(keyword in sample for keyword in DOC_HINT_KEYWORDS)
        )
        final_score = cumulative_score + keyword_bonus * 2.0

        if final_score > best_score:
            best_score = final_score
            best_lines = filtered_lines

    return best_lines


def _deduplicate_doc_lines(lines: Sequence[str]) -> List[str]:
    """按出现顺序去重，稳定后续 chunk 构建顺序。"""
    unique: List[str] = []
    seen: set[str] = set()
    for raw in lines:
        line = _normalize_doc_fragment(raw)
        if not line:
            continue
        if line in seen:
            continue
        seen.add(line)
        unique.append(line)
    return unique


def _extract_doc(file_path: Path) -> ParseOutput:
    """解析 legacy doc（文本优先，不做图片抽取）。"""
    if not olefile.isOleFile(str(file_path)):
        raise RuntimeError("文件损坏，无法解析，请检查文件后重新上传")

    stream_bytes_map: Dict[str, bytes] = {}
    with olefile.OleFileIO(str(file_path)) as ole:
        # 先建立“流名 -> 流路径”映射，便于按优先级读取固定流。
        stream_path_map = {"/".join(stream_path): stream_path for stream_path in ole.listdir(streams=True, storages=False)}
        for stream_name in DOC_STREAM_PRIORITY:
            stream_path = stream_path_map.get(stream_name)
            if not stream_path:
                continue
            try:
                stream_bytes_map[stream_name] = ole.openstream(stream_path).read()
            except Exception:  # noqa: BLE001
                continue

    if not stream_bytes_map:
        raise RuntimeError("文件损坏，无法解析，请检查文件后重新上传")

    selected_lines: List[str] = []
    for stream_name in DOC_STREAM_PRIORITY:
        stream_data = stream_bytes_map.get(stream_name)
        if not stream_data:
            continue
        candidate_lines = _deduplicate_doc_lines(_extract_doc_fragments_from_stream(stream_data))
        if len(candidate_lines) > len(selected_lines):
            selected_lines = candidate_lines

        # 正文流达到最小有效规模后直接采用，避免再次引入低优先级流噪声。
        if stream_name == "WordDocument" and len(candidate_lines) >= 80:
            selected_lines = candidate_lines
            break

    # 兼容兜底：若过滤后片段过少，回退到历史逻辑做一次“保守提取”，
    # 但仍套用噪声与质量过滤，防止重新引入大面积乱码。
    if len(selected_lines) < 20:
        fallback_lines: List[str] = []
        for stream_data in stream_bytes_map.values():
            for encoding in DOC_CANDIDATE_ENCODINGS:
                try:
                    decoded = stream_data.decode(encoding, errors="ignore")
                except Exception:  # noqa: BLE001
                    continue
                for fragment in DOC_FRAGMENT_PATTERN.findall(decoded):
                    line = _normalize_doc_fragment(fragment)
                    if len(line) < 4:
                        continue
                    if _is_likely_doc_noise_fragment(line):
                        continue
                    if _doc_fragment_quality_score(line) <= 0:
                        continue
                    fallback_lines.append(line)
        selected_lines = _deduplicate_doc_lines(fallback_lines)

    if not selected_lines:
        raise RuntimeError("文件损坏，无法解析，请检查文件后重新上传")

    _chapter_re = re.compile(
        r"^(第[一二三四五六七八九十百千\d]+[章篇部]|[一二三四五六七八九十]+[、．.])"
    )
    _section_re = re.compile(
        r"^(\d+[\.\u3001]\d*|[（(]\d+[)）]|第[一二三四五六七八九十百千\d]+[节条款])"
    )

    lines: List[str] = []
    units: List[Dict[str, Any]] = []
    chapter = "DOC正文"
    section = "DOC正文"
    for line in selected_lines:
        if _chapter_re.match(line):
            chapter = line[:50]
            section = chapter
            lines.append(f"# {line}")
        elif _section_re.match(line):
            section = line[:50]
            lines.append(f"## {line}")
        else:
            lines.append(line)
        units.append(
            {
                "chapter": chapter,
                "section": section,
                "content": line,
            }
        )

    return ParseOutput(
        markdown_text="\n".join(lines).strip(),
        units=units,
        image_count=0,
        document_type="doc",
    )


def parse_document_to_markdown(file_path: Path, image_dir: Path, api_base: str) -> ParseOutput:
    """按扩展名解析文档为 markdown + 结构化文本单元。"""
    suffix = file_path.suffix.lower()
    if suffix == ".docx":
        return _extract_docx(file_path=file_path, image_dir=image_dir, api_base=api_base)
    if suffix == ".pdf":
        return _extract_pdf(file_path=file_path, image_dir=image_dir, api_base=api_base)
    if suffix == ".doc":
        return _extract_doc(file_path=file_path)
    raise RuntimeError("不支持该格式文件，请上传.doc/.docx/.pdf 格式文件")


def build_keywords(instruction: str | None, custom_keywords: Sequence[str] | None = None) -> List[str]:
    """构造关键词候选列表。"""
    keywords = set(DEFAULT_KEYWORDS)
    for kw in custom_keywords or []:
        kw_clean = str(kw).strip()
        if len(kw_clean) >= 2:
            keywords.add(kw_clean)
    if instruction:
        for token in re.findall(r"[\u4e00-\u9fffA-Za-z0-9]{2,20}", instruction):
            if len(token) >= 2:
                keywords.add(token)
    return sorted(keywords)


def build_chunks(units: Sequence[Dict[str, Any]], direct_whole_text: str, threshold: int) -> List[Dict[str, Any]]:
    """按阈值构造 chunks_json。"""
    total = estimate_tokens(direct_whole_text)
    if total <= threshold:
        return [
            {
                "chunk_id": "chunk_0001",
                "chapter": "全文",
                "section": "全文",
                "content": direct_whole_text,
            }
        ]

    grouped: Dict[tuple[str, str], List[str]] = {}
    for unit in units:
        chapter = str(unit.get("chapter", "未分章") or "未分章")
        section = str(unit.get("section", "未分节") or "未分节")
        grouped.setdefault((chapter, section), []).append(str(unit.get("content", "")).strip())

    chunks: List[Dict[str, Any]] = []
    for index, ((chapter, section), texts) in enumerate(grouped.items(), start=1):
        content = "\n".join(item for item in texts if item).strip()
        if not content:
            continue
        chunks.append(
            {
                "chunk_id": f"chunk_{index:04d}",
                "chapter": chapter,
                "section": section,
                "content": content,
            }
        )
    return chunks


def _guess_project_name(document_name: str, chunks: Sequence[Dict[str, Any]]) -> str:
    """从文档名和内容中推测项目名称。"""
    for chunk in chunks:
        content = str(chunk.get("content", "")).strip()
        if not content:
            continue
        match = re.search(r"《([^》]{2,120})》", content)
        if match:
            return match.group(1).strip()
        match = re.search(r"([^\n]{2,120}项目)", content)
        if match:
            return match.group(1).strip()
    stem = Path(document_name).stem.strip()
    return stem or "未识别项目"


def _extract_category_items(
    chunks: Sequence[Dict[str, Any]],
    retrieval_hits: Sequence[Dict[str, Any]],
    keywords: Sequence[str],
    max_items: int = 20,
) -> List[Dict[str, Any]]:
    """按关键词从检索命中与分块内容中抽取条目。"""
    candidates: List[str] = []
    for hit in retrieval_hits:
        snippet = str(hit.get("snippet", "")).strip()
        if snippet:
            candidates.append(snippet)
    for chunk in chunks:
        content = str(chunk.get("content", "")).strip()
        if content:
            candidates.append(content)

    items: List[Dict[str, Any]] = []
    seen: set[str] = set()
    for text in candidates:
        lines = [line.strip() for line in text.splitlines() if line.strip()]
        for raw_line in lines:
            # 命中去噪：目录页码样式行（如“... (P2200)”）直接跳过，不参与条款提取。
            if _line_has_toc_page_marker(raw_line):
                continue
            line = _sanitize_business_line(raw_line)
            if not line:
                continue
            if not any(keyword in line for keyword in keywords):
                continue
            normalized = re.sub(r"\s+", " ", line)
            if len(normalized) < 4:
                continue
            key = normalized[:300]
            if key in seen:
                continue
            seen.add(key)
            items.append({"content": key, "pages": []})
            if len(items) >= max_items:
                return items
    return items


def extract_key_info_local(
    document_name: str,
    document_type: str,
    chunks_json: Sequence[Dict[str, Any]],
    retrieval_hits_json: Sequence[Dict[str, Any]],
    instruction: str | None = None,
) -> Dict[str, Any]:
    """
    在本地脚本中提取 key_info（不依赖服务端 /extract/key-info）。
    返回结构保持与服务端历史返回兼容。
    """
    chunks = [item for item in chunks_json if isinstance(item, dict)]
    retrieval_hits = [item for item in retrieval_hits_json if isinstance(item, dict)]
    if not chunks:
        raise RuntimeError("缺少有效的文档片段数据，请先执行本地解析脚本")

    categories: List[Dict[str, Any]] = []
    for config in KEY_INFO_CATEGORY_CONFIG:
        items = _extract_category_items(
            chunks=chunks,
            retrieval_hits=retrieval_hits,
            keywords=config["keywords"],
            max_items=20,
        )
        categories.append(
            {
                "category": config["name"],
                "count": len(items),
                "items": items,
            }
        )

    full_text = "\n".join(str(chunk.get("content", "")) for chunk in chunks).strip()
    supplemental_items: List[str] = []
    if instruction and instruction.strip():
        supplemental_items.append(f"用户补充要求：{instruction.strip()}")

    result = {
        "project_name": _guess_project_name(document_name=document_name, chunks=chunks),
        "strategy": "local_rule_extract",
        "total_chars": len(full_text),
        "categories": categories,
        "supplemental_items": supplemental_items,
    }

    return {
        "code": 0,
        "message": "success",
        "data": {
            "document_name": document_name,
            "document_type": document_type,
            "chunk_count": len(chunks),
            "retrieval_hit_count": len(retrieval_hits),
            "result": result,
        },
    }


def _snippet_from_chunk(content: str, keyword: str) -> str:
    """根据关键词截取片段摘要。"""
    idx = content.find(keyword)
    if idx < 0:
        return content[:240]
    start = max(0, idx - 80)
    end = min(len(content), idx + 160)
    return content[start:end]


def keyword_recall(chunks: Sequence[Dict[str, Any]], keywords: Sequence[str], max_hits: int = 120) -> List[Dict[str, Any]]:
    """第一层：关键词召回。"""
    hits: List[Dict[str, Any]] = []
    for chunk in chunks:
        content = str(chunk.get("content", ""))
        if not content:
            continue
        for keyword in keywords:
            if keyword not in content:
                continue
            hits.append(
                {
                    "keyword": keyword,
                    "chunk_id": chunk.get("chunk_id"),
                    "chapter": chunk.get("chapter"),
                    "section": chunk.get("section"),
                    "snippet": _snippet_from_chunk(content, keyword),
                }
            )
            if len(hits) >= max_hits:
                return hits
    return hits


def chapter_scoped_search(chunks: Sequence[Dict[str, Any]], keywords: Sequence[str]) -> List[Dict[str, Any]]:
    """第一层：先定位候选章节，再做章节内关键词召回。"""
    candidate_chapters: set[str] = set()
    for chunk in chunks:
        chapter = str(chunk.get("chapter", ""))
        if not chapter:
            continue
        if _line_has_toc_page_marker(chapter):
            continue
        if any(keyword in chapter for keyword in keywords):
            candidate_chapters.add(chapter)

    if not candidate_chapters:
        # 章节标题未命中时，优先跳过“目录页码噪声 chunk”，再选前若干有效章节。
        fallback_chunks = [chunk for chunk in chunks[:30] if not _is_toc_noise_chunk(chunk)]
        if not fallback_chunks:
            # 全部判为噪声时，回退旧逻辑，避免极端场景下完全无召回。
            fallback_chunks = list(chunks[:10])
        for chunk in fallback_chunks[:10]:
            chapter = str(chunk.get("chapter", ""))
            if chapter:
                candidate_chapters.add(chapter)

    scoped_chunks = [
        chunk
        for chunk in chunks
        if str(chunk.get("chapter", "")) in candidate_chapters and not _is_toc_noise_chunk(chunk)
    ]
    if not scoped_chunks:
        scoped_chunks = [chunk for chunk in chunks if str(chunk.get("chapter", "")) in candidate_chapters]
    return keyword_recall(scoped_chunks, keywords, max_hits=80)


def global_search(chunks: Sequence[Dict[str, Any]], keywords: Sequence[str], max_hits: int = 150) -> List[Dict[str, Any]]:
    """第三层：全局搜索（用户确认后执行）。"""
    hits = keyword_recall(chunks, keywords, max_hits=max_hits)
    if hits:
        return hits
    # 若关键词仍无命中，返回前若干片段作为全局上下文。
    fallback: List[Dict[str, Any]] = []
    for chunk in chunks[:20]:
        fallback.append(
            {
                "keyword": "GLOBAL_CONTEXT",
                "chunk_id": chunk.get("chunk_id"),
                "chapter": chunk.get("chapter"),
                "section": chunk.get("section"),
                "snippet": str(chunk.get("content", ""))[:240],
            }
        )
    return fallback


def _save_chunks_markdown(chunks: Sequence[Dict[str, Any]], chunk_dir: Path) -> None:
    """将 chunks 另存为 markdown 文件，方便 cowork grep 查找。"""
    # 兜底保障：即使上游目录被外部清理/并发影响，也先确保 chunks 目录存在，
    # 避免在写入第一个 chunk 文件时抛出 FileNotFoundError。
    chunk_dir.mkdir(parents=True, exist_ok=True)
    for chunk in chunks:
        chunk_id = str(chunk.get("chunk_id", "chunk"))
        chapter = str(chunk.get("chapter", "未分章"))
        section = str(chunk.get("section", "未分节"))
        content = str(chunk.get("content", ""))
        md = f"# {chunk_id}\n\n- 章节: {chapter}\n- 小节: {section}\n\n{content}\n"
        (chunk_dir / f"{chunk_id}.md").write_text(md, encoding="utf-8")


def prepare_document_payload(
    file_path: Path,
    skill_root: Path,
    role: str,
    api_base: str,
    instruction: str | None,
    enable_global_search: bool,
    custom_keywords: Sequence[str] | None = None,
    force_refresh: bool = False,
) -> Dict[str, Any]:
    """
    解析文档并生成提交服务端的 payload。

    返回字段包含：
    - chunks_json / retrieval_hits_json
    - need_global_search_confirmation
    - artifact_dir
    - strategy
    """
    if not file_path.exists():
        raise RuntimeError(f"文件不存在: {file_path}")

    resolved_file_path = file_path.expanduser().resolve()
    suffix = file_path.suffix.lower()
    if suffix not in {".doc", ".docx", ".pdf"}:
        raise RuntimeError("不支持该格式文件，请上传.doc/.docx/.pdf 格式文件")

    # 关键行为说明：
    # 1) 常规读取（force_refresh=False）优先复用缓存，保持现有性能；
    # 2) 再次读取/读取最新（force_refresh=True）时必须跳过缓存，强制重新解析，
    #    避免用户文档已修改但仍复用旧产物。
    if not force_refresh:
        cached_payload = _load_cached_payload(
            file_path=resolved_file_path,
            skill_root=skill_root,
            role=role,
            instruction=instruction,
            enable_global_search=enable_global_search,
            custom_keywords=custom_keywords,
        )
        if cached_payload:
            return cached_payload

    file_fingerprint = build_file_fingerprint(file_path=resolved_file_path)
    project_slug = safe_slug(file_path.stem)
    timestamp = now_stamp()
    artifact_dir = (
        skill_root
        / "artifacts"
        / project_slug
        / role
        / suffix.lstrip(".")
        / timestamp
    )
    image_dir = artifact_dir / "images"
    chunk_dir = artifact_dir / "chunks"
    hit_dir = artifact_dir / "hits"
    artifact_dir.mkdir(parents=True, exist_ok=True)
    image_dir.mkdir(parents=True, exist_ok=True)
    chunk_dir.mkdir(parents=True, exist_ok=True)
    hit_dir.mkdir(parents=True, exist_ok=True)

    parsed = parse_document_to_markdown(file_path=file_path, image_dir=image_dir, api_base=api_base)
    source_md_path = artifact_dir / "source.md"
    source_md_path.write_text(parsed.markdown_text, encoding="utf-8")

    token_estimate = estimate_tokens(parsed.markdown_text)
    source_char_count = len(parsed.markdown_text or "")
    preferred_search_target, preferred_reason = _select_preferred_search_target(source_char_count)
    chunks = build_chunks(
        units=parsed.units,
        direct_whole_text=parsed.markdown_text,
        threshold=SOURCE_TO_CHUNK_THRESHOLD,
    )
    retrieval_hits, need_global, strategy = _build_retrieval_result(
        chunks=chunks,
        token_estimate=token_estimate,
        instruction=instruction,
        enable_global_search=enable_global_search,
        custom_keywords=custom_keywords,
    )

    _save_chunks_markdown(chunks=chunks, chunk_dir=chunk_dir)
    (artifact_dir / CHUNKS_JSON_FILE_NAME).write_text(json.dumps(chunks, ensure_ascii=False, indent=2), encoding="utf-8")
    (hit_dir / "retrieval_hits.json").write_text(json.dumps(retrieval_hits, ensure_ascii=False, indent=2), encoding="utf-8")
    route_payload = _build_query_route_payload(
        artifact_dir=artifact_dir,
        source_char_count=source_char_count,
        preferred_search_target=preferred_search_target,
        preferred_reason=preferred_reason,
        generated_at=now_stamp(),
    )
    route_file = _write_query_route_file(artifact_dir=artifact_dir, route_payload=route_payload)

    manifest = {
        "file_name": resolved_file_path.name,
        "file_path": str(resolved_file_path),
        "file_fingerprint": file_fingerprint,
        "document_type": parsed.document_type,
        "token_estimate": token_estimate,
        "source_char_count": source_char_count,
        "source_to_chunk_threshold": SOURCE_TO_CHUNK_THRESHOLD,
        "preferred_search_target": preferred_search_target,
        "preferred_reason": preferred_reason,
        "chunk_split_threshold": SOURCE_TO_CHUNK_THRESHOLD,
        "token_threshold": TOKEN_THRESHOLD,
        "strategy": strategy,
        "need_global_search_confirmation": need_global,
        "image_count": parsed.image_count,
        "chunk_count": len(chunks),
        "retrieval_hit_count": len(retrieval_hits),
        "source_md_path": str(source_md_path),
        "query_route_file": str(route_file),
        "cache_hit": False,
        # 记录本次是否由上层显式触发强制刷新，便于审计与问题追踪。
        "force_refresh_requested": bool(force_refresh),
        # 当 force_refresh=True 时，说明主动跳过了缓存读取分支。
        "cache_lookup_skipped": bool(force_refresh),
    }
    (artifact_dir / MANIFEST_FILE_NAME).write_text(json.dumps(manifest, ensure_ascii=False, indent=2), encoding="utf-8")

    payload = {
        "artifact_dir": str(artifact_dir),
        "document_name": resolved_file_path.name,
        "document_type": parsed.document_type,
        "token_estimate": token_estimate,
        "source_char_count": source_char_count,
        "source_to_chunk_threshold": SOURCE_TO_CHUNK_THRESHOLD,
        "preferred_search_target": preferred_search_target,
        "preferred_reason": preferred_reason,
        "query_route_file": str(route_file),
        "query_route": route_payload,
        "strategy": strategy,
        "chunks_json": chunks,
        "retrieval_hits_json": retrieval_hits,
        "need_global_search_confirmation": need_global,
        "suggested_question": "当前未命中有效片段，是否执行全局搜索？",
        "manifest": manifest,
        "cache_hit": False,
        # 对调用方透出刷新行为，便于上层日志/调试直接判断是否强刷。
        "force_refresh_requested": bool(force_refresh),
        "cache_lookup_skipped": bool(force_refresh),
    }
    (artifact_dir / PREPARED_PAYLOAD_FILE_NAME).write_text(
        json.dumps(
            {
                "document_name": payload["document_name"],
                "document_type": payload["document_type"],
                "token_estimate": payload["token_estimate"],
                "source_char_count": payload["source_char_count"],
                "source_to_chunk_threshold": payload["source_to_chunk_threshold"],
                "preferred_search_target": payload["preferred_search_target"],
                "preferred_reason": payload["preferred_reason"],
                "query_route_file": payload["query_route_file"],
                "query_route": payload["query_route"],
                "strategy": payload["strategy"],
                "chunks_json": chunks,
                "retrieval_hits_json": retrieval_hits,
                "need_global_search_confirmation": need_global,
                "file_fingerprint": file_fingerprint,
                # 持久化强刷标记，确保离线查看 prepared_payload 也能确认触发来源。
                "force_refresh_requested": bool(force_refresh),
                "cache_lookup_skipped": bool(force_refresh),
            },
            ensure_ascii=False,
            indent=2,
        ),
        encoding="utf-8",
    )
    return payload


def post_json(api_base: str, endpoint: str, payload: Dict[str, Any], timeout_sec: int = 1800) -> Dict[str, Any]:
    """以 JSON 方式调用服务端接口。"""
    normalized_endpoint = _normalize_endpoint_path(endpoint)
    request_url = f"{api_base.rstrip('/')}{normalized_endpoint}"
    compat_mode = _detect_request_contract(api_base=api_base, endpoint=normalized_endpoint)
    if normalized_endpoint in JSON_REQUIRED_ENDPOINTS and compat_mode == CONTRACT_MULTIPART:
        raise RuntimeError(
            f"服务契约不匹配[{request_url}] (compat_mode={compat_mode})："
            f"当前服务仍是旧版 multipart 接口，请重启到当前代码版本后重试。"
        )

    body = json.dumps(payload, ensure_ascii=False).encode("utf-8")
    req = urllib.request.Request(
        url=request_url,
        data=body,
        method="POST",
    )
    req.add_header("Content-Type", "application/json")
    req.add_header("Content-Length", str(len(body)))
    try:
        with urllib.request.urlopen(req, timeout=timeout_sec) as resp:
            return json.loads(resp.read().decode("utf-8"))
    except urllib.error.HTTPError as exc:
        detail = ""
        try:
            detail_bytes = exc.read()
            if detail_bytes:
                detail = detail_bytes.decode("utf-8", errors="replace")
        except Exception as read_exc:  # noqa: BLE001
            detail = f"<error_body_unavailable: {read_exc}>"
        if not detail:
            detail = str(getattr(exc, "reason", "")) or "<empty error body>"
        raise RuntimeError(
            f"HTTP {exc.code} [{request_url}] (compat_mode={compat_mode}): {detail}"
        ) from exc
    except Exception as exc:  # noqa: BLE001
        raise RuntimeError(
            f"请求失败[{request_url}] (compat_mode={compat_mode}): {exc}"
        ) from exc
