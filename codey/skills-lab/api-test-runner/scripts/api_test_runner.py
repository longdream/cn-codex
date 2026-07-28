#!/usr/bin/env python3
"""
API Test Runner - 根据 Swagger/OpenAPI 规范自动执行接口测试

用法:
  python api_test_runner.py --spec https://petstore.swagger.io/v2/swagger.json
  python api_test_runner.py --spec ./swagger.json --base-url https://api.example.com
  python api_test_runner.py --spec ./openapi.yaml --paths "/users,/pets" --headers "Authorization:Bearer xxx"
"""

import argparse
import json
import os
import re
import sys
import time
import traceback
from datetime import datetime
from urllib.parse import urljoin, urlparse

try:
    import requests
except ImportError:
    print("正在安装依赖 requests...")
    os.system(f"{sys.executable} -m pip install requests -q")
    import requests

try:
    import yaml
except ImportError:
    print("正在安装依赖 pyyaml...")
    os.system(f"{sys.executable} -m pip install pyyaml -q")
    import yaml

try:
    import jsonschema
except ImportError:
    print("正在安装依赖 jsonschema...")
    os.system(f"{sys.executable} -m pip install jsonschema -q")
    import jsonschema


# ── 工具函数 ──────────────────────────────────────────────────────────────


def parse_spec(source: str) -> dict:
    """
    从 URL 或本地文件路径加载 OpenAPI 规范（支持 JSON/YAML）。
    返回解析后的 dict。
    """
    if re.match(r"^https?://", source):
        resp = requests.get(source, timeout=30)
        resp.raise_for_status()
        raw = resp.text
    else:
        with open(source, "r", encoding="utf-8") as f:
            raw = f.read()

    # 尝试 JSON 解析，失败则尝试 YAML
    try:
        return json.loads(raw)
    except json.JSONDecodeError:
        pass
    try:
        return yaml.safe_load(raw)
    except yaml.YAMLError as e:
        raise ValueError(f"无法解析规范文件（非 JSON 也非 YAML）：{e}")


def detect_openapi_version(spec: dict) -> str:
    """检测 OpenAPI 版本（swagger: "2.0" 或 openapi: "3.x"）"""
    if "openapi" in spec:
        return "3.x"
    if "swagger" in spec:
        return "2.0"
    return "unknown"


def resolve_base_url(spec: dict, version: str, override: str = None) -> str:
    """从规范中提取 Base URL，支持用户覆盖"""
    if override:
        return override.rstrip("/")

    if version == "3.x":
        servers = spec.get("servers", [])
        if servers:
            return servers[0].get("url", "").rstrip("/")
        return ""
    else:  # Swagger 2.0
        host = spec.get("host", "localhost")
        scheme = "https" if "https" in spec.get("schemes", ["http"]) else "http"
        base_path = spec.get("basePath", "")
        return f"{scheme}://{host}{base_path}".rstrip("/")


def generate_mock_value(schema: dict, depth: int = 0) -> object:
    """
    根据 JSON Schema 生成 mock 数据。
    支持简单类型、数组、嵌套对象、enum、format 等。
    """
    if depth > 5:
        return None

    if schema is None:
        return None

    # 如果有 example，直接使用
    if "example" in schema:
        return schema["example"]

    # 如果有 default，直接使用
    if "default" in schema:
        return schema["default"]

    # 处理 enum
    enum_vals = schema.get("enum")
    if enum_vals:
        return enum_vals[0]

    schema_type = schema.get("type", "string")
    schema_format = schema.get("format", "")
    ref = schema.get("$ref", "")
    if ref:
        return f"__REF__:{ref}"

    if schema_type == "string":
        if schema_format == "date":
            return "2024-01-01"
        if schema_format == "date-time":
            return "2024-01-01T00:00:00Z"
        if schema_format == "email":
            return "test@example.com"
        if schema_format == "uri":
            return "https://example.com"
        if schema_format == "uuid":
            return "00000000-0000-0000-0000-000000000000"
        if schema_format == "byte":
            base64_val = "dGVzdA=="
            return base64_val
        if schema_format == "binary":
            return ""
        max_len = schema.get("maxLength", 20)
        min_len = schema.get("minLength", 1)
        return "test_string"

    if schema_type == "integer" or schema_type == "number":
        if "minimum" in schema:
            return schema["minimum"]
        if schema_type == "integer":
            return 1
        return 1.0

    if schema_type == "boolean":
        return True

    if schema_type == "array":
        items_schema = schema.get("items", {})
        item = generate_mock_value(items_schema, depth + 1)
        return [item] if item is not None else []

    if schema_type == "object":
        result = {}
        required = schema.get("required", [])
        properties = schema.get("properties", {})
        # 先生成 required 字段
        for prop_name in required:
            if prop_name in properties:
                result[prop_name] = generate_mock_value(properties[prop_name], depth + 1)
        # 再生成部分非 required 字段
        for prop_name, prop_schema in properties.items():
            if prop_name not in result:
                result[prop_name] = generate_mock_value(prop_schema, depth + 1)
        return result

    return None


