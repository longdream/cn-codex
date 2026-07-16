from pathlib import Path

p = Path("src-tauri/src/tool_executor.rs")
lines = p.read_text(encoding="utf-8").splitlines()

print("===== memory_search args + exec =====")
for i, l in enumerate(lines, 1):
    if "MemorySearch" in l or "exec_memory_search" in l or "struct Memory" in l:
        print(f"{i}|{l.strip()[:220]}")

print("\n===== list_directory region =====")
for i in range(1460, 1520):
    print(f"{i+1}|{lines[i]}")

print("\n===== exec list_directory / path resolve =====")
for i, l in enumerate(lines, 1):
    if any(k in l for k in [
        "fn exec_list_directory",
        "fn exec_memory_search",
        "fn resolve_workspace",
        "fn resolve_path",
        "fn sanitize_path",
        "fn ensure_within",
        "fn resolve_command_cwd",
        "fn normalize_path",
        "path_within",
        "within_workspace",
        "fn exec_read_file",
    ]):
        print(f"{i}|{l.strip()[:220]}")

print("\n===== list_directory impl =====")
start = None
for i, l in enumerate(lines):
    if "async fn exec_list_directory" in l:
        start = i
        break
if start is not None:
    for j in range(start, min(start + 120, len(lines))):
        print(f"{j+1}|{lines[j]}")

print("\n===== read_file impl head =====")
start = None
for i, l in enumerate(lines):
    if "async fn exec_read_file" in l:
        start = i
        break
if start is not None:
    for j in range(start, min(start + 80, len(lines))):
        print(f"{j+1}|{lines[j]}")

# package scripts for resources
print("\n===== packaging scripts mentioning resources/ocr =====")
for pth in list(Path("scripts").rglob("*")) + list(Path(".").glob("*.bat")) + list(Path(".").glob("*.ps1")) + list(Path("docs").rglob("*")):
    if pth.is_file():
        try:
            text = pth.read_text(encoding="utf-8", errors="ignore")
        except Exception:
            continue
        if "resources" in text or "ocr" in text or "rg.exe" in text or "externalBin" in text:
            print(pth)
            for i, line in enumerate(text.splitlines(), 1):
                if any(k in line for k in ["resources", "ocr", "rg.exe", "externalBin", "sidecar"]):
                    print(f"  {i}|{line.strip()[:200]}")
