import threading
import json
from pathlib import Path

class WorkspaceManager:
    """Provides thread-safe file operations for the .novel workspace to support high concurrency."""
    def __init__(self, base_dir=".novel"):
        self.base_dir = Path(base_dir)
        self._lock = threading.Lock()
        
        # Ensure base directory exists
        self.base_dir.mkdir(parents=True, exist_ok=True)

    def safe_write_json(self, rel_path: str, data: dict):
        """Thread-safe JSON write."""
        with self._lock:
            target = self.base_dir / rel_path
            target.parent.mkdir(parents=True, exist_ok=True)
            with open(target, 'w', encoding='utf-8') as f:
                json.dump(data, f, ensure_ascii=False, indent=2)
                
    def safe_read_json(self, rel_path: str) -> dict:
        """Thread-safe JSON read. Returns empty dict if file is missing."""
        with self._lock:
            target = self.base_dir / rel_path
            if target.exists():
                with open(target, 'r', encoding='utf-8') as f:
                    return json.load(f)
            return {}

    def safe_write_text(self, rel_path: str, content: str):
        """Thread-safe purely text write."""
        with self._lock:
            target = self.base_dir / rel_path
            target.parent.mkdir(parents=True, exist_ok=True)
            with open(target, 'w', encoding='utf-8') as f:
                f.write(content)
                
    def safe_read_text(self, rel_path: str) -> str:
        """Thread-safe text read. Returns empty string if file is missing."""
        with self._lock:
            target = self.base_dir / rel_path
            if target.exists():
                with open(target, 'r', encoding='utf-8') as f:
                    return f.read()
            return ""

workspace = WorkspaceManager()