def resolve_schema_ref(ref: str, spec: dict) -> dict:
    """解析 $ref 指向的 schema 定义"""
    # $ref 格式: "#/definitions/Pet" 或 "#/components/schemas/Pet"
    path = ref.lstrip("#/").split("/")
    current = spec
    for part in path:
        if isinstance(current, dict) and part in current:
            current = current[part]
        else:
            return {}
    return current


def extract_parameters(params: list, spec: dict, version: str) -> dict:
    """
    按参数位置（query, header, path, cookie）分组并生成 mock 值。
    返回: {"query": {...}, "header": {...}, "path": {...}}
    """
    grouped = {"query": {}, "header": {}, "path": {}, "cookie": {}}
    for param in params:
        # 处理 $ref
        if "$ref" in param:
            param = resolve_schema_ref(param["$ref"], spec)

        name = param.get("name", "")
        location = param.get("in", "query")
        required = param.get("required", False)
        schema = param.get("schema", param)  # Swagger 2.0 直接放在参数层级
        # 移除 schema 外的字段，只保留 schema 内容
        if "type" in param and "schema" not in param:
            schema = param

        if location in grouped:
            mock_val = generate_mock_value(schema)
            if mock_val is not None:
                grouped[location][name] = mock_val
            elif required:
                grouped[location][name] = ""  # 必填项至少给空字符串

        # 处理 header 参数中的固定值（如 API Key）
        if location == "header" and "x-api-key" in name.lower():
            grouped["header"][name] = "test_api_key"

    return grouped


def generate_request_body(request_body: dict, spec: dict, version: str) -> dict:
    """
    从 OpenAPI 3.x 的 requestBody 或 Swagger 2.0 的 body 参数生成 mock 请求体。
    返回 dict 或 None。
    """
    if version == "3.x":
        content = request_body.get("content", {})
        for media_type in ["application/json", "*/*"]:
            if media_type in content:
                media = content[media_type]
                schema = media.get("schema", {})
                # 处理 $ref
                if "$ref" in schema.get("$ref", ""):
                    schema = resolve_schema_ref(schema["$ref"], spec)
                return generate_mock_value(schema)
    return None


def extract_all_endpoints(spec: dict, version: str, path_filter: list = None) -> list:
    """
    提取规范中所有接口端点。
    返回列表: [(method, path, summary, params, request_body)]
    """
    endpoints = []
    paths = spec.get("paths", {})

    for path, path_item in paths.items():
        if path_filter and not any(path.startswith(f) or path == f for f in path_filter):
            continue

        if not isinstance(path_item, dict):
            continue

        for method in ["get", "post", "put", "delete", "patch", "options", "head"]:
            operation = path_item.get(method)
            if not operation:
                continue

            summary = operation.get("summary", operation.get("operationId", f"{method.upper()} {path}"))

            # 提取参数（公共参数 + 操作级参数）
            all_params = []
            all_params.extend(path_item.get("parameters", []))
            all_params.extend(operation.get("parameters", []))
            params = extract_parameters(all_params, spec, version)

            # 提取请求体
            request_body = None
            if version == "3.x" and "requestBody" in operation:
                request_body = generate_request_body(operation["requestBody"], spec, version)
            elif version == "2.0":
                # Swagger 2.0 的 body 参数
                for param in all_params:
                    if "$ref" in param:
                        param = resolve_schema_ref(param["$ref"], spec)
                    if param.get("in") == "body":
                        schema = param.get("schema", {})
                        if "$ref" in schema.get("$ref", ""):
                            schema = resolve_schema_ref(schema["$ref"], spec)
                        request_body = generate_mock_value(schema)
                        break

            endpoints.append((method, path, summary, params, request_body))

    return endpoints


# ── 测试执行 ──────────────────────────────────────────────────────────────


