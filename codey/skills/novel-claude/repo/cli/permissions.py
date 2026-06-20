"""Permission system for Novel-Claude CLI."""
from enum import Enum
from pathlib import Path
import json
from typing import Optional

# Permission levels
class PermissionLevel(Enum):
    NONE = 0        # No file operations
    READ = 1        # Can read .novel workspace only
    WRITE = 2       # Can read/write .novel workspace
    FULL = 3        # Can execute any file operation


class PermissionManager:
    """Manages file operation permissions for the CLI."""

    def __init__(self):
        self.level = PermissionLevel.READ  # Default to READ
        self.config_file = Path(__file__).parent.parent.parent / ".novel_cli_config" / "permissions.json"
        self._load()

    def _load(self):
        """Load permissions from config."""
        self.config_file.parent.mkdir(parents=True, exist_ok=True)
        if self.config_file.exists():
            try:
                with open(self.config_file, 'r', encoding='utf-8') as f:
                    data = json.load(f)
                    level = data.get('level', 'READ')
                    self.level = PermissionLevel[level]
            except (json.JSONDecodeError, KeyError):
                pass

    def _save(self):
        """Save permissions to config."""
        with open(self.config_file, 'w', encoding='utf-8') as f:
            json.dump({'level': self.level.name}, f, ensure_ascii=False, indent=2)

    def set_level(self, level: PermissionLevel):
        """Set permission level."""
        self.level = level
        self._save()

    def can_read(self, path: Path) -> bool:
        """Check if read is allowed for path."""
        if self.level == PermissionLevel.FULL:
            return True
        if self.level == PermissionLevel.NONE:
            return False
        # READ and WRITE can access .novel workspace
        return str(path).startswith('.novel')

    def can_write(self, path: Path) -> bool:
        """Check if write is allowed for path."""
        if self.level == PermissionLevel.FULL:
            return True
        if self.level in [PermissionLevel.NONE, PermissionLevel.READ]:
            return False
        return str(path).startswith('.novel')

    def check_read(self, path: Path) -> bool:
        """Raise error if read not allowed."""
        if not self.can_read(path):
            raise PermissionError(f"Read not allowed for: {path}")
        return True

    def check_write(self, path: Path) -> bool:
        """Raise error if write not allowed."""
        if not self.can_write(path):
            raise PermissionError(f"Write not allowed for: {path}")
        return True


# Global instance
permissions = PermissionManager()