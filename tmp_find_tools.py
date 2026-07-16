from pathlib import Path
import sys
sys.stdout.reconfigure(encoding="utf-8", errors="replace")

# Find tool definitions and shell execution paths
needles = [
    "shell_command", "ShellCommand", "function_call", "tool_defs", "ToolSpec",
    "available_tools", "tool_list", "register_tool", "\"shell\"", "grep",
    "code_search", "list_dir", "read_file", "apply_patch", "tool_executor",
    "ToolDefinition", "tools_for", "builtin_tools",
]

files = list(Path("src-tauri/src").rglob("*.rs"))
for p in files:
    text = p.read_text(encoding="utf-8", errors="ignore")
    hits = []
    for i, line in enumerate(text.splitlines(), 1):
        low = line
        if any(n in low for n in needles):
            hits.append((i, line.strip()[:220]))
    if hits:
        print(f"\n=== {p} ({len(hits)} hits) ===")
        for i, line in hits[:50]:
            print(f"{i}|{line}")
