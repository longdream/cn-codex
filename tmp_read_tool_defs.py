from pathlib import Path
import sys
sys.stdout.reconfigure(encoding="utf-8", errors="replace")

p = Path("src-tauri/src/tool_executor.rs")
lines = p.read_text(encoding="utf-8").splitlines()

# tool schema section around 900-1500
print("===== tool schemas start =====")
for i in range(880, 1520):
    print(f"{i+1}|{lines[i]}")

print("\n===== exec dispatch =====")
for i in range(2580, 2680):
    print(f"{i+1}|{lines[i]}")

print("\n===== shell exec =====")
for i in range(2880, 3160):
    print(f"{i+1}|{lines[i]}")

# resource path resolution elsewhere
print("\n===== resource path patterns =====")
for pth in Path("src-tauri/src").rglob("*.rs"):
    text = pth.read_text(encoding="utf-8", errors="ignore")
    if "resource_dir" in text or "current_exe" in text or "sidecar" in text or "external_bin" in text:
        print(pth)
        for i, line in enumerate(text.splitlines(), 1):
            if any(k in line for k in ["resource_dir", "current_exe", "sidecar", "external_bin", "resources/"]):
                print(f"  {i}|{line.strip()[:200]}")
