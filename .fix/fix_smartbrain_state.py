from pathlib import Path

path = Path("src/components/settings/smartbrainDatabaseState.ts")
text = path.read_text(encoding="utf-8")
replacements = [
    (
        'typeof existing.port === "number"; Number.isFinite(existing.port)',
        'typeof existing.port === "number" && Number.isFinite(existing.port)',
    ),
    (
        "existing.queryParams; Object.keys(existing.queryParams).length > 0",
        "existing.queryParams && Object.keys(existing.queryParams).length > 0",
    ),
    (
        'typeof item === "string"; item.trim().length > 0',
        'typeof item === "string" && item.trim().length > 0',
    ),
]
for old, new in replacements:
    if old not in text:
        raise SystemExit(f"missing snippet: {old}")
    text = text.replace(old, new)
path.write_text(text, encoding="utf-8")
print("state fixed")
