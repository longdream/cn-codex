from pathlib import Path

p = Path("src-tauri/src/tool_executor.rs")
lines = p.read_text(encoding="utf-8").splitlines()

start = None
for i, l in enumerate(lines):
    if "async fn exec_ocr_image" in l:
        start = i
        break
if start is not None:
    for j in range(start, min(start + 100, len(lines))):
        print(f"{j+1}|{lines[j]}")

print("\n===== project_root helpers =====")
for i, l in enumerate(lines, 1):
    if "project_root" in l or "workspace_root" in l or "fn resolve_" in l and "root" in l:
        if i < 12000:
            print(f"{i}|{l.strip()[:220]}")

print("\n===== ToolExecutor::new =====")
for i, l in enumerate(lines, 1):
    if "impl ToolExecutor" in l or "fn new(" in l or "with_workspace_config_dir" in l or "self.cwd" in l and "fn " in l:
        if i < 900:
            print(f"{i}|{l.strip()[:220]}")

# agent system prompt section around tools
ap = Path("src-tauri/src/agent.rs")
al = ap.read_text(encoding="utf-8").splitlines()
print("\n===== agent tools prompt =====")
for i in range(2420, 2520):
    print(f"{i+1}|{al[i]}")