def run_test(
    method: str,
    path: str,
    summary: str,
    params: dict,
    request_body: object,
    base_url: str,
    global_headers: dict,
    timeout: int,
    spec: dict,
) -> dict:
    """
    执行单个接口测试。
    返回 dict 包含测试结果。
    """
    # 替换路径参数
    real_path = path
    path_params = params.get("path", {})
    for pname, pval in path_params.items():
        real_path = real_path.replace(f"{{{pname}}}", str(pval))

    url = f"{base_url}{real_path}" if base_url else real_path

    # 构建请求参数
    req_kwargs = {
        "timeout": timeout,
        "headers": dict(global_headers),
    }

    # 添加 query 参数
    query_params = params.get("query", {})
    if query_params:
        req_kwargs["params"] = query_params

    # 添加 header 参数
    header_params = params.get("header", {})
    req_kwargs["headers"].update(header_params)

    # 添加请求体
    if request_body is not None and method in ("post", "put", "patch"):
        req_kwargs["json"] = request_body

    # 执行请求
    start_time = time.time()
    result = {
        "method": method.upper(),
        "path": path,
        "summary": summary,
        "url": url,
        "status": "pending",
        "status_code": None,
        "response_time_ms": None,
        "response_body": None,
        "error": None,
        "validation": None,
    }

    try:
        resp = requests.request(method, url, **req_kwargs)
        elapsed_ms = round((time.time() - start_time) * 1000, 2)

        result["status_code"] = resp.status_code
        result["response_time_ms"] = elapsed_ms
        result["response_body"] = resp.text[:2000]  # 截断长响应

        # 判断状态码是否成功（2xx）
        if 200 <= resp.status_code < 300:
            result["status"] = "passed"
        elif 400 <= resp.status_code < 500:
            result["status"] = "failed"
            result["error"] = f"客户端错误 {resp.status_code}：{resp.reason}"
        else:
            result["status"] = "failed"
            result["error"] = f"服务端错误 {resp.status_code}：{resp.reason}"

        # 尝试 JSON 响应校验
        try:
            resp_json = resp.json()
            result["response_body"] = json.dumps(resp_json, ensure_ascii=False, indent=2)[:2000]
        except (json.JSONDecodeError, ValueError):
            pass

    except requests.exceptions.Timeout:
        elapsed_ms = round((time.time() - start_time) * 1000, 2)
        result["status"] = "error"
        result["error"] = f"请求超时（{timeout}s）"
        result["response_time_ms"] = elapsed_ms
    except requests.exceptions.ConnectionError as e:
        result["status"] = "error"
        result["error"] = f"连接失败：{e}"
    except Exception as e:
        result["status"] = "error"
        result["error"] = f"异常：{traceback.format_exc()[:500]}"

    return result


# ── 报告生成 ──────────────────────────────────────────────────────────────


def generate_report(all_results: list, spec_url: str, base_url: str, total_time: float) -> str:
    """生成 Markdown 格式的测试报告"""
    total = len(all_results)
    passed = sum(1 for r in all_results if r["status"] == "passed")
    failed = sum(1 for r in all_results if r["status"] == "failed")
    errors = sum(1 for r in all_results if r["status"] == "error")

    response_times = [r["response_time_ms"] for r in all_results if r["response_time_ms"] is not None]
    avg_time = round(sum(response_times) / len(response_times), 2) if response_times else 0

    lines = []
    lines.append("# API 接口测试报告\n")
    lines.append(f"**生成时间**：{datetime.now().strftime('%Y-%m-%d %H:%M:%S')}\n")
    lines.append(f"**规范地址**：`{spec_url}`\n")
    lines.append(f"**Base URL**：`{base_url}`\n\n")
    lines.append("---\n")
    lines.append("## 测试概况\n\n")
    lines.append(f"| 指标 | 数值 |\n| --- | --- |\n")
    lines.append(f"| 总接口数 | {total} |\n")
    lines.append(f"| ✅ 通过 | {passed} |\n")
    lines.append(f"| ❌ 失败 | {failed} |\n")
    lines.append(f"| ⚠️ 错误 | {errors} |\n")
    lines.append(f"| 平均响应时间 | {avg_time} ms |\n")
    lines.append(f"| 总耗时 | {round(total_time, 2)} s |\n\n")
    lines.append("---\n")
    lines.append("## 详细结果\n\n")

    for i, r in enumerate(all_results, 1):
        status_icon = {"passed": "✅", "failed": "❌", "error": "⚠️", "pending": "⬜"}.get(r["status"], "❓")
        lines.append(f"### {i}. {status_icon} {r['method']} {r['path']}\n")
        lines.append(f"- **描述**：{r['summary']}\n")
        lines.append(f"- **请求 URL**：`{r['url']}`\n")
        lines.append(f"- **状态码**：{r['status_code'] or 'N/A'}\n")
        lines.append(f"- **响应时间**：{r['response_time_ms'] or 'N/A'} ms\n")

        if r["status"] == "passed":
            lines.append(f"- **结果**：✅ 通过\n")
        elif r["status"] == "failed":
            lines.append(f"- **结果**：❌ 失败\n")
            lines.append(f"- **错误**：{r['error']}\n")
        else:
            lines.append(f"- **结果**：⚠️ 错误\n")
            lines.append(f"- **错误**：{r['error']}\n")

        if r["response_body"]:
            # 截断显示
            body_snippet = r["response_body"]
            if len(body_snippet) > 500:
                body_snippet = body_snippet[:500] + "\n...（截断）"
            lines.append(f"- **响应体**：\n```\n{body_snippet}\n```\n")

        lines.append("")

    lines.append("---\n")
    lines.append("## 风险提示\n\n")
    if any(r["method"] in ("POST", "PUT", "DELETE", "PATCH") for r in all_results):
        lines.append("> ⚠️ **注意**：本次测试包含写操作（POST/PUT/DELETE/PATCH），")
        lines.append("可能会对服务端数据产生实际影响。\n\n")
    lines.append("> 自动生成的 mock 数据可能不完全符合业务语义，")
    lines.append("建议在关键接口上使用自定义请求体验证。\n")

    return "\n".join(lines)


