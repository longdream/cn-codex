"""Quick test to verify chapter outline loading."""
import sys
sys.stdout.reconfigure(encoding='utf-8')
sys.path.insert(0, '.')
from scene_writer import load_chapter_outline, _global_chapter_id

# Test conversion
gid = _global_chapter_id(2, 1)
print(f'volume_id=2, chapter_id=1 -> global_id={gid}')

# Test Load ch1
outline = load_chapter_outline(2, 1)
if outline:
    print(f'SUCCESS ch1: {outline.get("title")}')
else:
    print('FAILED: could not load ch1')

# Test Load ch30 (last chapter)
outline2 = load_chapter_outline(2, 30)
if outline2:
    print(f'SUCCESS ch30: {outline2.get("title")}')
else:
    print('FAILED: could not load ch30')
