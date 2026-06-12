#!/usr/bin/env python3
from __future__ import annotations

"""
标书审查 — 文档预处理脚本。

职责：
1) 解析招标文件并获取 key_info（复用已有缓存或重新解析）
2) 解析标书文件生成文本产物
3) 输出两份文档的 artifact 目录路径，供 Agent 后续 Grep/Read 使用
4) 输出 ask_ai 策略文本（供 Cowork 自主问询，不在本地新增 LLM 调用）

审查判断由 Agent 在 SKILL.md 引导下自主完成，本脚本不做任何审查逻辑。
"""

import argparse
import json
import sys
from pathlib import Path

EXTRACT_SKILL_DIRNAME = "tender-document-parsing"


def _load_pipeline_module() -> tuple:
    """从"招标文件解析"技能目录加载共享流水线模块。"""
    skills_root = Path(__file__).resolve().parents[2]
    pipeline_dir = skills_root / EXTRACT_SKILL_DIRNAME / "scripts"
    if not pipeline_dir.exists():
        raise RuntimeError(f"未找到共享流水线脚本目录: {pipeline_dir}")
    sys.path.insert(0, str(pipeline_dir))
    from local_pipeline import extract_key_info_local, prepare_document_payload, resolve_api_base  # type: ignore

    return prepare_document_payload, extract_key_info_local, resolve_api_base


def _build_ask_ai_strategy_text(key_info_payload: dict) -> str:
    """输出给 Cowork 的策略问询文本（不在本地直接调用 LLM）。"""
    result = key_info_payload.get("result", {}) if isinstance(key_info_payload, dict) else {}
    categories = result.get("categories", []) if isinstance(result, dict) else []
    highlights = []
    if isinstance(categories, list):
        for category in categories:
            if not isinstance(category, dict):
                continue
            name = str(category.get("category", "")).strip()
            count = int(category.get("count", 0) or 0)
            if name and count > 0:
                highlights.append(f"{name}({count})")
    highlight_text = "、".join(highlights[:8]) if highlights else "暂无高置信命中条目"
    return (
        "请由 Cowork 基于以下上下文自主发起策略问询：\n"
        f"1) 招标文件关键分类命中：{highlight_text}\n"
        "2) 先围绕废标项/保证金/评分标准追问证据是否完整，再追问响应充分性与整改优先级。\n"
        "3) 问询输出要求：\n"
        '   - 风险等级必须使用中文"高风险/中等风险/低风险"，禁止输出 high/medium/low。\n'
        "   - 每条结论须附带至少 2 段证据（招标证据 + 标书实质证据），且标书证据不得仅为引用/目录/模板。\n"
        "   - 每条结论须附带章节定位（章节/小节）与原文证据片段。\n"
        '   - 证据不足时输出"证据不足/待补充"，不得强行定性。'
    )


