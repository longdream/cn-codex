from pathlib import Path
al = Path("src-tauri/src/agent.rs").read_text(encoding="utf-8").splitlines()
print("===== core tools list =====")
for i in range(800, 880):
    print(f"{i+1}|{al[i]}")
print("\n===== tools prompt start =====")
for i in range(2430, 2465):
    print(f"{i+1}|{al[i]}")
