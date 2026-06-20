"""Project management commands for Novel-Claude CLI."""
from typing import List, Any, Dict
from cli.project_manager import project_manager


def handle(args: List[str]) -> Dict[str, Any]:
    """Handle projects command with no subcommand."""
    return {
        'message': '''Projects Commands:
  projects create <name> <logline>  - Create a new project
  projects switch <name>           - Switch to a project
  projects list                     - List all projects
  projects info                     - Show current project info
  projects delete <name>            - Delete a project'''
    }


def create(args: List[str]) -> Dict[str, Any]:
    """Create a new project."""
    if len(args) < 1:
        return {'error': 'Usage: projects create <name> [logline]'}
    name = args[0]
    logline = args[1] if len(args) > 1 else ""

    success = project_manager.create_project(name, logline)
    if success:
        return {'message': f'Project "{name}" created and set as current.'}
    return {'error': f'Project "{name}" already exists.'}


def switch_project(args: List[str]) -> Dict[str, Any]:
    """Switch to a different project."""
    if len(args) < 1:
        return {'error': 'Usage: projects switch <name>'}
    name = args[0]

    success = project_manager.switch_project(name)
    if success:
        return {'message': f'Switched to project "{name}".'}
    return {'error': f'Project "{name}" not found.'}


def list_projects(args: List[str]) -> Dict[str, Any]:
    """List all projects."""
    projects = project_manager.list_projects()
    current = project_manager.current_project

    if not projects:
        return {'message': 'No projects found. Use "projects create <name>" to create one.'}

    output = ["Available projects:"]
    for p in projects:
        marker = " (current)" if p == current else ""
        output.append(f"  - {p}{marker}")

    return {'message': '\n'.join(output)}


def delete_project(args: List[str]) -> Dict[str, Any]:
    """Delete a project."""
    if len(args) < 1:
        return {'error': 'Usage: projects delete <name>'}
    name = args[0]

    if name == project_manager.current_project:
        return {'error': 'Cannot delete current project. Switch to another first.'}

    success = project_manager.delete_project(name)
    if success:
        return {'message': f'Project "{name}" deleted.'}
    return {'error': f'Project "{name}" not found.'}


def info(args: List[str]) -> Dict[str, Any]:
    """Show current project info."""
    name = project_manager.current_project
    if not name:
        return {'message': 'No project selected. Use "projects create" or "projects switch".'}

    meta = project_manager.get_project_info(name)
    if not meta:
        return {'error': f'Project "{name}" metadata not found.'}

    output = [f"Project: {name}"]
    output.append(f"Logline: {meta.get('logline', 'N/A')}")
    output.append(f"Created: {meta.get('created_at', 'N/A')}")

    project_dir = project_manager.get_project_dir(name)
    output.append(f"Location: {project_dir}")

    return {'message': '\n'.join(output)}