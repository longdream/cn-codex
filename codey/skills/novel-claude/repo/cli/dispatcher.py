"""Command dispatcher for Novel-Claude CLI."""
import shlex
from typing import Dict, Optional, Any

from cli.commands import (
    project_commands,
    file_commands,
    skill_commands,
    novel_commands,
    agent_commands,
    settings_commands,
    builtins
)


class CommandDispatcher:
    """Routes commands to appropriate handlers."""

    def __init__(self):
        self.commands: Dict[str, callable] = {}
        self._register_commands()

    def _register_commands(self):
        """Register all available commands."""
        # Project commands
        self.commands['projects'] = project_commands.handle
        self.commands['projects create'] = project_commands.create
        self.commands['projects switch'] = project_commands.switch_project
        self.commands['projects list'] = project_commands.list_projects
        self.commands['projects delete'] = project_commands.delete_project
        self.commands['projects info'] = project_commands.info

        # File operations
        self.commands['ls'] = file_commands.ls
        self.commands['cat'] = file_commands.cat
        self.commands['find'] = file_commands.find
        self.commands['cd'] = file_commands.cd
        self.commands['pwd'] = file_commands.pwd

        # Novel workflow commands
        self.commands['init'] = novel_commands.init
        self.commands['plan'] = novel_commands.plan
        self.commands['write'] = novel_commands.write
        self.commands['batch'] = novel_commands.batch
        self.commands['batch_build'] = novel_commands.batch_build
        self.commands['batch_submit'] = novel_commands.batch_submit
        self.commands['batch_sync'] = novel_commands.batch_sync
        self.commands['reindex'] = novel_commands.reindex
        self.commands['audit'] = novel_commands.audit
        self.commands['track'] = novel_commands.track

        # Skill commands
        self.commands['skills'] = skill_commands.handle
        self.commands['skills list'] = skill_commands.list_skills
        self.commands['skills enable'] = skill_commands.enable
        self.commands['skills disable'] = skill_commands.disable
        self.commands['skills reload'] = skill_commands.reload
        self.commands['skills build'] = skill_commands.build

        # Agent commands
        self.commands['agent'] = agent_commands.handle
        self.commands['agent review'] = agent_commands.review

        # Settings commands
        self.commands['settings'] = settings_commands.handle
        self.commands['settings show'] = settings_commands.show
        self.commands['settings set'] = settings_commands.set_value

        # Builtin commands
        self.commands['alias'] = builtins.alias

        # Also register aliases without / prefix
        for cmd in ['init', 'plan', 'write', 'reindex', 'review', 'audit', 'track']:
            self.commands[cmd] = self._get_handler(cmd)

    def _get_handler(self, cmd: str):
        """Get handler for a command, handling aliases."""
        handlers = {
            'init': novel_commands.init,
            'plan': novel_commands.plan,
            'write': novel_commands.write,
            'reindex': novel_commands.reindex,
            'audit': novel_commands.audit,
            'track': novel_commands.track,
        }
        return handlers.get(cmd)

    def dispatch(self, user_input: str) -> Dict[str, Any]:
        """Parse and dispatch a command."""
        try:
            # Handle slash commands
            if user_input.startswith('/'):
                user_input = user_input[1:]

            # Parse arguments (supports quotes)
            parts = shlex.split(user_input)
            if not parts:
                return {'error': 'Empty command'}

            # Find command handler
            cmd = parts[0]
            args = parts[1:]

            # Try command + subcommand FIRST (more specific match)
            if args and f"{cmd} {args[0]}" in self.commands:
                subcmd = f"{cmd} {args[0]}"
                handler = self.commands[subcmd]
                return handler(args[1:] if len(args) > 1 else [])

            # Try exact match
            if cmd in self.commands:
                handler = self.commands[cmd]
                if handler:
                    return handler(args)

            # Check for Click-style commands (init, plan, write, etc.)
            if cmd in ['init', 'plan', 'write', 'reindex', 'skills']:
                return self._dispatch_click_command(cmd, args)

            return {'error': f"Unknown command: {cmd}. Type /help for available commands."}

        except Exception as e:
            import traceback
            traceback.print_exc()
            return {'error': f"Command execution failed: {e}"}

    def _dispatch_click_command(self, cmd: str, args: list) -> Dict[str, Any]:
        """Dispatch to existing Click commands for backward compatibility."""
        import subprocess
        import sys

        # Build command line args
        cmd_args = [sys.executable, "cli.py", cmd] + args
        try:
            result = subprocess.run(
                cmd_args,
                capture_output=True,
                text=True,
                cwd=os.getcwd(),
                encoding='utf-8'
            )
            return {'output': result.stdout + result.stderr}
        except Exception as e:
            return {'error': str(e)}


import os  # For subprocess cwd