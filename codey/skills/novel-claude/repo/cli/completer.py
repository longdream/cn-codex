"""Autocompletion for Novel-Claude CLI."""
from typing import List, Callable
from prompt_toolkit.completion import Completer, Completion, PathCompleter
from prompt_toolkit.document import Document

from cli.project_manager import project_manager


class NovelClaudeCompleter(Completer):
    """Custom completer for Novel-Claude CLI."""

    def __init__(self):
        self.base_completer = PathCompleter()
        self.commands = [
            # Built-in
            '/help', '/exit', '/clear', '/history',

            # Project management
            'projects', 'projects create', 'projects switch',
            'projects list', 'projects info', 'projects delete',

            # Novel workflow
            'init', 'plan', 'write',
            'batch build', 'batch submit', 'batch sync',
            'reindex', 'review',

            # File operations
            'ls', 'cat', 'find', 'cd', 'pwd',

            # Skills
            'skills', 'skills list', 'skills enable',
            'skills disable', 'skills reload', 'skills build',

            # Settings
            'settings', 'settings show', 'settings set',

            # Agent
            'agent', 'agent review',
        ]

        # Add project names as completions
        self.project_names = project_manager.list_projects()

    def get_completions(self, document: Document, complete_event):
        """Generate completions based on current input."""
        word = document.get_word_before_cursor()
        text = document.text

        # Check if in project name context
        if text.startswith('projects switch '):
            for proj in self.project_names:
                if proj.startswith(word):
                    yield Completion(proj, start_position=-len(word))

        # Check if in path context (after ls, cat, cd, find)
        elif any(text.startswith(cmd) for cmd in ['ls ', 'cat ', 'cd ', 'find ']):
            # Use path completer for file paths
            for completion in self.base_completer.get_completions(document, complete_event):
                yield completion

        # Otherwise, command completion
        else:
            for cmd in self.commands:
                if cmd.startswith(word):
                    yield Completion(cmd, start_position=-len(word))

            # Also complete skill names
            if text.startswith('skills enable ') or text.startswith('skills disable '):
                skill_name = word
                skills_dir = 'skills'
                import os
                if os.path.exists(skills_dir):
                    for d in os.listdir(skills_dir):
                        if os.path.isdir(os.path.join(skills_dir, d)) and d.startswith(skill_name):
                            yield Completion(d, start_position=-len(skill_name))