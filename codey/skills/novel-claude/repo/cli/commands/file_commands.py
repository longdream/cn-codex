"""File operation commands for Novel-Claude CLI."""
import os
from pathlib import Path
from typing import List, Any, Dict
from cli.project_manager import project_manager


def _get_project_root() -> Path:
    """Get the current project root directory."""
    if project_manager.current_project:
        return project_manager.get_project_dir()
    # Fall back to current working directory
    return Path.cwd()


def ls(args: List[str]) -> Dict[str, Any]:
    """List directory contents."""
    path = args[0] if args else "."
    try:
        full_path = Path(path)
        if not full_path.is_absolute():
            full_path = project_manager.current_path / path

        if not full_path.exists():
            return {'error': f'Path does not exist: {path}'}

        entries = list(full_path.iterdir())
        if not entries:
            return {'message': f'(empty directory: {full_path})'}

        output = []
        for entry in sorted(entries):
            marker = '/' if entry.is_dir() else ''
            output.append(f"  {entry.name}{marker}")

        return {'message': '\n'.join(output)}
    except Exception as e:
        return {'error': f'ls failed: {e}'}


def cat(args: List[str]) -> Dict[str, Any]:
    """Show file contents."""
    if not args:
        return {'error': 'Usage: cat <file>'}

    filepath = Path(args[0])
    if not filepath.is_absolute():
        filepath = project_manager.current_path / filepath

    if not filepath.exists():
        return {'error': f'File not found: {filepath}'}

    try:
        with open(filepath, 'r', encoding='utf-8') as f:
            content = f.read()
        return {'output': content}
    except Exception as e:
        return {'error': f'cat failed: {e}'}


def find(args: List[str]) -> Dict[str, Any]:
    """Find files matching pattern."""
    if not args:
        return {'error': 'Usage: find <pattern>'}

    pattern = args[0]
    try:
        matches = []
        search_root = project_manager.current_path

        for root, dirs, files in os.walk(search_root):
            for name in files:
                if pattern in name:
                    rel_path = Path(root) / name
                    matches.append(str(rel_path))

        if not matches:
            return {'message': f'No files matching "{pattern}" found.'}

        return {'message': '\n'.join(f"  {m}" for m in matches[:50])}
    except Exception as e:
        return {'error': f'find failed: {e}'}


def cd(args: List[str]) -> Dict[str, Any]:
    """Change current directory."""
    if not args:
        project_manager.current_path = _get_project_root()
        return {'message': f'Changed to project root: {project_manager.current_path}'}

    path = args[0]
    if path == '..':
        parent = project_manager.current_path.parent
        project_manager.current_path = parent
        return {'message': f'Changed to: {parent}'}

    new_path = project_manager.current_path / path
    if new_path.exists() and new_path.is_dir():
        project_manager.current_path = new_path.resolve()
        return {'message': f'Changed to: {project_manager.current_path}'}
    return {'error': f'Directory not found: {path}'}


def pwd(args: List[str]) -> Dict[str, Any]:
    """Print working directory."""
    return {'message': str(project_manager.current_path)}