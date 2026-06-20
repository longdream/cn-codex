"""
Context Assembler - @DSL syntax for dynamic card reference injection.

Supports:
- @卡片标题 - reference by exact title
- @type:角色卡 - reference all cards of type
- @type:角色卡[previous] - sibling reference
- @parent - parent card
- @self - current card
- @type:角色卡[filter:expression] - filtered reference
"""

import re
from pathlib import Path
from typing import Dict, List, Any, Optional


class CardDatabase:
    """Simple card storage mimicking NovelForge's card system."""

    def __init__(self, base_dir: str = ".novel"):
        self.base_dir = Path(base_dir)
        self.cards: Dict[str, List[dict]] = {}  # card_type -> list of cards

    def load_all_cards(self):
        """Load all cards from the novel directory structure."""
        # Settings cards
        settings_dir = self.base_dir / "settings"
        if settings_dir.exists():
            for f in settings_dir.glob("*.json"):
                if f.stem in ["one_sentence", "story_outline", "world_setting", "core_blueprint"]:
                    with open(f, 'r', encoding='utf-8') as fp:
                        data = json.load(fp)
                        self.cards[f.stem] = [data]

        # Volume outlines
        volumes_dir = self.base_dir / "volumes"
        if volumes_dir.exists():
            for f in volumes_dir.glob("vol_*_outline.json"):
                import re
                m = re.match(r"vol_(\d+)_outline", f.stem)
                if m:
                    vol_id = int(m.group(1))
                    with open(f, 'r', encoding='utf-8') as fp:
                        data = json.load(fp)
                        if "volume_outlines" not in self.cards:
                            self.cards["volume_outlines"] = []
                        self.cards["volume_outlines"].append(data)

        # Chapter cards (for entity tracking)
        manuscripts_dir = self.base_dir / "manuscripts"
        if manuscripts_dir.exists():
            for vol_dir in manuscripts_dir.glob("vol_*"):
                for ch_file in vol_dir.glob("ch_*_outline.json"):
                    pass  # load chapter outlines


import json


class ContextAssembler:
    """Assembles context for AI prompts using @DSL syntax."""

    def __init__(self, base_dir: str = ".novel"):
        self.db = CardDatabase(base_dir)
        self.db.load_all_cards()  # Load cards on initialization
        self._cache = {}

    def assemble(self, template: str, current_card_type: str = None, current_card: dict = None) -> str:
        """
        Replace @DSL references in template with actual card content.

        Args:
            template: String with @DSL references
            current_card_type: Type of current card being generated
            current_card: Content of current card
        """
        if not template:
            return template

        # Note: Do NOT clear cache on every assemble
        result = template

        # Replace @self
        if current_card:
            result = result.replace("@self", json.dumps(current_card, ensure_ascii=False))

        # Replace @parent
        # (would need parent tracking in real implementation)

        # Replace @type:CardType
        type_pattern = r'@type:(\w+)(?:\[([^\]]+)\])?'
        def replace_type_ref(m):
            card_type = m.group(1)
            modifier = m.group(2) or ""

            cards = self._get_cards(card_type, modifier, current_card)
            return self._format_cards(cards)

        result = re.sub(type_pattern, replace_type_ref, result)

        # Replace @CardTitle
        title_pattern = r'@([^\s@]+?(?:[^\s@]+)*?)(?:\.content\.([\w]+))?'
        def replace_title_ref(m):
            title = m.group(1)
            field = m.group(2)
            # Find card by title
            for cards in self.db.cards.values():
                for card in cards:
                    if card.get("title") == title or card.get("name") == title:
                        if field:
                            return str(card.get("content", {}).get(field, ""))
                        return json.dumps(card, ensure_ascii=False)
            return f"/* Card not found: {title} */"

        result = re.sub(title_pattern, replace_title_ref, result)

        return result

    def _get_cards(self, card_type: str, modifier: str, current_card: dict) -> List[dict]:
        """Get cards matching type and modifier."""
        # Map our types to actual files
        type_map = {
            "金手指": "goldfinger",
            "一句话梗概": "one_sentence",
            "故事大纲": "story_outline",
            "世界观设定": "world_setting",
            "核心蓝图": "core_blueprint",
            "角色卡": "characters",
            "场景卡": "scenes",
            "组织卡": "organizations",
            "分卷大纲": "volume_outline",
            "阶段大纲": "stage_outline",
            "章节大纲": "chapter_outline",
            "章节正文": "chapter_content",
        }

        mapped_type = type_map.get(card_type, card_type)
        cards = []

        # Load from settings
        if mapped_type in ["goldfinger", "one_sentence", "story_outline", "world_setting", "core_blueprint"]:
            settings_file = Path(self.db.base_dir) / "settings" / f"{mapped_type}.json"
            if settings_file.exists():
                with open(settings_file, 'r', encoding='utf-8') as f:
                    cards.append(json.load(f))

        # Load characters/scenes/organizations from core_blueprint
        if mapped_type in ["characters", "scenes", "organizations"]:
            blueprint_file = Path(self.db.base_dir) / "settings" / "core_blueprint.json"
            if blueprint_file.exists():
                with open(blueprint_file, 'r', encoding='utf-8') as f:
                    data = json.load(f)
                    content = data.get("content", {})
                    if mapped_type in content:
                        cards = content[mapped_type]

        # Support filter expressions like [filter:name contains "X"]
        if modifier and modifier.startswith("filter:"):
            filter_expr = modifier[7:].strip()
            cards = self._apply_filter(cards, filter_expr)

        # Support [previous] modifier for sibling reference
        if modifier == "previous" and current_card:
            # Would need parent context to get previous sibling
            pass

        return cards

    def _apply_filter(self, cards: List[dict], filter_expr: str) -> List[dict]:
        """Apply filter expression to card list."""
        if not cards or not filter_expr:
            return cards

        # Simple filter: name contains "X"
        import re
        m = re.search(r'name\s+contains\s+"([^"]+)"', filter_expr, re.IGNORECASE)
        if m:
            keyword = m.group(1)
            return [c for c in cards if keyword.lower() in c.get("name", "").lower()]

        return cards

    def _format_cards(self, cards: List[dict]) -> str:
        """Format list of cards for prompt injection."""
        if not cards:
            return ""
        return json.dumps(cards, ensure_ascii=False, indent=2)


# Global instance
_assembler = None


def get_assembler(base_dir: str = ".novel") -> ContextAssembler:
    """Get or create global context assembler."""
    global _assembler
    if _assembler is None:
        _assembler = ContextAssembler(base_dir)
    return _assembler


def assemble_context(template: str, current_card_type: str = None, current_card: dict = None) -> str:
    """Convenience function for context assembly."""
    return get_assembler().assemble(template, current_card_type, current_card)