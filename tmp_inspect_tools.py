from pathlib import Path

p = Path("src-tauri/src/tool_executor.rs")
lines = p.read_text(encoding="utf-8").splitlines()

print("===== tool names in tool_specs =====")
for i, l in enumerate(lines, 1):
    if '"name":' in l and 900 <= i <= 2500:
        print(f"{i}|{l.strip()}")

print("\n===== dispatch / execute markers =====")
keys = (
    "match tool",
    "tool_name",
    '"memory_search"',
    '"list_directory"',
    '"read_file"',
    "execute_tool",
    "fn execute",
    "async fn run_",
    '"code_review"',
    '"apply_patch"',
    "\"shell\"",
    "\"shell_command\"",
)
for i, l in enumerate(lines, 1):
    if any(k in l for k in keys) and i < 4500:
        print(f"{i}|{l.strip()[:220]}")

print("\n===== tool_specs end region =====")
for i in range(2300, 2550):
    if i < len(lines):
        print(f"{i+1}|{lines[i]}")

print("\n===== execute match region =====")
for i in range(2550, 2850):
    if i < len(lines):
        print(f"{i+1}|{lines[i]}")
