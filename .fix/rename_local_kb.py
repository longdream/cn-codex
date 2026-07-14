# -*- coding: utf-8 -*-
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def replace_in_file(path: Path, replacements: list[tuple[str, str]]) -> int:
    text = path.read_text(encoding="utf-8")
    original = text
    for old, new in replacements:
        text = text.replace(old, new)
    if text != original:
        path.write_text(text, encoding="utf-8", newline="\n")
        return sum(original.count(old) for old, _ in replacements)
    return 0


def main() -> None:
    zh = ROOT / "src" / "i18n" / "zh-CN" / "common.json"
    en = ROOT / "src" / "i18n" / "en-US" / "common.json"
    prompt = ROOT / "src-tauri" / "src" / "agent" / "prompt_context.rs"
    prompts = ROOT / "src-tauri" / "src" / "smartbrain" / "prompts.rs"
    db_query = ROOT / "src-tauri" / "src" / "smartbrain" / "db_query.rs"
    commands = ROOT / "src-tauri" / "src" / "smartbrain" / "commands.rs"
    skill = ROOT / "codey" / "skills" / "smartbrain-context-read" / "SKILL.md"

    zh_replacements = [
        ("融合智脑经验系统与知识库", "融合本地知识库经验系统与知识库"),
        ("智脑经验提取中...", "本地知识库经验提取中..."),
        ("智脑经验提取中（{current}/{total}）", "本地知识库经验提取中（{current}/{total}）"),
        ('"settings.smartbrain": "智脑"', '"settings.smartbrain": "本地知识库"'),
        ("开启智脑后，是否立即检查并提取所有历史对话中的经验？", "开启本地知识库后，是否立即检查并提取所有历史对话中的经验？"),
        ('"settings.smartbrain.enableModal.title": "开启智脑"', '"settings.smartbrain.enableModal.title": "开启本地知识库"'),
        ("智脑功能未启用。请前往「通用设置」开启智脑后方可使用此功能。", "本地知识库功能未启用。请前往「通用设置」开启本地知识库后方可使用此功能。"),
        ("智脑已启用。可在「通用设置」中管理开关。", "本地知识库已启用。可在「通用设置」中管理开关。"),
        ("管理可供智脑检索的数据库连接。建议始终使用只读账号。", "管理可供本地知识库检索的数据库连接。建议始终使用只读账号。"),
        ("该数据库可参与智脑检索与后续安全查询。", "该数据库可参与本地知识库检索与后续安全查询。"),
        ("后续数据库工具或智脑检索可直接读取这些规则。", "后续数据库工具或本地知识库检索可直接读取这些规则。"),
        ("加入智脑知识库", "加入本地知识库"),
        ("已加入智脑", "已加入本地知识库"),
    ]

    en_replacements = [
        ("featuring a SmartBrain experience system and knowledge base", "featuring a Local Knowledge Base experience system and knowledge base"),
        ("Extracting SmartBrain experiences...", "Extracting Local Knowledge Base experiences..."),
        ("Extracting SmartBrain experiences ({current}/{total})", "Extracting Local Knowledge Base experiences ({current}/{total})"),
        ('"settings.smartbrain": "SmartBrain"', '"settings.smartbrain": "Local Knowledge Base"'),
        ("SmartBrain is being enabled.", "Local Knowledge Base is being enabled."),
        ('"settings.smartbrain.enableModal.title": "Enable SmartBrain"', '"settings.smartbrain.enableModal.title": "Enable Local Knowledge Base"'),
        ("When enabling SmartBrain,", "When enabling Local Knowledge Base,"),
        ("SmartBrain is not enabled.", "Local Knowledge Base is not enabled."),
        ("SmartBrain is enabled.", "Local Knowledge Base is enabled."),
        ("Manage database connections that SmartBrain can reference.", "Manage database connections that Local Knowledge Base can reference."),
        ("This database can participate in SmartBrain retrieval and future safe queries.", "This database can participate in Local Knowledge Base retrieval and future safe queries."),
        ("future database tools or SmartBrain retrieval can consume it directly.", "future database tools or Local Knowledge Base retrieval can consume it directly."),
        ("Add to SmartBrain", "Add to Local Knowledge Base"),
        ("Added to SmartBrain", "Added to Local Knowledge Base"),
    ]

    prompt_replacements = [
        (
            "你已经有一组通过智脑配置好的数据库连接。它们属于“智脑”上下文的一部分，不要把它们当作缺失信息。",
            "你已经有一组通过本地知识库配置好的数据库连接。它们属于“本地知识库”上下文的一部分，不要把它们当作缺失信息。",
        ),
        (
            "不要用 Python/shell 手写连接脚本执行 SQL，也不要让用户再次提供密码；密码已保存在智脑数据库配置中。",
            "不要用 Python/shell 手写连接脚本执行 SQL，也不要让用户再次提供密码；密码已保存在本地知识库数据库配置中。",
        ),
        (
            "\\n\\n## SmartBrain (智脑)\\n\\n{}\\n\\n\\\n             When you apply knowledge from SmartBrain, note which experience, knowledge, or database configuration helped.",
            "\\n\\n## Local Knowledge Base (本地知识库)\\n\\n{}\\n\\n\\\n             When you apply knowledge from Local Knowledge Base, note which experience, knowledge, or database configuration helped.",
        ),
    ]

    prompts_replacements = [
        (
            "when SmartBrain is enabled.",
            "when Local Knowledge Base is enabled.",
        )
    ]

    db_query_replacements = [
        (
            "没有可用的智脑数据库配置。请先在设置 → 智脑 → 数据库中配置并启用至少一个数据源。",
            "没有可用的本地知识库数据库配置。请先在设置 → 本地知识库 → 数据库中配置并启用至少一个数据源。",
        ),
        (
            "未找到名为 `{raw_name}` 的智脑数据库配置。可用数据库：{names}",
            "未找到名为 `{raw_name}` 的本地知识库数据库配置。可用数据库：{names}",
        ),
        (
            "请确认智脑数据库配置完整（host/port/database/username/password）且网络可达。\\",
            "请确认本地知识库数据库配置完整（host/port/database/username/password）且网络可达。\\",
        ),
        (
            "SmartBrain SQL 查询成功\\n数据库: {}\\n类型: {}\\n返回行数: {}{}\\nSQL:\\n{}\\n\\n",
            "本地知识库 SQL 查询成功\\n数据库: {}\\n类型: {}\\n返回行数: {}{}\\nSQL:\\n{}\\n\\n",
        ),
        ("//! Built-in SmartBrain database SQL execution.", "//! Built-in Local Knowledge Base database SQL execution."),
        ("//! Uses configured SmartBrain DB sources (host/user/password already stored)", "//! Uses configured Local Knowledge Base DB sources (host/user/password already stored)"),
    ]

    commands_replacements = [
        (
            '"error": "SmartBrain is disabled. Enable it in Settings.",',
            '"error": "Local Knowledge Base is disabled. Enable it in Settings.",',
        )
    ]

    skill_replacements = [
        ("SmartBrain 检索命中后的连续上下文读取规范", "本地知识库检索命中后的连续上下文读取规范"),
        ("# SmartBrain 连续上下文读取", "# 本地知识库 连续上下文读取"),
    ]

    total = 0
    for path, reps in [
        (zh, zh_replacements),
        (en, en_replacements),
        (prompt, prompt_replacements),
        (prompts, prompts_replacements),
        (db_query, db_query_replacements),
        (commands, commands_replacements),
        (skill, skill_replacements),
    ]:
        count = replace_in_file(path, reps)
        print(f"{path.relative_to(ROOT)}: {count} replacements")
        total += count
    print(f"total: {total}")


if __name__ == "__main__":
    main()
