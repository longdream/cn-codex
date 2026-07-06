use super::WorkflowDef;

/// Generate SKILL.md content from a workflow definition.
/// This file enables the workflow to be discovered and triggered like a skill.
pub fn generate_skill_md(def: &WorkflowDef, snapshot_paths: &[String]) -> String {
    let mut out = String::new();

    // YAML frontmatter
    out.push_str("---\n");
    out.push_str(&format!("name: {}\n", def.title));
    out.push_str(&format!("description: {}\n", def.description));
    if !def.trigger_phrases.is_empty() {
        let tags: Vec<String> = def
            .trigger_phrases
            .iter()
            .take(5)
            .map(|t| format!("\"{}\"", t.replace('"', "'")))
            .collect();
        out.push_str(&format!("tags: [{}]\n", tags.join(", ")));
    }
    out.push_str("---\n\n");

    // Title and description
    out.push_str(&format!("# {}\n\n", def.title));
    out.push_str(&format!("{}\n\n", def.description));

    // Instructions for AI
    out.push_str(
        "这是一个结构化 Workflow，请严格按以下节点顺序执行。\
         每完成一个节点后确认输出符合预期再继续下一个节点。\
         如果任何节点失败，停止并报告问题。\n\n",
    );

    // Variables section
    if !def.variables.is_empty() {
        out.push_str("## 变量\n\n");
        for (name, var) in &def.variables {
            let default_str = var
                .default
                .as_ref()
                .map(|d| format!(" (默认: {d})"))
                .unwrap_or_default();
            out.push_str(&format!(
                "- `{{{{{name}}}}}`: {}{default_str}\n",
                var.description
            ));
        }
        out.push_str("\n");
    }

    // 脚本快照 section：
    // - 保存 workflow 时会自动把命中的脚本复制到 workflow 目录下的 scripts/；
    // - 在 SKILL 中显式列出，提示执行时优先复用，避免重复造轮子。
    if !snapshot_paths.is_empty() {
        out.push_str("## 脚本快照\n\n");
        out.push_str("以下脚本已在保存时自动快照，执行时请优先复用这些脚本：\n");
        for snapshot_path in snapshot_paths {
            out.push_str(&format!("- `{snapshot_path}`\n"));
        }
        out.push_str("\n");
    }

    // Nodes section
    out.push_str("## 执行节点\n\n");
    for (i, node) in def.nodes.iter().enumerate() {
        out.push_str(&format!("### Node {}: {}\n\n", i + 1, node.objective));
        out.push_str(&format!("- **目标**: {}\n", node.objective));
        out.push_str(&format!("- **工具**: {}\n", node.tools.join(", ")));

        if let Some(hints) = &node.args_hints {
            if let Some(obj) = hints.as_object() {
                for (tool, args) in obj {
                    if let Some(args_obj) = args.as_object() {
                        for (key, val) in args_obj {
                            let val_str = match val.as_str() {
                                Some(s) => s.to_string(),
                                None => val.to_string(),
                            };
                            out.push_str(&format!("- **{tool}.{key}**: `{val_str}`\n"));
                        }
                    }
                }
            }
        }

        if let Some(expected) = &node.expected_output {
            out.push_str(&format!("- **期望输出**: {expected}\n"));
        }
        if let Some(budget) = node.token_budget {
            out.push_str(&format!("- **Token 预算**: {budget}\n"));
        }

        // 已知陷阱（反例经验）
        if let Some(failures) = &node.known_failures {
            if !failures.is_empty() {
                out.push_str("\n#### ⚠️ 已知陷阱\n\n");
                for (j, failure) in failures.iter().enumerate() {
                    out.push_str(&format!("**陷阱 {}**: {}\n", j + 1, failure.error));
                    out.push_str(&format!("- 原因: {}\n", failure.cause));
                    out.push_str(&format!("- 正确做法: {}\n\n", failure.fix));
                }
            }
        }

        out.push('\n');
    }

    // Footer
    out.push_str("## 执行规则\n\n");
    out.push_str("- 按节点顺序逐个执行，不要跳过\n");
    out.push_str("- 每个节点只使用该节点列出的工具\n");
    out.push_str("- 将变量 `{{...}}` 替换为用户提供的实际值（或使用默认值）\n");
    out.push_str("- 如果 `scripts/` 下已有对应脚本，优先直接复用，避免重复编写同类脚本\n");
    out.push_str("- 保持输出简洁，节约 token\n");

    out
}