# ── 主流程 ──────────────────────────────────────────────────────────────


def main():
    parser = argparse.ArgumentParser(description="API 接口测试工具 - 基于 Swagger/OpenAPI 规范")
    parser.add_argument("--spec", required=True, help="Swagger/OpenAPI 规范的 URL 或本地文件路径")
    parser.add_argument("--base-url", default=None, help="覆盖规范中的 Base URL")
    parser.add_argument("--paths", default=None, help="逗号分隔的路径过滤，如 /pet,/store")
    parser.add_argument("--headers", default=None, help="全局请求头，格式 k1:v1,k2:v2")
    parser.add_argument("--timeout", type=int, default=30, help="每个请求的超时秒数，默认 30")
    parser.add_argument("--output", default=None, help="测试报告输出路径，默认 scripts/test_report.md")
    args = parser.parse_args()

    # 解析全局头
    global_headers = {}
    if args.headers:
        for h in args.headers.split(","):
            if ":" in h:
                k, v = h.split(":", 1)
                global_headers[k.strip()] = v.strip()

    # 路径过滤
    path_filter = None
    if args.paths:
        path_filter = [p.strip() for p in args.paths.split(",")]

    # 输出路径
    script_dir = os.path.dirname(os.path.abspath(__file__))
    output_path = args.output or os.path.join(script_dir, "test_report.md")

    print(f"🔍 正在加载规范：{args.spec}")
    spec = parse_spec(args.spec)
    version = detect_openapi_version(spec)
    print(f"📋 检测到 OpenAPI 版本：{version}")

    base_url = resolve_base_url(spec, version, args.base_url)
    print(f"🌐 Base URL：{base_url}")

    if not base_url:
        print("⚠️ 未检测到 Base URL，请通过 --base-url 手动指定")
        sys.exit(1)

    print(f"📦 正在提取接口端点...")
    endpoints = extract_all_endpoints(spec, version, path_filter)
    print(f"📦 共发现 {len(endpoints)} 个接口")

    if not endpoints:
        print("❌ 未找到任何接口，请检查规范内容")
        sys.exit(1)

    # 逐个执行测试
    all_results = []
    start_total = time.time()

    for i, (method, path, summary, params, request_body) in enumerate(endpoints, 1):
        print(f"  [{i}/{len(endpoints)}] {method.upper()} {path} ... ", end="", flush=True)
        result = run_test(method, path, summary, params, request_body, base_url, global_headers, args.timeout, spec)
        all_results.append(result)
        status_icon = {"passed": "✅", "failed": "❌", "error": "⚠️"}.get(result["status"], "❓")
        status_code = result["status_code"] or "ERR"
        time_ms = result["response_time_ms"] or "?"
        print(f"{status_icon} {status_code} ({time_ms} ms)")

    total_time = time.time() - start_total

    # 生成报告
    print(f"\n📝 正在生成测试报告...")
    report = generate_report(all_results, args.spec, base_url, total_time)
    with open(output_path, "w", encoding="utf-8") as f:
        f.write(report)
    print(f"✅ 测试报告已写入：{output_path}")

    # 输出摘要
    passed = sum(1 for r in all_results if r["status"] == "passed")
    failed = sum(1 for r in all_results if r["status"] == "failed")
    errors = sum(1 for r in all_results if r["status"] == "error")
    print(f"\n{'='*50}")
    print(f"总接口: {len(endpoints)} | ✅ 通过: {passed} | ❌ 失败: {failed} | ⚠️ 错误: {errors}")
    print(f"总耗时: {round(total_time, 2)} s")
    print(f"{'='*50}")


if __name__ == "__main__":
    main()