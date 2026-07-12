from pathlib import Path

path = Path(r"src-tauri/src/smartbrain/commands.rs")
lines = path.read_text(encoding="utf-8").splitlines()
for i, line in enumerate(lines):
    if ".rsplit(['/'," in line:
        lines[i] = "                .rsplit(['/', '\\\\'])"
        print(f"fixed line {i+1}: {lines[i]!r}")
path.write_text("\n".join(lines) + "\n", encoding="utf-8")
