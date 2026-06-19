"""Skill management commands for Novel-Claude CLI."""
from typing import List, Any, Dict


def handle(args: List[str]) -> Dict[str, Any]:
    """Handle skills command with no subcommand."""
    return {'message': 'Use: skills list, skills enable <name>, skills disable <name>, skills reload [name], skills build <request>'}


def list_skills(args: List[str]) -> Dict[str, Any]:
    """List all skills."""
    try:
        from core.novel_context import NovelContext
        from core.plugin_manager import PluginManager
        from utils.workspace import WorkspaceManager
        from utils.config import NOVEL_DIR

        workspace = WorkspaceManager(NOVEL_DIR)
        context = NovelContext(workspace)
        mgr = PluginManager(context)
        mgr.scan_and_load()

        output = []
        if context.active_skills:
            output.append(f"Loaded {len(context.active_skills)} skills:")
            for name, skill in context.active_skills.items():
                output.append(f"  🟢 {skill.name} (skills/{name}/skill.py)")
        else:
            output.append("No skills loaded.")

        # List skills directory
        import os
        skills_dir = "skills"
        if os.path.exists(skills_dir):
            all_dirs = [d for d in os.listdir(skills_dir)
                       if os.path.isdir(os.path.join(skills_dir, d))
                       and not d.startswith("__") and not d.startswith(".")]
            unloaded = [d for d in all_dirs if d not in context.active_skills]

            disabled = [d for d in unloaded if os.path.exists(os.path.join(skills_dir, d, ".disabled"))]
            errors = [d for d in unloaded if d not in disabled]

            if disabled:
                output.append("\nDisabled:")
                for d in disabled:
                    output.append(f"  🔴 skills/{d}/")
            if errors:
                output.append("\nLoad errors:")
                for d in errors:
                    output.append(f"  ⚠️ skills/{d}/")

        return {'message': '\n'.join(output)}
    except Exception as e:
        return {'error': f'skills list failed: {e}'}


def enable(args: List[str]) -> Dict[str, Any]:
    """Enable a skill."""
    if not args:
        return {'error': 'Usage: skills enable <name>'}
    name = args[0]

    try:
        from core.novel_context import NovelContext
        from core.plugin_manager import PluginManager
        from utils.workspace import WorkspaceManager
        from utils.config import NOVEL_DIR

        workspace = WorkspaceManager(NOVEL_DIR)
        context = NovelContext(workspace)
        mgr = PluginManager(context)
        mgr.enable_skill(name)
        return {'message': f'Skill "{name}" enabled.'}
    except Exception as e:
        return {'error': f'skills enable failed: {e}'}


def disable(args: List[str]) -> Dict[str, Any]:
    """Disable a skill."""
    if not args:
        return {'error': 'Usage: skills disable <name>'}
    name = args[0]

    try:
        from core.novel_context import NovelContext
        from core.plugin_manager import PluginManager
        from utils.workspace import WorkspaceManager
        from utils.config import NOVEL_DIR

        workspace = WorkspaceManager(NOVEL_DIR)
        context = NovelContext(workspace)
        mgr = PluginManager(context)
        mgr.disable_skill(name)
        return {'message': f'Skill "{name}" disabled.'}
    except Exception as e:
        return {'error': f'skills disable failed: {e}'}


def reload(args: List[str]) -> Dict[str, Any]:
    """Reload skills."""
    name = args[0] if args else None

    try:
        from core.novel_context import NovelContext
        from core.plugin_manager import PluginManager
        from utils.workspace import WorkspaceManager
        from utils.config import NOVEL_DIR

        workspace = WorkspaceManager(NOVEL_DIR)
        context = NovelContext(workspace)
        mgr = PluginManager(context)

        if name:
            mgr.scan_and_load()
            mgr.hot_reload(name)
            return {'message': f'Skill "{name}" reloaded.'}
        else:
            mgr.scan_and_load()
            return {'message': f'All skills reloaded. {len(context.active_skills)} skills active.'}
    except Exception as e:
        return {'error': f'skills reload failed: {e}'}


def build(args: List[str]) -> Dict[str, Any]:
    """Build a new skill from natural language request."""
    if not args:
        return {'error': 'Usage: skills build <request>'}
    request = ' '.join(args)

    try:
        from core.novel_context import NovelContext
        from core.plugin_manager import PluginManager
        from core.agents.skill_builder_agent import SkillBuilderAgent
        from utils.workspace import WorkspaceManager
        from utils.config import NOVEL_DIR

        workspace = WorkspaceManager(NOVEL_DIR)
        context = NovelContext(workspace)
        mgr = PluginManager(context)
        mgr.scan_and_load()

        agent = SkillBuilderAgent(context, mgr)
        success = agent.build_skill(request)

        if success:
            return {'message': 'Skill built and reloaded successfully.'}
        return {'error': 'Skill build failed. Check logs for details.'}
    except Exception as e:
        return {'error': f'skills build failed: {e}'}