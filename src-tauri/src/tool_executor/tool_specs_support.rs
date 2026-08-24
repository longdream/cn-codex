use super::*;

impl ToolExecutor {
    pub fn tool_specs(&self, web_search_enabled: bool) -> Vec<serde_json::Value> {
        let mut tools = vec![
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "shell",
                    "description": "Runs one complete shell script and returns its output. On Windows the script must be valid PowerShell with every pipeline starting from an input-producing command. Supports workdir, timeout_ms (or block_until_ms), login, and sandbox permission approval fields.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "command": {
                                "type": "string",
                                "minLength": 1,
                                "description": "One complete shell script. On Windows use valid PowerShell syntax, provide an input command before every pipeline, and use $_ as the current pipeline object."
                            },
                            "workdir": {
                                "type": "string",
                                "description": "Working directory for the command. Defaults to the turn cwd."
                            },
                            "timeout_ms": {
                                "type": "integer",
                                "minimum": 1000,
                                "maximum": 3600000,
                                "description": "Maximum command runtime in milliseconds. Defaults to 30000 ms. Valid range: 1000-3600000."
                            },
                            "block_until_ms": {
                                "type": "integer",
                                "minimum": 1000,
                                "maximum": 3600000,
                                "description": "Compatibility alias for timeout_ms."
                            },
                            "login": {
                                "type": "boolean",
                                "description": "True runs with login/default shell semantics; false disables profile/login behavior where supported. Defaults to true."
                            },
                            "sandbox_permissions": {
                                "type": "string",
                                "enum": ["use_default", "with_additional_permissions", "require_escalated"],
                                "description": "Per-command permission override. Defaults to use_default; require_escalated or with_additional_permissions asks the user before running."
                            },
                            "justification": {
                                "type": "string",
                                "description": "User-facing approval reason for sandbox_permissions overrides."
                            },
                            "prefix_rule": {
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "Reusable approval prefix for the command, only with sandbox_permissions: require_escalated."
                            },
                            "additional_permissions": {
                                "type": "object",
                                "description": "Sandboxed filesystem or network access for this command; only with sandbox_permissions: with_additional_permissions."
                            }
                        },
                        "required": ["command"]
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "shell_command",
                    "description": "Codex-compatible shell tool. Runs one complete PowerShell script on Windows or one shell script on Unix and returns output. Every PowerShell pipeline must start from an input-producing command, and Where-Object/ForEach-Object use $_ as the current object. Supports workdir, timeout_ms (or block_until_ms), login, sandbox_permissions, justification, prefix_rule, and additional_permissions.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "command": {
                                "type": "string",
                                "minLength": 1,
                                "description": "One complete shell script. On Windows use valid PowerShell syntax, provide an input command before every pipeline, and use $_ as the current pipeline object."
                            },
                            "workdir": {
                                "type": "string",
                                "description": "Working directory for the command. Defaults to the turn cwd."
                            },
                            "timeout_ms": {
                                "type": "integer",
                                "minimum": 1000,
                                "maximum": 3600000,
                                "description": "Maximum command runtime in milliseconds. Defaults to 30000 ms. Valid range: 1000-3600000."
                            },
                            "block_until_ms": {
                                "type": "integer",
                                "minimum": 1000,
                                "maximum": 3600000,
                                "description": "Compatibility alias for timeout_ms."
                            },
                            "login": {
                                "type": "boolean",
                                "description": "True runs with login/default shell semantics; false disables profile/login behavior where supported. Defaults to true."
                            },
                            "sandbox_permissions": {
                                "type": "string",
                                "enum": ["use_default", "with_additional_permissions", "require_escalated"],
                                "description": "Per-command permission override. Defaults to use_default; require_escalated or with_additional_permissions asks the user before running."
                            },
                            "justification": {
                                "type": "string",
                                "description": "User-facing approval reason for sandbox_permissions overrides."
                            },
                            "prefix_rule": {
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "Reusable approval prefix for the command, only with sandbox_permissions: require_escalated."
                            },
                            "additional_permissions": {
                                "type": "object",
                                "description": "Sandboxed filesystem or network access for this command; only with sandbox_permissions: with_additional_permissions."
                            }
                        },
                        "required": ["command"]
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "exec_command",
                    "description": "Run a command as a persistent exec session. Returns output and, when the process is still running after yield_time_ms, a session_id that can be passed to write_stdin for input or polling. This is CN-Codex's lightweight equivalent of Codex unified exec sessions.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "cmd": {
                                "type": "string",
                                "minLength": 1,
                                "description": "Shell command to execute."
                            },
                            "workdir": {
                                "type": "string",
                                "description": "Working directory for the command. Defaults to the turn cwd."
                            },
                            "shell": {
                                "type": "string",
                                "description": "Optional shell binary to launch. Defaults to PowerShell on Windows and SHELL/sh on Unix."
                            },
                            "login": {
                                "type": "boolean",
                                "description": "True runs with login/default shell semantics where supported; false disables profile/login behavior. Defaults to true."
                            },
                            "yield_time_ms": {
                                "type": "integer",
                                "minimum": 250,
                                "maximum": 30000,
                                "description": "Wait before returning output. Defaults to 10000 ms."
                            },
                            "timeout_ms": {
                                "type": "integer",
                                "minimum": 250,
                                "maximum": 30000,
                                "description": "Compatibility alias for yield_time_ms."
                            },
                            "max_output_tokens": {
                                "type": "integer",
                                "minimum": 100,
                                "maximum": 50000,
                                "description": "Approximate output token budget. Defaults to 10000 tokens."
                            },
                            "sandbox_permissions": {
                                "type": "string",
                                "enum": ["use_default", "with_additional_permissions", "require_escalated"],
                                "description": "Per-command permission override. Defaults to use_default; require_escalated or with_additional_permissions asks the user before running."
                            },
                            "justification": {
                                "type": "string",
                                "description": "User-facing approval reason for sandbox_permissions overrides."
                            },
                            "prefix_rule": {
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "Reusable approval prefix for cmd, only with sandbox_permissions: require_escalated."
                            },
                            "additional_permissions": {
                                "type": "object",
                                "description": "Sandboxed filesystem or network access for this command; only with sandbox_permissions: with_additional_permissions."
                            }
                        },
                        "required": ["cmd"]
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "write_stdin",
                    "description": "Write characters to a running exec_command session, or poll recent output when chars is omitted.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "session_id": {
                                "type": "integer",
                                "description": "Identifier returned by exec_command."
                            },
                            "chars": {
                                "type": "string",
                                "description": "Characters to write to stdin. Omit or pass an empty string to poll output only."
                            },
                            "yield_time_ms": {
                                "type": "integer",
                                "minimum": 250,
                                "maximum": 300000,
                                "description": "Wait before returning output. Defaults to 250 ms after writes and 5000 ms for polling."
                            },
                            "max_output_tokens": {
                                "type": "integer",
                                "minimum": 100,
                                "maximum": 50000,
                                "description": "Approximate output token budget. Defaults to 10000 tokens."
                            }
                        },
                        "required": ["session_id"]
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "close_exec_session",
                    "description": "Terminate and remove a running exec_command session by session_id. Use this to stop long-running servers, hung commands, or sessions that are no longer needed.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "session_id": {
                                "type": "integer",
                                "description": "Identifier returned by exec_command."
                            }
                        },
                        "required": ["session_id"]
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "read_file",
                    "description": "Read the contents of a file at the given path. Returns a numbered page by default (max_lines defaults to 400, hard cap 2000). Use line_offset/end_line to page through large files. Prefer this over shell/python for reading source slices.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "path": {
                                "type": "string",
                                "description": "The file path to read."
                            },
                            "line_offset": {
                                "type": "integer",
                                "minimum": 1,
                                "description": "1-indexed starting line. Defaults to 1. Use with max_lines/end_line for ranged reads."
                            },
                            "max_lines": {
                                "type": "integer",
                                "minimum": 1,
                                "maximum": 2000,
                                "description": "Maximum lines to return. Defaults to 400. Large files are always returned as a numbered page; use line_offset to continue."
                            },
                            "end_line": {
                                "type": "integer",
                                "minimum": 1,
                                "description": "Optional inclusive end line. When set, overrides max_lines as (end_line - line_offset + 1)."
                            },
                            "show_line_numbers": {
                                "type": "boolean",
                                "description": "Prefix each returned line with its 1-indexed line number. Defaults to true for ranged reads and false for full-file reads."
                            }
                        },
                        "required": ["path"]
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "write_file",
                    "description": "Create a new UTF-8 text file. Existing files require overwrite=true and complete replacement content; omission placeholders and destructive truncation are rejected. For changes to an existing text file, use apply_patch so unrelated content, encoding, and line endings are preserved.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "path": {
                                "type": "string",
                                "description": "The file path to write."
                            },
                            "content": {
                                "type": "string",
                                "description": "The content to write to the file."
                            },
                            "overwrite": {
                                "type": "boolean",
                                "description": "Set to true only when the user explicitly requested a complete rewrite of an existing file. This does not bypass placeholder or destructive-truncation safeguards. Defaults to false."
                            }
                        },
                        "required": ["path", "content"]
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "tool_search",
                    "description": "Search available CN-Codex tools, local skills, plugin skills, and discovered MCP tools, then activate matching non-core tool schemas for the next model call in this same turn (and later iterations in the thread). Default turns only expose a small core tool set; use this to lazy-load MCP/Playwright and other non-core tools before calling them.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "query": {
                                "type": "string",
                                "description": "Search query for tools or skills."
                            },
                            "limit": {
                                "type": "integer",
                                "minimum": 1,
                                "maximum": 50,
                                "description": "Maximum number of matches to return. Defaults to 8."
                            }
                        },
                        "required": ["query"]
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "apps_list",
                    "description": "List plugin-declared app connectors and currently exposed trusted codex-apps MCP tools. Use this to see which imported plugin apps are installed, which connector IDs they use, and whether matching MCP tools are available.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "connector_id": {
                                "type": "string",
                                "description": "Optional connector ID to filter to one app connector."
                            },
                            "include_tools": {
                                "type": "boolean",
                                "description": "Include matching MCP tool names for each connector. Defaults to true."
                            }
                        },
                        "required": []
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "list_available_plugins_to_install",
                    "description": "List Codex plugin cache candidates that CN-Codex can import into this workspace. Use this before request_plugin_install when a requested plugin or connector is not yet available.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "query": {
                                "type": "string",
                                "description": "Optional search text matched against plugin name, description, source path, MCP servers, and app connector IDs."
                            },
                            "source_dir": {
                                "type": "string",
                                "description": "Optional Codex plugin cache directory. Defaults to the user's .cn-codex/plugins/cache directory."
                            },
                            "include_installed": {
                                "type": "boolean",
                                "description": "Include plugins already imported into codey/plugins. Defaults to true."
                            },
                            "limit": {
                                "type": "integer",
                                "minimum": 1,
                                "maximum": 100,
                                "description": "Maximum candidates to return. Defaults to 50."
                            }
                        },
                        "required": []
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "request_plugin_install",
                    "description": "Import one plugin from the local Codex plugin cache into codey/plugins. This is CN-Codex's local equivalent of Codex plugin install suggestions; it reimports/updates the plugin if already present.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "tool_id": {
                                "type": "string",
                                "description": "Candidate id returned by list_available_plugins_to_install."
                            },
                            "name": {
                                "type": "string",
                                "description": "Optional plugin name fallback when tool_id is not known."
                            },
                            "tool_type": {
                                "type": "string",
                                "enum": ["plugin"],
                                "description": "For compatibility with Codex request_plugin_install. Only plugin is supported locally."
                            },
                            "action_type": {
                                "type": "string",
                                "enum": ["install"],
                                "description": "For compatibility with Codex request_plugin_install. Only install is supported."
                            },
                            "suggest_reason": {
                                "type": "string",
                                "description": "Short reason this plugin is needed."
                            },
                            "source_dir": {
                                "type": "string",
                                "description": "Optional Codex plugin cache directory. Defaults to the user's .cn-codex/plugins/cache directory."
                            }
                        },
                        "required": []
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "plugin_manage",
                    "description": "List, enable, disable, or uninstall local workspace plugins under codey/plugins. Disabled plugins stay on disk but are excluded from skill prompts, MCP servers, app connectors, and hooks. Use uninstall only when explicitly requested.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "action": {
                                "type": "string",
                                "enum": ["list", "enable", "disable", "uninstall"],
                                "description": "Plugin management action. Defaults to list."
                            },
                            "plugin_id": {
                                "type": "string",
                                "description": "Workspace plugin id from plugin_manage list output. Required for enable, disable, and uninstall."
                            },
                            "id": {
                                "type": "string",
                                "description": "Compatibility alias for plugin_id."
                            }
                        },
                        "required": []
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "mcp_manage",
                    "description": "Install, list, enable, disable, or uninstall MCP servers in codey/config.toml so they appear in Settings > Integration. Prefer this over freeform config edits when the user asks to install an MCP server. Supports stdio (command/args) and remote (type+url) servers.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "action": {
                                "type": "string",
                                "enum": ["list", "install", "add", "enable", "disable", "uninstall", "remove"],
                                "description": "MCP management action. Defaults to list. install/add writes config.toml and refreshes Settings."
                            },
                            "name": {
                                "type": "string",
                                "description": "MCP server name, e.g. godot or playwright. Required for install/enable/disable/uninstall."
                            },
                            "server": {
                                "type": "string",
                                "description": "Compatibility alias for name."
                            },
                            "command": {
                                "type": "string",
                                "description": "stdio launch command, e.g. npx or node."
                            },
                            "args": {
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "stdio command arguments."
                            },
                            "cwd": {
                                "type": "string",
                                "description": "Optional working directory for stdio servers."
                            },
                            "env": {
                                "type": "object",
                                "additionalProperties": { "type": "string" },
                                "description": "Optional environment variables for the MCP process."
                            },
                            "url": {
                                "type": "string",
                                "description": "Remote MCP server URL for http/sse transports."
                            },
                            "type": {
                                "type": "string",
                                "description": "Transport type: stdio, sse, or http/streamable-http."
                            },
                            "headers": {
                                "type": "object",
                                "additionalProperties": { "type": "string" },
                                "description": "Optional HTTP headers for remote MCP servers."
                            },
                            "disabled": {
                                "type": "boolean",
                                "description": "Whether the server starts disabled. Defaults to false on install."
                            },
                            "config": {
                                "type": "object",
                                "description": "Optional full server config object. Merged with top-level fields."
                            },
                            "overwrite": {
                                "type": "boolean",
                                "description": "Allow overwriting an existing MCP server with the same name. Defaults to false."
                            }
                        },
                        "required": []
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "skill_manage",
                    "description": "Install, list, update, or uninstall local skills under codey/skills so they appear in Settings > Skills. Prefer this over freeform file writes when the user asks to install a skill.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "action": {
                                "type": "string",
                                "enum": ["list", "install", "create", "update", "uninstall", "remove"],
                                "description": "Skill management action. Defaults to list. install/create writes codey/skills/<id>/SKILL.md."
                            },
                            "skill_id": {
                                "type": "string",
                                "description": "Skill directory id under codey/skills (lowercase letters, digits, hyphen/underscore). Required for install/update/uninstall."
                            },
                            "id": {
                                "type": "string",
                                "description": "Compatibility alias for skill_id."
                            },
                            "name": {
                                "type": "string",
                                "description": "Optional display name written into SKILL.md frontmatter when content is omitted."
                            },
                            "description": {
                                "type": "string",
                                "description": "Optional skill description for generated SKILL.md frontmatter."
                            },
                            "content": {
                                "type": "string",
                                "description": "Full SKILL.md markdown content. If omitted for install/create, a template is generated from name/description/tags."
                            },
                            "tags": {
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "Optional tags for generated SKILL.md frontmatter."
                            },
                            "overwrite": {
                                "type": "boolean",
                                "description": "Allow overwriting an existing skill. Defaults to false for install/create, true for update."
                            }
                        },
                        "required": []
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "robot_save",
                    "description": "Create or update a Robot configuration. A Robot is a specialized AI role that binds skills to each workflow node. Available in robot-create and robot-modify modes.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "id": {
                                "type": "string",
                                "description": "Robot directory name (lowercase, hyphens, no spaces). E.g. 'code-reviewer', 'security-auditor'."
                            },
                            "config": {
                                "type": "object",
                                "description": "Complete robot.json content.",
                                "properties": {
                                    "name": { "type": "string", "description": "Human-readable robot name." },
                                    "description": { "type": "string", "description": "What this robot does." },
                                    "icon": { "type": "string", "description": "Icon identifier for the robot." },
                                    "skills": {
                                        "type": "array",
                                        "items": { "type": "string" },
                                        "description": "Local skill IDs from codey/skills/."
                                    },
                                    "pluginSkills": {
                                        "type": "array",
                                        "items": {
                                            "type": "object",
                                            "properties": {
                                                "pluginId": { "type": "string" },
                                                "skillId": { "type": "string" }
                                            },
                                            "required": ["pluginId", "skillId"]
                                        },
                                        "description": "Plugin skill references."
                                    },
                                    "workflow": {
                                        "type": "array",
                                        "items": { "type": "string" },
                                        "description": "Legacy workflow text steps for backward compatibility. Prefer workflowNodes."
                                    },
                                    "workflowNodes": {
                                        "type": "array",
                                        "description": "Structured workflow nodes with per-node skill assignment.",
                                        "minItems": 1,
                                        "items": {
                                            "type": "object",
                                            "properties": {
                                                "objective": {
                                                    "type": "string",
                                                    "description": "What this node must complete before moving to the next node."
                                                },
                                                "skills": {
                                                    "type": "array",
                                                    "items": { "type": "string" },
                                                    "description": "Local skill IDs assigned to this node."
                                                },
                                                "pluginSkills": {
                                                    "type": "array",
                                                    "items": {
                                                        "type": "object",
                                                        "properties": {
                                                            "pluginId": { "type": "string" },
                                                            "skillId": { "type": "string" }
                                                        },
                                                        "required": ["pluginId", "skillId"]
                                                    },
                                                    "description": "Plugin skill references assigned to this node."
                                                }
                                            },
                                            "required": ["objective", "skills", "pluginSkills"],
                                            "anyOf": [
                                                {
                                                    "properties": {
                                                        "skills": { "minItems": 1 }
                                                    }
                                                },
                                                {
                                                    "properties": {
                                                        "pluginSkills": { "minItems": 1 }
                                                    }
                                                }
                                            ]
                                        }
                                    },
                                    "systemPrompt": { "type": "string", "description": "Role definition and behavior rules for the robot." }
                                },
                                "required": ["name", "description", "workflowNodes", "systemPrompt"]
                            }
                        },
                        "required": ["id", "config"]
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "code_search",
                    "description": "Search source code in the current workspace using CN-Codex's built-in search engine. The implementation is embedded in CN-Codex and does not require an external rg installation.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "pattern": {
                                "type": "string",
                                "minLength": 1,
                                "description": "Text or regular expression to search for."
                            },
                            "path": {
                                "type": "string",
                                "description": "Optional file or directory inside the current workspace. Defaults to the workspace root."
                            },
                            "glob": {
                                "type": "array",
                                "items": { "type": "string", "minLength": 1 },
                                "description": "Optional glob filters such as '*.rs' or 'src/**'."
                            },
                            "case_sensitive": {
                                "type": "boolean",
                                "description": "Use case-sensitive matching. Defaults to false."
                            },
                            "fixed_strings": {
                                "type": "boolean",
                                "description": "Treat pattern as literal text instead of a regular expression. Defaults to false."
                            },
                            "context": {
                                "type": "integer",
                                "minimum": 0,
                                "maximum": 5,
                                "description": "Context lines before and after each match."
                            },
                            "head_limit": {
                                "type": "integer",
                                "minimum": 1,
                                "maximum": 200,
                                "description": "Maximum match lines to return. Defaults to 50."
                            },
                            "timeout_ms": {
                                "type": "integer",
                                "minimum": 1000,
                                "maximum": 120000,
                                "description": "Search timeout in milliseconds. Defaults to 30000."
                            }
                        },
                        "required": ["pattern"]
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "code_review",
                    "description": "Review the current git diff or a diff against a base ref. It summarizes changed files, runs git diff --check, and flags obvious risks such as secret-looking additions, risky APIs, debug logging, and source changes without matching tests.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "base_ref": {
                                "type": "string",
                                "description": "Optional git ref to compare against. Defaults to HEAD, so staged and unstaged tracked changes are reviewed."
                            },
                            "paths": {
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "Optional path filters inside the repository."
                            },
                            "max_diff_bytes": {
                                "type": "integer",
                                "minimum": 4000,
                                "maximum": 1000000,
                                "description": "Maximum diff bytes to inspect. Defaults to 200000."
                            },
                            "include_untracked": {
                                "type": "boolean",
                                "description": "Include untracked file names in the report. Defaults to true; contents are not inspected until tracked."
                            }
                        },
                        "required": []
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "apply_patch",
                    "description": "Use apply_patch to edit files. The patch must contain exactly one *** Begin Patch and one *** End Patch wrapper. For multiple files, repeat *** Update File sections inside that single wrapper. Every update must include actual '-' and '+' lines. Do not nest another Begin Patch or use Markdown, context-diff, diff --git, timestamp, ---, or +++ envelopes.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "patch": {
                                "type": "string",
                                "description": "Complete raw patch body with one outer wrapper. Multiple files use multiple Add/Update/Delete File sections inside that wrapper, never multiple Begin/End markers."
                            }
                        },
                        "required": ["patch"],
                        "additionalProperties": false
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "list_directory",
                    "description": "List files and directories at the given path.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "path": {
                                "type": "string",
                                "description": "The directory path to list. Defaults to current working directory."
                            }
                        },
                        "required": []
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "update_plan",
                    "description": "Update the current task plan for multi-step work. Use concise steps, and keep at most one step in_progress.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "explanation": {
                                "type": "string",
                                "description": "Optional short explanation for this plan update."
                            },
                            "plan": {
                                "type": "array",
                                "description": "Ordered plan items.",
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "step": {
                                            "type": "string",
                                            "description": "A concise task step."
                                        },
                                        "status": {
                                            "type": "string",
                                            "enum": ["pending", "in_progress", "completed"],
                                            "description": "Current status for this step."
                                        }
                                    },
                                    "required": ["step", "status"]
                                }
                            }
                        },
                        "required": ["plan"]
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "request_user_input",
                    "description": "Request user input for one to three short questions and wait for the response. Use this only when progress genuinely depends on a user choice or answer.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "questions": {
                                "type": "array",
                                "description": "Questions to show the user. Prefer 1 and do not exceed 3.",
                                "minItems": 1,
                                "maxItems": 3,
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "id": {
                                            "type": "string",
                                            "description": "Stable identifier for mapping answers, preferably snake_case."
                                        },
                                        "header": {
                                            "type": "string",
                                            "description": "Short header label shown in the UI."
                                        },
                                        "question": {
                                            "type": "string",
                                            "description": "Single-sentence prompt shown to the user."
                                        },
                                        "options": {
                                            "type": "array",
                                            "description": "Optional mutually exclusive choices. Put the recommended option first when there is one, and prefer adding '(Recommended)' to the recommended label.",
                                            "minItems": 0,
                                            "maxItems": 3,
                                            "items": {
                                                "type": "object",
                                                "properties": {
                                                    "label": {
                                                        "type": "string",
                                                        "description": "User-facing label."
                                                    },
                                                    "description": {
                                                        "type": "string",
                                                        "description": "Short explanation of the option."
                                                    }
                                                },
                                                "required": ["label", "description"]
                                            }
                                        }
                                    },
                                    "required": ["id", "header", "question"]
                                }
                            }
                        },
                        "required": ["questions"]
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "request_permissions",
                    "description": "Request additional filesystem or network permissions from the user and wait for the client to grant a subset of the requested permission profile. Granted additional_permissions are cached for this executor and reused by later shell/exec calls that request the same or narrower profile.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "reason": {
                                "type": "string",
                                "description": "Optional short explanation for why additional permissions are needed."
                            },
                            "environment_id": {
                                "type": "string",
                                "description": "Optional environment id. Omit to use the primary workspace environment."
                            },
                            "permissions": {
                                "type": "object",
                                "description": "Requested permission profile. Use network and/or file_system fields.",
                                "properties": {
                                    "network": {
                                        "type": "object",
                                        "description": "Requested network permissions."
                                    },
                                    "file_system": {
                                        "type": "object",
                                        "description": "Requested filesystem permissions."
                                    }
                                }
                            }
                        },
                        "required": ["permissions"]
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "view_image",
                    "description": "Inspect a local image file and return its format, dimensions, size, and absolute path. Use this when the user asks about an image or when visual assets need verification.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "path": {
                                "type": "string",
                                "description": "Relative path inside the workspace, or an absolute local image path."
                            }
                        },
                        "required": ["path"]
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "ocr_image",
                    "description": "Run bundled PP-OCRv5 mobile via ONNX Runtime on a local image and return extracted text.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "path": {
                                "type": "string",
                                "description": "Relative path inside the workspace, or an absolute local image path."
                            }
                        },
                        "required": ["path"]
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "image_generate",
                    "description": "Generate an image through an OpenAI Images API-compatible backend, save it as a local file, and return its path, format, dimensions, and size. Resolution order: tool args > image_generation settings > env vars > defaults.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "prompt": {
                                "type": "string",
                                "description": "Detailed prompt describing the image to generate."
                            },
                            "model": {
                                "type": "string",
                                "description": "Optional image model. Defaults to image_generation.model, then CN_CODEX_IMAGE_MODEL, then gpt-image-2."
                            },
                            "size": {
                                "type": "string",
                                "description": "Image size such as 1024x1024, 1024x1536, 1536x1024, or auto. Defaults to 1024x1024."
                            },
                            "quality": {
                                "type": "string",
                                "description": "Optional provider-specific quality value, such as low, medium, high, hd, or auto."
                            },
                            "background": {
                                "type": "string",
                                "description": "Optional provider-specific background value, such as transparent, opaque, or auto."
                            },
                            "n": {
                                "type": "integer",
                                "minimum": 1,
                                "maximum": 10,
                                "description": "Number of images to generate. Values are clamped to 1-10. Defaults to 1."
                            },
                            "output_path": {
                                "type": "string",
                                "description": "Optional output file path. Relative paths resolve inside the workspace and must not contain '..'. Defaults to codey/images/generated/<id>.png. When n is greater than 1, suffixed paths such as name-1.png and name-2.png are used."
                            },
                            "base_url": {
                                "type": "string",
                                "description": "Optional OpenAI-compatible base URL or full /images/generations endpoint. Defaults to image_generation.base_url, then CN_CODEX_IMAGE_BASE_URL, then https://api.openai.com/v1."
                            }
                        },
                        "required": ["prompt"]
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "echarts_report",
                    "description": "Prepare an interactive ECharts report configuration and return a reusable ```echarts code block for chat rendering.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "title": {
                                "type": "string",
                                "description": "Optional chart title shown in tool summaries."
                            },
                            "chart_type": {
                                "type": "string",
                                "description": "Optional chart type hint, such as line, bar, pie, scatter, radar, or heatmap."
                            },
                            "option": {
                                "type": "object",
                                "description": "ECharts option object. Must be valid JSON object syntax accepted by echarts.setOption."
                            },
                            "notes": {
                                "type": "string",
                                "description": "Optional notes or interpretation text for this report."
                            }
                        },
                        "required": ["option"]
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "browser_run",
                    "description": "Run a browser session for navigation, UI interaction, screenshots, rendered DOM inspection, and web app testing. Runtime uses CN-Codex Tauri WebView with Rust-side JS Injection + CDP.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "engine": {
                                "type": "string",
                                "enum": ["webview-js-injection"],
                                "description": "Compatibility field. Runtime is always mapped to webview-js-injection internally."
                            },
                            "url": {
                                "type": "string",
                                "description": "Initial URL to open. Required unless the first action is a goto."
                            },
                            "headless": {
                                "type": "boolean",
                                "description": "Compatibility field kept for old prompts. Ignored by WebView runtime."
                            },
                            "channel": {
                                "type": "string",
                                "description": "Compatibility field kept for old prompts. Ignored in WebView runtime."
                            },
                            "use_visible_browser": {
                                "type": "boolean",
                                "description": "Compatibility field kept for old prompts. WebView runtime always controls CN-Codex built-in browser."
                            },
                            "viewport": {
                                "type": "object",
                                "properties": {
                                    "width": { "type": "integer", "minimum": 320 },
                                    "height": { "type": "integer", "minimum": 240 }
                                }
                            },
                            "actions": {
                                "type": "array",
                                "description": "Ordered browser actions: goto, reload, back, forward, click, hover, fill, type, press, check, uncheck, select_option, wait_for_selector, wait_for_timeout, screenshot, set_viewport, title, url, html, snapshot, assets, bundle_assets, eval, text, list_tabs, new_tab, switch_tab, or close_tab.",
                                "items": { "type": "object" }
                            },
                            "timeout_ms": {
                                "type": "integer",
                                "minimum": 1000,
                                "maximum": 120000,
                                "description": "Overall runner timeout in milliseconds. Defaults to 60000."
                            }
                        },
                        "required": []
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "spawn_agent",
                    "description": "Start a background CN-Codex subagent for delegated investigation, review, testing, or implementation. The subagent runs on the built-in internal subagent engine.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "prompt": {
                                "type": "string",
                                "description": "Complete task instructions for the subagent."
                            },
                            "role": {
                                "type": "string",
                                "description": "Short role label, such as reviewer, tester, researcher, or implementer."
                            },
                            "cwd": {
                                "type": "string",
                                "description": "Optional working directory. Relative paths resolve under the current workspace."
                            },
                            "timeout_ms": {
                                "type": "integer",
                                "minimum": 1000,
                                "maximum": 1800000,
                                "description": "Maximum runtime for the subagent. Defaults to 600000."
                            },
                            "wait": {
                                "type": "boolean",
                                "description": "Wait for completion before returning. Defaults to false."
                            },
                            "model": {
                                "type": "string",
                                "description": "Optional model override for the internal subagent engine."
                            },
                            "sandbox": {
                                "type": "string",
                                "enum": ["read-only", "workspace-write", "danger-full-access"],
                                "description": "Compatibility field reserved for Codex parity. Ignored by the current internal subagent runtime."
                            },
                            "dangerously_bypass_approvals_and_sandbox": {
                                "type": "boolean",
                                "description": "Compatibility field reserved for Codex parity. Ignored by the current internal subagent runtime."
                            }
                        },
                        "required": ["prompt"]
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "wait_agent",
                    "description": "Wait for one or more background subagents started with spawn_agent and return their latest status and output.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "agent_id": {
                                "type": "string",
                                "description": "Single subagent id to wait for."
                            },
                            "agent_ids": {
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "Multiple subagent ids to wait for. If omitted, waits for all running subagents."
                            },
                            "timeout_ms": {
                                "type": "integer",
                                "minimum": 0,
                                "maximum": 1800000,
                                "description": "Maximum time to wait. Defaults to 60000. Use 0 to return current status immediately."
                            }
                        },
                        "required": []
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "send_input",
                    "description": "Send a follow-up message to an existing background subagent. CN-Codex forwards the message to the running internal subagent channel when available and records the submission in subagent input history.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "target": {
                                "type": "string",
                                "description": "Subagent id to message, returned by spawn_agent or list_agents."
                            },
                            "agent_id": {
                                "type": "string",
                                "description": "Compatibility alias for target."
                            },
                            "id": {
                                "type": "string",
                                "description": "Compatibility alias for target."
                            },
                            "message": {
                                "type": "string",
                                "description": "Plain-text follow-up message for the subagent."
                            },
                            "items": {
                                "type": "array",
                                "description": "Optional structured input items. Text is extracted when message is omitted.",
                                "items": { "type": "object" }
                            },
                            "interrupt": {
                                "type": "boolean",
                                "description": "Compatibility flag for Codex send_input. Currently recorded in input history only and does not force immediate turn interruption."
                            }
                        },
                        "required": []
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "resume_agent",
                    "description": "Resume a previously closed, completed, failed, or timed-out background subagent by restarting the internal subagent engine with the same task context and id.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "id": {
                                "type": "string",
                                "description": "Subagent id to resume, returned by spawn_agent or list_agents."
                            },
                            "target": {
                                "type": "string",
                                "description": "Compatibility alias for id."
                            },
                            "agent_id": {
                                "type": "string",
                                "description": "Compatibility alias for id."
                            },
                            "timeout_ms": {
                                "type": "integer",
                                "minimum": 1000,
                                "maximum": 1800000,
                                "description": "Maximum runtime for the resumed subagent run. Defaults to 600000."
                            },
                            "wait": {
                                "type": "boolean",
                                "description": "Wait for the resumed subagent run to finish before returning. Defaults to false."
                            }
                        },
                        "required": []
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "list_agents",
                    "description": "List background subagents and their statuses.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "status": {
                                "type": "string",
                                "enum": ["running", "completed", "failed", "timed_out", "closed", "interrupted"],
                                "description": "Optional status filter."
                            }
                        },
                        "required": []
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "close_agent",
                    "description": "Close a background subagent started with spawn_agent when it is no longer needed. If it is still running, CN-Codex signals cancellation in the internal runtime and returns the previous status.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "target": {
                                "type": "string",
                                "description": "Subagent id to close, returned by spawn_agent or list_agents."
                            },
                            "agent_id": {
                                "type": "string",
                                "description": "Compatibility alias for target."
                            },
                            "id": {
                                "type": "string",
                                "description": "Compatibility alias for target."
                            }
                        },
                        "required": []
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "memory_list",
                    "description": "List immediate files and directories in the CN-Codex memories store.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "path": {
                                "type": "string",
                                "description": "Optional relative directory path inside the memories store. Defaults to root."
                            },
                            "max_entries": {
                                "type": "integer",
                                "minimum": 1,
                                "maximum": 200,
                                "description": "Maximum entries to return. Defaults to 100."
                            },
                            "cursor": {
                                "type": "string",
                                "description": "Opaque cursor from a previous memory_list response."
                            },
                            "format": {
                                "type": "string",
                                "enum": ["text", "json"],
                                "description": "Output format. Defaults to text."
                            }
                        },
                        "required": []
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "memory_read",
                    "description": "Read a memory file by relative path from the CN-Codex memories store.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "path": {
                                "type": "string",
                                "description": "Relative memory file path."
                            },
                            "line_offset": {
                                "type": "integer",
                                "minimum": 1,
                                "description": "1-indexed starting line. Defaults to 1."
                            },
                            "max_lines": {
                                "type": "integer",
                                "minimum": 1,
                                "maximum": 500,
                                "description": "Maximum lines to return. Defaults to 200."
                            },
                            "format": {
                                "type": "string",
                                "enum": ["text", "json"],
                                "description": "Output format. Defaults to text."
                            }
                        },
                        "required": ["path"]
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "memory_search",
                    "description": "Search memory files for a text query and return matching lines.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "query": {
                                "type": "string",
                                "description": "Text to search for."
                            },
                            "path": {
                                "type": "string",
                                "description": "Optional relative directory or file path to search within."
                            },
                            "case_sensitive": {
                                "type": "boolean",
                                "description": "Whether matching is case-sensitive. Defaults to false."
                            },
                            "max_results": {
                                "type": "integer",
                                "minimum": 1,
                                "maximum": 50,
                                "description": "Maximum matches to return. Defaults to 20."
                            },
                            "context_lines": {
                                "type": "integer",
                                "minimum": 0,
                                "maximum": 5,
                                "description": "Number of context lines before and after each match. Defaults to 0."
                            },
                            "cursor": {
                                "type": "string",
                                "description": "Opaque cursor from a previous memory_search response."
                            },
                            "format": {
                                "type": "string",
                                "enum": ["text", "json"],
                                "description": "Output format. Defaults to text."
                            }
                        },
                        "required": ["query"]
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "memory_write",
                    "description": "Create, overwrite, or append a Markdown memory file. Use only when the user explicitly asks CN-Codex to remember, forget, or update durable information.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "path": {
                                "type": "string",
                                "description": "Relative memory file path, usually ending in .md."
                            },
                            "content": {
                                "type": "string",
                                "description": "Markdown content to write."
                            },
                            "append": {
                                "type": "boolean",
                                "description": "Append to the file instead of replacing it. Defaults to false."
                            }
                        },
                        "required": ["path", "content"]
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "memory_update",
                    "description": "Replace exact text inside an existing memory file. Use only when the user explicitly asks CN-Codex to update durable memory.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "path": {
                                "type": "string",
                                "description": "Relative memory file path."
                            },
                            "old_text": {
                                "type": "string",
                                "description": "Exact text to replace."
                            },
                            "new_text": {
                                "type": "string",
                                "description": "Replacement text. Use an empty string to remove the exact text."
                            },
                            "replace_all": {
                                "type": "boolean",
                                "description": "Replace every occurrence instead of only the first. Defaults to false."
                            }
                        },
                        "required": ["path", "old_text", "new_text"]
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "memory_forget",
                    "description": "Forget durable memory by deleting a memory file/directory or removing lines containing exact text from a memory file. Use only when the user explicitly asks CN-Codex to forget durable information.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "path": {
                                "type": "string",
                                "description": "Relative memory file or directory path."
                            },
                            "match_text": {
                                "type": "string",
                                "description": "Optional exact text. When provided, removes lines containing this text from the file instead of deleting the whole path."
                            },
                            "recursive": {
                                "type": "boolean",
                                "description": "Allow deleting a directory recursively. Defaults to false."
                            }
                        },
                        "required": ["path"]
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "mcp_list_servers",
                    "description": "List configured MCP servers from codey/config.toml, excluding secret environment values.",
                    "parameters": {
                        "type": "object",
                        "properties": {},
                        "required": []
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "mcp_status",
                    "description": "Inspect configured MCP server status without revealing secret env values. Optionally probes tools, resources, resource templates, and prompts to report availability and counts.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "server": {
                                "type": "string",
                                "description": "Optional MCP server name from mcp_list_servers."
                            },
                            "probe": {
                                "type": "boolean",
                                "description": "Whether to probe enabled servers. Defaults to true."
                            }
                        },
                        "required": []
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "mcp_list_tools",
                    "description": "Start one configured MCP server, or all enabled servers if omitted, and list available MCP tools.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "server": {
                                "type": "string",
                                "description": "Optional MCP server name from mcp_list_servers."
                            }
                        },
                        "required": []
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "mcp_call_tool",
                    "description": "Call a tool exposed by a configured MCP server.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "server": {
                                "type": "string",
                                "description": "MCP server name from mcp_list_servers."
                            },
                            "tool": {
                                "type": "string",
                                "description": "MCP tool name to call."
                            },
                            "arguments": {
                                "type": "object",
                                "description": "JSON object arguments for the MCP tool."
                            }
                        },
                        "required": ["server", "tool"]
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "mcp_list_resources",
                    "description": "Start one configured MCP server, or all enabled servers if omitted, and list MCP resources.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "server": {
                                "type": "string",
                                "description": "Optional MCP server name from mcp_list_servers."
                            }
                        },
                        "required": []
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "mcp_read_resource",
                    "description": "Read a resource by URI from a configured MCP server.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "server": {
                                "type": "string",
                                "description": "MCP server name from mcp_list_servers."
                            },
                            "uri": {
                                "type": "string",
                                "description": "Resource URI to read."
                            }
                        },
                        "required": ["server", "uri"]
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "mcp_list_resource_templates",
                    "description": "Start one configured MCP server, or all enabled servers if omitted, and list MCP resource templates.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "server": {
                                "type": "string",
                                "description": "Optional MCP server name from mcp_list_servers."
                            }
                        },
                        "required": []
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "mcp_list_prompts",
                    "description": "Start one configured MCP server, or all enabled servers if omitted, and list MCP prompts.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "server": {
                                "type": "string",
                                "description": "Optional MCP server name from mcp_list_servers."
                            }
                        },
                        "required": []
                    }
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "mcp_get_prompt",
                    "description": "Get a prompt by name from a configured MCP server.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "server": {
                                "type": "string",
                                "description": "MCP server name from mcp_list_servers."
                            },
                            "prompt": {
                                "type": "string",
                                "description": "Prompt name from mcp_list_prompts."
                            },
                            "arguments": {
                                "type": "object",
                                "description": "Optional JSON object arguments for the MCP prompt."
                            }
                        },
                        "required": ["server", "prompt"]
                    }
                }
            }),
        ];

        if !self.image_generation_is_enabled() {
            tools.retain(|tool| {
                tool.pointer("/function/name")
                    .and_then(serde_json::Value::as_str)
                    != Some("image_generate")
            });
        }

        if web_search_enabled {
            tools.push(serde_json::json!({
                "type": "function",
                "function": {
                    "name": "web_search",
                    "description": "Search the web for current information and return concise result titles, snippets, and URLs.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "query": {
                                "type": "string",
                                "description": "The search query."
                            },
                            "max_results": {
                                "type": "integer",
                                "minimum": 1,
                                "maximum": 10,
                                "description": "Maximum number of search results to return. Defaults to 5."
                            }
                        },
                        "required": ["query"]
                    }
                }
            }));
            tools.push(serde_json::json!({
                "type": "function",
                "function": {
                    "name": "web_fetch",
                    "description": "Fetch a web page by URL and return a readable text excerpt with the page title when available.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "url": {
                                "type": "string",
                                "description": "The http or https URL to fetch."
                            },
                            "max_chars": {
                                "type": "integer",
                                "minimum": 1000,
                                "maximum": 20000,
                                "description": "Maximum characters of extracted text to return. Defaults to 8000."
                            }
                        },
                        "required": ["url"]
                    }
                }
            }));
        }

        if self.smartbrain_is_active() {
            tools.push(serde_json::json!({
                "type": "function",
                "function": {
                    "name": "smartbrain_search",
                    "description": "Search the Local Knowledge Base (本地知识库) using BM25 relevance ranking. Returns the most relevant documents (experiences and uploaded knowledge) matching the query. For chunk hits, results include bridged previous/current/next context with at least 30 lines of overlap to avoid cut-off sections.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "query": {
                                "type": "string",
                                "description": "Search query text."
                            },
                            "top_k": {
                                "type": "integer",
                                "minimum": 1,
                                "maximum": 20,
                                "description": "Maximum number of results. Defaults to 5."
                            },
                            "domain": {
                                "type": "string",
                                "description": "Optional domain filter for Local Knowledge Base knowledge (for example: database, backend, devops)."
                            },
                            "concept_type": {
                                "type": "string",
                                "description": "Optional OKF type filter, for example Knowledge or Experience."
                            },
                            "tags": {
                                "type": "array",
                                "items": {
                                    "type": "string"
                                },
                                "description": "Optional tag filters. Any matching tag will pass."
                            },
                            "source_type": {
                                "type": "string",
                                "enum": ["knowledge", "experience"],
                                "description": "Optional source type filter."
                            },
                            "source_group": {
                                "type": "string",
                                "description": "Optional source group filter (for example folder import batch)."
                            },
                            "relative_path_prefix": {
                                "type": "string",
                                "description": "Optional relative path prefix filter under knowledge sources."
                            },
                            "source_file": {
                                "type": "string",
                                "description": "Optional exact source file filter."
                            }
                        },
                        "required": ["query"]
                    }
                }
            }));
            tools.push(serde_json::json!({
                "type": "function",
                "function": {
                    "name": "smartbrain_sql_query",
                    "description": "Execute SQL against a Local Knowledge Base-configured database using the built-in SQL runner. Uses saved connection settings (including password) and permission rules. Prefer this over Python/shell database scripts. Do not ask the user for password when the database is already configured.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "database": {
                                "type": "string",
                                "description": "Database display name, physical database name, or alias from Local Knowledge Base settings. Optional when only one database is configured."
                            },
                            "sql": {
                                "type": "string",
                                "description": "A single SQL statement to execute. Prefer SELECT / SHOW / DESCRIBE for reads."
                            },
                            "row_limit": {
                                "type": "integer",
                                "minimum": 1,
                                "maximum": 1000,
                                "description": "Maximum rows to return. Defaults to Local Knowledge Base DB settings (usually 200)."
                            },
                            "timeout_sec": {
                                "type": "integer",
                                "minimum": 1,
                                "maximum": 120,
                                "description": "Query timeout in seconds. Defaults to Local Knowledge Base DB settings (usually 15)."
                            }
                        },
                        "required": ["sql"]
                    }
                }
            }));
        }

        // Recording tools
        tools.push(serde_json::json!({
            "type": "function",
            "function": {
                "name": "recording_control",
                "description": "Control browser recording and replay for the Record & Replay workflow. Use action='launch_browser' to open an external Chrome, 'show_toggle' to display the recording UI so the user can start/stop recording, 'read_trace' to read a completed recording trace, 'list_traces' to list all saved recordings, or 'run_replay' to run a saved replay script and get its pass/fail result for self-testing and fixing.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "action": {
                            "type": "string",
                            "enum": ["launch_browser", "show_toggle", "hide_toggle", "status", "read_trace", "list_traces", "run_replay"],
                            "description": "The recording control action to perform."
                        },
                        "session_id": {
                            "type": "string",
                            "description": "Session ID of the trace to read (required for read_trace action)."
                        },
                        "script_id": {
                            "type": "string",
                            "description": "Replay script ID to run (required for run_replay action). This is the script file name without the .py extension."
                        }
                    },
                    "required": ["action"]
                }
            }
        }));

        tools
    }


    pub async fn tool_specs_with_mcp(
        &mut self,
        web_search_enabled: bool,
        thread_id: Option<&str>,
    ) -> Vec<serde_json::Value> {
        self.tool_specs_for_turn(web_search_enabled, true, thread_id)
            .await
    }


    /// Layered tool schema assembly:
    /// - always include the small core built-in set
    /// - optionally discover MCP catalogs for search/activation
    /// - only attach non-core built-ins / MCP direct tools that have been activated
    pub async fn tool_specs_for_turn(
        &mut self,
        web_search_enabled: bool,
        discover_mcp: bool,
        thread_id: Option<&str>,
    ) -> Vec<serde_json::Value> {
        self.web_search_enabled = web_search_enabled;
        if discover_mcp {
            // Discover catalogs for search + activation, but do not attach all schemas yet.
            let _ = self.discover_mcp_direct_tool_specs().await;
        }

        let activated = match thread_id {
            Some(id) => self.activated_tool_names_for_thread(id).await,
            None => BTreeSet::new(),
        };
        let mut tools = Vec::new();
        let mut seen = BTreeSet::new();

        for spec in self.tool_specs(web_search_enabled) {
            let Some(name) = Self::tool_spec_name(&spec) else {
                continue;
            };
            // Subagent tools are dialog-gated: even if tool_search previously activated them,
            // keep them hidden unless this chat explicitly enabled subagents.
            if !self.subagent_tools_allowed(name) {
                continue;
            }
            if Self::is_core_tool_name(name)
                || activated.contains(name)
                || Self::is_subagent_tool_name(name)
            {
                if seen.insert(name.to_string()) {
                    tools.push(spec);
                }
            }
        }

        // Activated MCP direct tools (including Playwright) are attached only after search/activation.
        let mut mcp_aliases = self.mcp_tool_specs.keys().cloned().collect::<Vec<_>>();
        mcp_aliases.sort();
        for alias in mcp_aliases {
            if !activated.contains(&alias) {
                continue;
            }
            if let Some(spec) = self.mcp_tool_specs.get(&alias).cloned() {
                if seen.insert(alias) {
                    tools.push(spec);
                }
            }
        }

        tools
    }

}
