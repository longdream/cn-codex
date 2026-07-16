from pathlib import Path

p = Path("src-tauri/src/tool_executor.rs")
lines = p.read_text(encoding="utf-8").splitlines()

print("===== dispatch around list_directory =====")
for i in range(2595, 2680):
    print(f"{i+1}|{lines[i]}")

print("\n===== search exec_list / list dir function =====")
for i, l in enumerate(lines, 1):
    if "list_directory" in l or "ListDir" in l or "exec_list" in l:
        print(f"{i}|{l.strip()[:220]}")

print("\n===== memory_search impl =====")
start = None
for i, l in enumerate(lines):
    if "async fn exec_memory_search" in l:
        start = i
        break
if start is not None:
    for j in range(start, min(start + 120, len(lines))):
        print(f"{j+1}|{lines[j]}")

print("\n===== resolve_command_cwd =====")
start = None
for i, l in enumerate(lines):
    if "fn resolve_command_cwd" in l:
        start = i
        break
if start is not None:
    for j in range(start, min(start + 40, len(lines))):
        print(f"{j+1}|{lines[j]}")

print("\n===== truncate_output =====")
for i, l in enumerate(lines, 1):
    if "fn truncate_output" in l:
        for j in range(i - 1, min(i + 30, len(lines))):
            print(f"{j+1}|{lines[j]}")
        break

# release-portable section for resources
print("\n===== release-portable OCR copy section =====")
rp = Path("scripts/release-portable.ps1")
rpl = rp.read_text(encoding="utf-8").splitlines()
for i, l in enumerate(rpl, 1):
    if "OCR" in l or "resources" in l.lower() or "Copy-Directory" in l or "RequiredOcr" in l:
        print(f"{i}|{l.rstrip()[:220]}")
