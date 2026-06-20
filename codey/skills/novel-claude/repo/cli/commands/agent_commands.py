"""Agent commands for Novel-Claude CLI."""
from typing import List, Any, Dict
import os
import re


def handle(args: List[str]) -> Dict[str, Any]:
    """Handle agent command with no subcommand."""
    return {'message': 'Use: agent review -f <file1> -f <file2> -i <instruction>'}


def perform_multi_file_review_impl(filepaths, instruction):
    """Core AI multi-file review logic, extracted from original gui_modules.logic."""
    from cli.commands.constants import DEFAULT_PROMPT_REVIEW
    from utils.llm_client import generate_stream

    print(f"\n[AI Review] Starting review of {len(filepaths)} files...")
    file_contents = {}
    for fp in filepaths:
        try:
            with open(fp, "r", encoding="utf-8") as f:
                file_contents[fp] = f.read()
        except Exception as e:
            print(f"[ERROR] Failed to read file {fp}: {e}")
            return False

    print("[AI Review] Requesting LLM for modified content (may take a while)...")

    sys_prompt = os.getenv("PROMPT_REVIEW", DEFAULT_PROMPT_REVIEW).format(instruction=instruction)

    user_prompt_parts = ["【源文件内容】:"]
    for fp, content in file_contents.items():
        user_prompt_parts.append(f"==== BEGIN FILE: {fp} ====\n{content}\n==== END FILE ====\n")

    user_prompt_parts.append("\n请输出修改后的完整内容，必须严格遵守与输入相同的格式：\n==== BEGIN FILE: <完整路径> ====\n<修改后的内容>\n==== END FILE ====\n")
    user_prompt_parts.append("如果你认为某个文件不需要修改，也请按此格式原样输出它的内容，不要遗漏。不要输出任何除了这个标记块之外的内容（比如分析或总结）。")
    user_prompt = "\n".join(user_prompt_parts)

    try:
        new_content = generate_stream(prompt=user_prompt, system_message=sys_prompt)
        new_content = new_content.strip()

        if new_content.startswith("```"):
            lines = new_content.split("\n")
            if len(lines) > 2:
                new_content = "\n".join(lines[1:-1])

        pattern = re.compile(r"==== BEGIN FILE: (.*?) ====\n(.*?)\n==== END FILE ====", re.DOTALL)
        matches = pattern.findall(new_content)

        if not matches:
            print("\n[ERROR] LLM output format incorrect, cannot auto-parse.")
            return False

        success_count = 0
        for fp, modified_content in matches:
            fp = fp.strip()
            if fp in file_contents or os.path.exists(fp):
                modified_content = modified_content.strip()
                if modified_content.startswith("```"):
                    m_lines = modified_content.split("\n")
                    if len(m_lines) > 2:
                        modified_content = "\n".join(m_lines[1:-1])

                with open(fp, "w", encoding="utf-8") as f:
                    f.write(modified_content + "\n")
                print(f"\n[OK] Modified and saved: {fp}")
                success_count += 1
            else:
                print(f"\n[WARN] Unknown file path in LLM output: {fp}, skipping.")

        print(f"\n[INFO] Review complete! Updated {success_count} files.")
        return True
    except Exception as e:
        import traceback
        traceback.print_exc()
        print(f"\n[ERROR] AI generation or save failed: {e}")
        return False


def review(args: List[str]) -> Dict[str, Any]:
    """Review files with AI."""
    files = []
    instruction = ""

    i = 0
    while i < len(args):
        if args[i] == '-f' and i + 1 < len(args):
            files.append(args[i + 1])
            i += 2
        elif args[i] == '--file' and i + 1 < len(args):
            files.append(args[i + 1])
            i += 2
        elif args[i] == '-i' and i + 1 < len(args):
            instruction = args[i + 1]
            i += 2
        elif args[i] == '--instruction' and i + 1 < len(args):
            instruction = args[i + 1]
            i += 2
        else:
            i += 1

    if not files or not instruction:
        return {'error': 'Usage: agent review -f <file1> -f <file2> -i <instruction>'}

    # Filter existing files
    valid_files = []
    for f in files:
        if os.path.exists(f):
            valid_files.append(f)
        else:
            print(f"[WARN] File not found: {f}")

    if not valid_files:
        return {'error': 'No valid files to review.'}

    try:
        success = perform_multi_file_review_impl(valid_files, instruction)
        if success:
            return {'message': f'Successfully reviewed {len(valid_files)} files.'}
        return {'error': 'Review failed.'}
    except ImportError:
        return {'error': 'Review function not available. GUI modules were removed.'}
    except Exception as e:
        return {'error': f'review failed: {e}'}