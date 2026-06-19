"""
Chapter State Manager - Progressive saving and state machine management

Tracks chapter generation state and enables resume from interruption.
"""

import json
import os
from pathlib import Path
from typing import Optional, Dict, List
from datetime import datetime
from utils.config import VOLUMES_DIR, MANUSCRIPTS_DIR
from utils.config_loader import get_config


# Chapter states
STATE_PENDING = "pending"
STATE_GENERATING = "generating"
STATE_COMPLETED = "completed"
STATE_FAILED = "failed"


class ChapterState:
    """State for a single chapter"""
    def __init__(self, volume_id: int, chapter_id: int):
        self.volume_id = volume_id
        self.chapter_id = chapter_id
        self.state = STATE_PENDING
        self.generated_chars = 0
        self.last_updated = None
        self.error_message = ""
        self.retry_count = 0

    def to_dict(self) -> dict:
        return {
            "volume_id": self.volume_id,
            "chapter_id": self.chapter_id,
            "state": self.state,
            "generated_chars": self.generated_chars,
            "last_updated": self.last_updated,
            "error_message": self.error_message,
            "retry_count": self.retry_count
        }

    @classmethod
    def from_dict(cls, data: dict) -> "ChapterState":
        state = cls(data["volume_id"], data["chapter_id"])
        state.state = data.get("state", STATE_PENDING)
        state.generated_chars = data.get("generated_chars", 0)
        state.last_updated = data.get("last_updated")
        state.error_message = data.get("error_message", "")
        state.retry_count = data.get("retry_count", 0)
        return state


class ChapterStateManager:
    """Manages states for all chapters in a volume"""

    def __init__(self, volume_id: int):
        self.volume_id = volume_id
        self.state_file = Path(VOLUMES_DIR) / f"vol_{volume_id:02d}_chapter_states.json"
        self.chapters: Dict[int, ChapterState] = {}
        self._load()

    def _load(self):
        """Load states from file"""
        if self.state_file.exists():
            with open(self.state_file, 'r', encoding='utf-8') as f:
                data = json.load(f)
                for chapter_data in data.get("chapters", []):
                    state = ChapterState.from_dict(chapter_data)
                    self.chapters[state.chapter_id] = state

    def _save(self):
        """Save states to file"""
        data = {
            "volume_id": self.volume_id,
            "last_updated": datetime.now().isoformat(),
            "chapters": [s.to_dict() for s in self.chapters.values()]
        }
        with open(self.state_file, 'w', encoding='utf-8') as f:
            json.dump(data, f, ensure_ascii=False, indent=2)

    def get_state(self, chapter_id: int) -> ChapterState:
        """Get or create state for a chapter"""
        if chapter_id not in self.chapters:
            self.chapters[chapter_id] = ChapterState(self.volume_id, chapter_id)
        return self.chapters[chapter_id]

    def set_state(self, chapter_id: int, state: str, error_message: str = ""):
        """Update chapter state"""
        s = self.get_state(chapter_id)
        s.state = state
        s.last_updated = datetime.now().isoformat()
        if error_message:
            s.error_message = error_message
        if state == STATE_FAILED:
            s.retry_count += 1
        self._save()

    def update_progress(self, chapter_id: int, chars: int):
        """Update generation progress (for progressive saving)"""
        s = self.get_state(chapter_id)
        s.generated_chars = chars
        s.last_updated = datetime.now().isoformat()
        self._save()

    def mark_completed(self, chapter_id: int):
        """Mark chapter as completed"""
        self.set_state(chapter_id, STATE_COMPLETED)

    def mark_failed(self, chapter_id: int, error: str):
        """Mark chapter as failed"""
        self.set_state(chapter_id, STATE_FAILED, error)

    def mark_generating(self, chapter_id: int):
        """Mark chapter as currently generating"""
        self.set_state(chapter_id, STATE_GENERATING)

    def get_pending_chapters(self) -> List[int]:
        """Get list of chapters that need to be generated"""
        pending = []
        for ch_id, state in sorted(self.chapters.items()):
            if state.state in [STATE_PENDING, STATE_FAILED]:
                pending.append(ch_id)
        return pending

    def get_completed_count(self) -> int:
        """Get count of completed chapters"""
        return sum(1 for s in self.chapters.values() if s.state == STATE_COMPLETED)

    def get_failed_count(self) -> int:
        """Get count of failed chapters"""
        return sum(1 for s in self.chapters.values() if s.state == STATE_FAILED)


def get_state_manager(volume_id: int) -> ChapterStateManager:
    """Get or create state manager for a volume"""
    return ChapterStateManager(volume_id)