import sys
import json

try:
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
except Exception:
    pass

print("步骤 1: 测试输出", flush=True)
print("步骤 2: 中文输出测试", flush=True)
result = {"ok": True, "step": 2, "url": "https://example.com", "title": "test"}
print(f"REPLAY_RESULT: {json.dumps(result, ensure_ascii=False)}", flush=True)