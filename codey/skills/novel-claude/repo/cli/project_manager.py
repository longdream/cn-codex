"""Project management for Novel-Claude CLI."""
import os
import shutil
import json
from pathlib import Path
from typing import Optional, List

# Base directory for all novel projects
NOVEL_BASE_DIR = Path(__file__).parent.parent / ".novel_projects"

# Config file to track last active project
CONFIG_DIR = Path(__file__).parent.parent / ".novel_cli_config"
CONFIG_FILE = CONFIG_DIR / "state.json"


class ProjectManager:
    """Manages multiple novel projects."""

    def __init__(self):
        self.current_project: Optional[str] = None
        self.current_volume: int = 1
        self.current_chapter: int = 1
        self.current_path: Path = Path.cwd()
        self._load_state()

    def _load_state(self):
        """Load saved state from config file."""
        CONFIG_DIR.mkdir(parents=True, exist_ok=True)
        if CONFIG_FILE.exists():
            try:
                with open(CONFIG_FILE, 'r', encoding='utf-8') as f:
                    state = json.load(f)
                    self.current_project = state.get('current_project')
                    self.current_volume = state.get('current_volume', 1)
                    self.current_chapter = state.get('current_chapter', 1)
                    self.current_path = Path(state.get('current_path', str(Path.cwd())))
            except (json.JSONDecodeError, KeyError):
                pass

    def _save_state(self):
        """Save current state to config file."""
        state = {
            'current_project': self.current_project,
            'current_volume': self.current_volume,
            'current_chapter': self.current_chapter,
            'current_path': str(self.current_path)
        }
        with open(CONFIG_FILE, 'w', encoding='utf-8') as f:
            json.dump(state, f, ensure_ascii=False, indent=2)

    def create_project(self, name: str, logline: str = "") -> bool:
        """Create a new novel project."""
        project_dir = NOVEL_BASE_DIR / f".novel_{name}"
        if project_dir.exists():
            return False

        # Create project structure
        project_dir.mkdir(parents=True, exist_ok=True)
        (project_dir / "settings").mkdir(exist_ok=True)
        (project_dir / "volumes").mkdir(exist_ok=True)
        (project_dir / "manuscripts").mkdir(exist_ok=True)
        (project_dir / "memory").mkdir(exist_ok=True)
        (project_dir / "batch_jobs").mkdir(exist_ok=True)

        # Save project metadata
        meta = {
            'name': name,
            'logline': logline,
            'created_at': str(project_dir.stat().st_mtime)
        }
        with open(project_dir / 'project.json', 'w', encoding='utf-8') as f:
            json.dump(meta, f, ensure_ascii=False, indent=2)

        # Set as current project
        self.current_project = name
        self._save_state()
        return True

    def switch_project(self, name: str) -> bool:
        """Switch to a different project."""
        project_dir = NOVEL_BASE_DIR / f".novel_{name}"
        if not project_dir.exists():
            return False
        self.current_project = name
        self._save_state()
        return True

    def list_projects(self) -> List[str]:
        """List all existing projects."""
        if not NOVEL_BASE_DIR.exists():
            return []
        return [d.name.replace('.novel_', '') for d in NOVEL_BASE_DIR.iterdir() if d.is_dir() and d.name.startswith('.novel_')]

    def delete_project(self, name: str) -> bool:
        """Delete a project and all its data."""
        project_dir = NOVEL_BASE_DIR / f".novel_{name}"
        if not project_dir.exists():
            return False
        shutil.rmtree(project_dir)
        if self.current_project == name:
            self.current_project = None
            self._save_state()
        return True

    def get_project_info(self, name: str) -> Optional[dict]:
        """Get project metadata."""
        project_dir = NOVEL_BASE_DIR / f".novel_{name}"
        meta_file = project_dir / 'project.json'
        if not meta_file.exists():
            return None
        with open(meta_file, 'r', encoding='utf-8') as f:
            return json.load(f)

    def get_project_dir(self, name: Optional[str] = None) -> Path:
        """Get the project directory path."""
        name = name or self.current_project
        if not name:
            return Path.cwd()
        return NOVEL_BASE_DIR / f".novel_{name}"

    def update_context(self, volume: int = None, chapter: int = None):
        """Update current volume/chapter context."""
        if volume is not None:
            self.current_volume = volume
        if chapter is not None:
            self.current_chapter = chapter
        self._save_state()

    def cd(self, path: str) -> bool:
        """Change current working path within project."""
        new_path = self.current_path / path
        if new_path.exists() and new_path.is_dir():
            self.current_path = new_path.resolve()
            self._save_state()
            return True
        return False


# Global instance
project_manager = ProjectManager()