def main() -> None:
    """命令行入口。"""
    prepare_document_payload, extract_key_info_local, resolve_api_base = _load_pipeline_module()

    parser = argparse.ArgumentParser(description="标书审查预处理：解析文档并获取 key_info")
    parser.add_argument("--tender", required=True, help="招标文件绝对路径（doc/docx/pdf）")
    parser.add_argument("--bid", required=True, help="标书文件绝对路径（doc/docx/pdf）")
    parser.add_argument(
        "--api-base",
        default=None,
        help="服务地址；未传时默认读取 tender-document-parsing/.skill.env 中的 BID_API_BASE",
    )
    parser.add_argument("--instruction", default=None, help="补充指令")
    parser.add_argument("--enable-global-search", action="store_true", help="允许全局搜索")
    parser.add_argument("--keywords", nargs="*", default=None, help="自定义候选关键词列表")
    parser.add_argument(
        "--force-refresh",
        action="store_true",
        help="强制刷新：跳过缓存并重新解析招标文件与标书，适用于再次读取最新内容",
    )
    args = parser.parse_args()

    tender_path = Path(args.tender).expanduser().resolve()
    bid_path = Path(args.bid).expanduser().resolve()
    try:
        # 地址解析复用 tender-document-parsing 的统一策略：
        # - 支持 --api-base 临时覆盖；
        # - 默认读取 tender-document-parsing/.skill.env；
        # - 命中本地地址会直接失败，确保链路固定走远程服务。
        api_base = resolve_api_base(
            args.api_base,
            dotenv_candidates=[tender_path.parent / ".env", bid_path.parent / ".env"],
        )
    except ValueError as exc:
        raise SystemExit(f"[配置错误] {exc}") from exc
    print(f"[标书审查] 使用服务地址: {api_base}", file=sys.stderr)
    print("[提示] 默认读取 tender-document-parsing/.skill.env 的 BID_API_BASE。", file=sys.stderr)

    if not tender_path.exists():
        raise SystemExit(f"请提供招标文件。文件不存在: {tender_path}")
    if not bid_path.exists():
        raise SystemExit(f"标书文件不存在: {bid_path}")

    skills_root = Path(__file__).resolve().parents[2]
    extract_skill_root = skills_root / EXTRACT_SKILL_DIRNAME
    skill_root = Path(__file__).resolve().parents[1]

    tender_payload = prepare_document_payload(
        file_path=tender_path,
        skill_root=extract_skill_root,
        role="extract",
        api_base=api_base,
        instruction=args.instruction,
        enable_global_search=bool(args.enable_global_search),
        custom_keywords=args.keywords,
        # 强制刷新时，招标文件侧跳过缓存，确保 key_info 基于最新文档重建。
        force_refresh=bool(args.force_refresh),
    )

    bid_payload = prepare_document_payload(
        file_path=bid_path,
        skill_root=skill_root,
        role="review_bid",
        api_base=api_base,
        instruction=args.instruction,
        enable_global_search=bool(args.enable_global_search),
        custom_keywords=args.keywords,
        # 强制刷新时，标书侧同样跳过缓存，避免审查证据仍来自历史产物。
        force_refresh=bool(args.force_refresh),
    )

    if tender_payload["need_global_search_confirmation"] or bid_payload["need_global_search_confirmation"]:
        print(
            json.dumps(
                {
                    "code": 0,
                    "message": "need_global_search_confirmation",
                    "data": {
                        "question": "当前检索命中不足，是否执行全局搜索？",
                        "tender_need_confirm": tender_payload["need_global_search_confirmation"],
                        "bid_need_confirm": bid_payload["need_global_search_confirmation"],
                        "tender_artifact_dir": tender_payload["artifact_dir"],
                        "bid_artifact_dir": bid_payload["artifact_dir"],
                    },
                },
                ensure_ascii=False,
                indent=2,
            )
        )
        return

    key_info_response = extract_key_info_local(
        document_name=tender_payload["document_name"],
        document_type=tender_payload["document_type"],
        chunks_json=tender_payload["chunks_json"],
        retrieval_hits_json=tender_payload["retrieval_hits_json"],
        instruction=args.instruction,
    )

    result = {
        "code": 0,
        "message": "success",
        "data": {
            "tender_document_name": tender_payload["document_name"],
            "bid_document_name": bid_payload["document_name"],
            "tender_artifact_dir": tender_payload["artifact_dir"],
            "bid_artifact_dir": bid_payload["artifact_dir"],
            "tender_query_route_file": tender_payload.get("query_route_file"),
            "bid_query_route_file": bid_payload.get("query_route_file"),
            "tender_query_route": tender_payload.get("query_route"),
            "bid_query_route": bid_payload.get("query_route"),
            "tender_source_char_count": tender_payload.get("source_char_count"),
            "bid_source_char_count": bid_payload.get("source_char_count"),
            "tender_preferred_search_target": tender_payload.get("preferred_search_target"),
            "bid_preferred_search_target": bid_payload.get("preferred_search_target"),
            "tender_preferred_reason": tender_payload.get("preferred_reason"),
            "bid_preferred_reason": bid_payload.get("preferred_reason"),
            "source_to_chunk_threshold": tender_payload.get("source_to_chunk_threshold"),
            "key_info": key_info_response.get("data", {}),
            "ask_ai_strategy_text": _build_ask_ai_strategy_text(key_info_response.get("data", {})),
            "bid_chunks_count": len(bid_payload["chunks_json"]),
            "tender_chunks_count": len(tender_payload["chunks_json"]),
            # 仅供内部链路排查与日志核对使用：
            # - 明确记录本次命令是否显式请求强制刷新；
            # - 同时记录两份文档是否跳过缓存/是否命中缓存，便于定位“是否真的重读”。
            # 注意：该字段是内部调试信息，不应直接对外展示给客户。
            "internal_refresh_state": {
                "force_refresh_arg": bool(args.force_refresh),
                "tender_force_refresh_requested": bool(tender_payload.get("force_refresh_requested", False)),
                "bid_force_refresh_requested": bool(bid_payload.get("force_refresh_requested", False)),
                "tender_cache_lookup_skipped": bool(tender_payload.get("cache_lookup_skipped", False)),
                "bid_cache_lookup_skipped": bool(bid_payload.get("cache_lookup_skipped", False)),
                "tender_cache_hit": bool(tender_payload.get("cache_hit", False)),
                "bid_cache_hit": bool(bid_payload.get("cache_hit", False)),
            },
        },
    }

    result_dir = Path(bid_payload["artifact_dir"])
    result_dir.mkdir(parents=True, exist_ok=True)
    (result_dir / "review_preparation.json").write_text(
        json.dumps(result, ensure_ascii=False, indent=2), encoding="utf-8"
    )
    print(json.dumps(result, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
