from pathlib import Path

p = Path("src-tauri/src/tool_executor.rs")
lines = p.read_text(encoding="utf-8").splitlines()

print("===== tool_search / catalog =====")
for i, l in enumerate(lines, 1):
    if "tool_search" in l or "TOOL_CATALOG" in l or "builtin tool" in l.lower() or "search tools" in l.lower():
        print(f"{i}|{l.strip()[:220]}")

print("\n===== tool_specs test =====")
start = None
for i, l in enumerate(lines):
    if "fn tool_specs_include_web_tools_only_when_enabled" in l:
        start = i
        break
if start is not None:
    for j in range(start, min(start + 80, len(lines))):
        print(f"{j+1}|{lines[j]}")

print("\n===== exec_list_dir =====")
start = None
for i, l in enumerate(lines):
    if "async fn exec_list_dir" in l:
        start = i
        break
if start is not None:
    for j in range(start, min(start + 70, len(lines))):
        print(f"{j+1}|{lines[j]}")

# agent prompt tools mention
print("\n===== agent prompt tool mentions =====")
for pth in Path("src-tauri/src").rglob("*.rs"):
    text = pth.read_text(encoding="utf-8", errors="ignore")
    if "Available tools" in text or "read_file" in text and "list_directory" in text and "shell" in text:
        hits = []
        for i, line in enumerate(text.splitlines(), 1):
            if any(k in line for k in ["Available tools", "read_file", "list_directory", "code_search", "ripgrep", "Prefer", "tool"]):
                if any(k in line for k in ["read_file", "list_directory", "code_search", "Available", "shell", "rg", "search code"]):
                    hits.append((i, line.strip()[:200]))
        if hits:
            print(pth)
            for i, line in hits[:40]:
                print(f"  {i}|{line}")
