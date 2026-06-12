#!/usr/bin/env python3
from __future__ import annotations

"""
招标文件解析脚本入口。

流程：
1) 本地解析 doc/docx/pdf；
2) 按小文档/大文档策略构建检索上下文；
3) 执行本地规则 key_info 提取；
4) 在本地脚本执行 key_info 提取（不调用服务端 key-info 接口）。
"""

import argparse
import json
import sys
from pathlib import Path

from local_pipeline import extract_key_info_local, prepare_document_payload, resolve_api_base


def _configure_utf8_stdio() -> None:
    """
    统一配置标准输出/错误输出编码为 UTF-8。

    背景：
    - Windows 下默认控制台编码常为 GBK；
    - OCR/解析结果里可能包含 GBK 无法编码的 Unicode 字符；
    - 直接 print(ensure_ascii=False) 会触发 UnicodeEncodeError。

    这里在脚本入口强制 UTF-8，并将错误策略设为 replace，避免输出阶段中断主流程。
    """
    for stream in (sys.stdout, sys.stderr):
        try:
            stream.reconfigure(encoding="utf-8", errors="replace")
        except Exception:  # noqa: BLE001
            # 某些受限环境（例如被重定向的特殊流）不支持 reconfigure，忽略即可。
            continue


def _safe_print_json(payload: dict) -> None:
    """
    安全输出 JSON。

    优先按 UTF-8/中文直出；若外部环境仍强制使用窄编码导致异常，
    回退为 ensure_ascii=True，确保脚本不因打印失败而退出。
    """
    try:
        print(json.dumps(payload, ensure_ascii=False, indent=2))
    except UnicodeEncodeError:
        print(json.dumps(payload, ensure_ascii=True, indent=2))


def main() -> None:
    """命令行入口。"""
    _configure_utf8_stdio()
    parser = argparse.ArgumentParser(description="调用招标文件解析（关键信息提取）")
    parser.add_argument("--file", required=True, help="招标文件绝对路径（doc/docx/pdf）")
    parser.add_argument(
        "--api-base",
        default=None,
        help="服务地址；未传时默认读取 tender-document-parsing/.skill.env 中的 BID_API_BASE",
    )
    parser.add_argument("--instruction", default=None, help="补充提取指令")
    parser.add_argument("--enable-global-search", action="store_true", help="允许执行全局搜索（需用户确认后再开启）")
    parser.add_argument("--keywords", nargs="*", default=None, help="自定义候选关键词列表")
    parser.add_argument(
        "--force-refresh",
        action="store_true",
        help="强制刷新：跳过缓存并重新解析文档，适用于再次读取最新内容",
    )
    args = parser.parse_args()

    file_path = Path(args.file).expanduser().resolve()
    try:
        # 统一在这里做地址解析与远程地址校验：
        # - 优先遵循 --api-base；
        # - 未传时读取 skill/.skill.env 的 BID_API_BASE；
        # - 命中本地地址会直接抛错，避免误连本机 127.0.0.1。
        api_base = resolve_api_base(args.api_base, dotenv_candidates=[file_path.parent / ".env"])
    except ValueError as exc:
        raise SystemExit(f"[配置错误] {exc}") from exc
    print(f"[招标文件解析] 使用服务地址: {api_base}", file=sys.stderr)
    print("[提示] 默认读取 skill/.skill.env 的 BID_API_BASE；可用 --api-base 临时覆盖。", file=sys.stderr)
    skill_root = Path(__file__).resolve().parents[1]

    payload = prepare_document_payload(
        file_path=file_path,
        skill_root=skill_root,
        role="extract",
        api_base=api_base,
        instruction=args.instruction,
        enable_global_search=bool(args.enable_global_search),
        custom_keywords=args.keywords,
        # 再次读取最新内容时由上层显式开启，确保不复用旧产物。
        force_refresh=bool(args.force_refresh),
    )

    # 命中不足时先返回确认请求，由上层 skill 触发 AskUserQuestion。
    if payload["need_global_search_confirmation"]:
        _safe_print_json(
            {
                "code": 0,
                "message": "need_global_search_confirmation",
                "data": {
                    "question": payload["suggested_question"],
                    "artifact_dir": payload["artifact_dir"],
                    "token_estimate": payload["token_estimate"],
                    "strategy": payload["strategy"],
                },
            }
        )
        return

    response = extract_key_info_local(
        document_name=payload["document_name"],
        document_type=payload["document_type"],
        chunks_json=payload["chunks_json"],
        retrieval_hits_json=payload["retrieval_hits_json"],
        instruction=args.instruction,
    )

    result_path = Path(payload["artifact_dir"]) / "result_extract.json"
    result_path.write_text(json.dumps(response, ensure_ascii=False, indent=2), encoding="utf-8")
    _safe_print_json(response)


if __name__ == "__main__":
    main()
