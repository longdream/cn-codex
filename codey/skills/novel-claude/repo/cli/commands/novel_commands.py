"""Novel workflow commands for Novel-Claude CLI."""
import os
import sys
import subprocess
from pathlib import Path
from typing import List, Any, Dict
from cli.project_manager import project_manager


def handle(args: List[str]) -> Dict[str, Any]:
    """Handle novel command with no subcommand."""
    return {'message': 'Use: init <logline>, plan [volume], write --volume N --chapters X-Y'}


def init(args: List[str]) -> Dict[str, Any]:
    """Initialize world view."""
    if not args:
        return {'error': 'Usage: init <logline>'}

    logline = ' '.join(args)
    print(f"[INFO] Initializing world view: {logline[:50]}...")

    # Call world_builder
    try:
        from world_builder import run_world_builder
        run_world_builder(logline)
        return {'message': 'World view initialized successfully.'}
    except Exception as e:
        return {'error': f'init failed: {e}'}


def plan(args: List[str]) -> Dict[str, Any]:
    """Generate volume outline."""
    volume = None
    if args:
        try:
            volume = int(args[0])
        except ValueError:
            return {'error': f'Invalid volume number: {args[0]}'}

    try:
        if volume is None:
            from volume_planner import plan_macro_outlines
            plan_macro_outlines()
            return {'message': 'Macro outlines generated for all 10 volumes.'}
        else:
            from volume_planner import run_volume_planner
            run_volume_planner(volume)
            project_manager.update_context(volume=volume)
            return {'message': f'Outline for volume {volume} generated.'}
    except Exception as e:
        return {'error': f'plan failed: {e}'}


def write(args: List[str]) -> Dict[str, Any]:
    """Write chapters."""
    volume = None
    start_ch = None
    end_ch = None

    i = 0
    while i < len(args):
        if args[i] == '--volume' and i + 1 < len(args):
            try:
                volume = int(args[i + 1])
            except ValueError:
                return {'error': f'Invalid volume: {args[i + 1]}'}
            i += 2
        elif args[i] == '--chapters' and i + 1 < len(args):
            chapters = args[i + 1]
            if '-' in chapters:
                start_ch, end_ch = map(int, chapters.split('-'))
            else:
                start_ch = end_ch = int(chapters)
            i += 2
        else:
            i += 1

    if volume is None:
        return {'error': 'Usage: write --volume N --chapters X-Y'}

    try:
        from scene_writer import run_scene_writer
        run_scene_writer(volume, start_ch, end_ch)
        project_manager.update_context(volume=volume, chapter=end_ch)
        return {'message': f'Chapters {start_ch}-{end_ch} of volume {volume} written.'}
    except Exception as e:
        return {'error': f'write failed: {e}'}


def batch(args: List[str]) -> Dict[str, Any]:
    """Handle batch command."""
    if not args:
        return {'message': 'Use: batch build, batch submit, batch sync'}
    return {'error': f'Unknown batch subcommand: {args[0]}'}


def batch_build(args: List[str]) -> Dict[str, Any]:
    """Build batch JSONL."""
    volume = None
    start_ch = None
    end_ch = None

    i = 0
    while i < len(args):
        if args[i] == '--volume' and i + 1 < len(args):
            volume = int(args[i + 1])
            i += 2
        elif args[i] == '--chapters' and i + 1 < len(args):
            chapters = args[i + 1]
            if '-' in chapters:
                start_ch, end_ch = map(int, chapters.split('-'))
            else:
                start_ch = end_ch = int(chapters)
            i += 2
        else:
            i += 1

    if volume is None or start_ch is None:
        return {'error': 'Usage: batch_build --volume N --chapters X-Y'}

    try:
        from scene_writer import generate_batch_jsonl
        from utils.config import BATCH_DIR
        output_path = os.path.join(BATCH_DIR, f"vol_{volume:02d}_ch_{start_ch}_{end_ch}_req.jsonl")
        generate_batch_jsonl(volume, start_ch, end_ch, output_path)
        return {'message': f'Batch JSONL generated: {output_path}'}
    except Exception as e:
        return {'error': f'batch_build failed: {e}'}


def batch_submit(args: List[str]) -> Dict[str, Any]:
    """Submit batch task."""
    if not args:
        return {'error': 'Usage: batch_submit <jsonl_path>'}

    jsonl_path = args[0]
    if not os.path.exists(jsonl_path):
        return {'error': f'File not found: {jsonl_path}'}

    try:
        from utils.batch_client import submit_batch_task
        batch_id = submit_batch_task(jsonl_path, desc=f"Submit: {os.path.basename(jsonl_path)}")
        return {'message': f'Batch submitted. ID: {batch_id}'}
    except Exception as e:
        return {'error': f'batch_submit failed: {e}'}


def batch_sync(args: List[str]) -> Dict[str, Any]:
    """Sync batch results."""
    if not args:
        return {'error': 'Usage: batch_sync <batch_id>'}

    batch_id = args[0]
    try:
        from utils.batch_client import get_batch_status, download_batch_results
        from scene_writer import process_batch_results
        from utils.config import BATCH_DIR
        import time

        print(f"[INFO] Polling batch {batch_id}...")
        while True:
            status = get_batch_status(batch_id)
            cs = status.status
            print(f"  Status: {cs}")
            if cs == "completed":
                res_path = os.path.join(BATCH_DIR, f"{batch_id}_results.jsonl")
                err_path = os.path.join(BATCH_DIR, f"{batch_id}_errors.jsonl")
                if download_batch_results(batch_id, res_path, err_path):
                    process_batch_results(res_path)
                return {'message': 'Batch completed and merged.'}
            elif cs in ["failed", "cancelled", "expired"]:
                return {'error': f'Batch failed: {cs}'}
            time.sleep(60)
    except Exception as e:
        return {'error': f'batch_sync failed: {e}'}


def reindex(args: List[str]) -> Dict[str, Any]:
    """Reindex manuscripts to RAG."""
    volume = None
    start_ch = None
    end_ch = None

    i = 0
    while i < len(args):
        if args[i] == '--volume' and i + 1 < len(args):
            volume = int(args[i + 1])
            i += 2
        elif args[i] == '--chapters' and i + 1 < len(args):
            chapters = args[i + 1]
            if '-' in chapters:
                start_ch, end_ch = map(int, chapters.split('-'))
            else:
                start_ch = end_ch = int(chapters)
            i += 2
        else:
            i += 1

    if volume is None:
        return {'error': 'Usage: reindex --volume N --chapters X-Y'}

    try:
        from core.event_bus import event_bus
        from utils.config import MANUSCRIPTS_DIR

        for chap in range(start_ch, end_ch + 1):
            path = os.path.join(MANUSCRIPTS_DIR, f"vol_{volume:02d}", f"ch_{chap:03d}_final.md")
            if os.path.exists(path):
                with open(path, "r", encoding="utf-8") as f:
                    content = f.read()
                print(f"[REINDEX] Chapter {chap}...")
                beat_mock = {"chapter_id": chap, "beats": []}
                event_bus.emit("on_after_scene_write", beat_mock, content)
            else:
                print(f"[WARN] File not found: {path}")

        return {'message': f'Reindexed chapters {start_ch}-{end_ch}.'}
    except Exception as e:
        return {'error': f'reindex failed: {e}'}


# ============================================================================
# Audit Commands
# ============================================================================

def audit(args: List[str]) -> Dict[str, Any]:
    """Audit stage or chapter consistency."""
    target_type = None
    target_id = None

    i = 0
    while i < len(args):
        if args[i] == '--stage' and i + 1 < len(args):
            target_type = 'stage'
            target_id = int(args[i + 1])
            i += 2
        elif args[i] == '--chapter' and i + 1 < len(args):
            target_type = 'chapter'
            target_id = int(args[i + 1])
            i += 2
        else:
            i += 1

    if target_type is None:
        return {'error': 'Usage: audit --stage N, audit --chapter N'}

    try:
        from core.context_assembler import assemble_context
        from utils.llm_client import generate_stream
        from utils.config import VOLUMES_DIR
        import json

        if target_type == 'stage':
            # Load stage outline
            vol_id = project_manager.get_current_project()['context'].get('volume', 1)
            stage_path = Path(VOLUMES_DIR) / f"vol_{vol_id:02d}_stages" / f"stage_{target_id:02d}.json"
            if not stage_path.exists():
                return {'error': f'Stage outline not found: {stage_path}'}

            with open(stage_path, 'r', encoding='utf-8') as f:
                stage_data = json.load(f)

            prompt_path = Path("prompts/阶段审核.txt")
            if not prompt_path.exists():
                return {'error': 'Stage audit prompt not found'}

            with open(prompt_path, 'r', encoding='utf-8') as f:
                audit_prompt = f.read()

            context = assemble_context(audit_prompt, "stage_outline", stage_data)
            result = generate_stream(context)

            print(f"\n[Stage Audit] Stage {target_id}:")
            print(result)
            return {'message': f'Stage {target_id} audit complete.'}

        elif target_type == 'chapter':
            # Load chapter outline and content
            vol_id = project_manager.get_current_project()['context'].get('volume', 1)
            ch_outline_path = Path(VOLUMES_DIR) / f"vol_{vol_id:02d}_chapters" / f"ch_{target_id:03d}_outline.json"
            ch_content_path = Path(MANUSCRIPTS_DIR) / f"vol_{vol_id:02d}" / f"ch_{target_id:03d}_final.md"

            if not ch_outline_path.exists():
                return {'error': f'Chapter outline not found: {ch_outline_path}'}

            with open(ch_outline_path, 'r', encoding='utf-8') as f:
                chapter_data = json.load(f)

            chapter_content = ""
            if ch_content_path.exists():
                with open(ch_content_path, 'r', encoding='utf-8') as f:
                    chapter_content = f.read()

            prompt_path = Path("prompts/章节审核.txt")
            if not prompt_path.exists():
                return {'error': 'Chapter audit prompt not found'}

            with open(prompt_path, 'r', encoding='utf-8') as f:
                audit_prompt = f.read()

            # Inject chapter content into context
            chapter_data['content'] = chapter_content
            context = assemble_context(audit_prompt, "chapter_outline", chapter_data)
            result = generate_stream(context)

            print(f"\n[Chapter Audit] Chapter {target_id}:")
            print(result)
            return {'message': f'Chapter {target_id} audit complete.'}

    except Exception as e:
        import traceback
        traceback.print_exc()
        return {'error': f'audit failed: {e}'}


def track(args: List[str]) -> Dict[str, Any]:
    """Track entity states after chapter generation."""
    volume = None
    chapter = None

    i = 0
    while i < len(args):
        if args[i] == '--volume' and i + 1 < len(args):
            volume = int(args[i + 1])
            i += 2
        elif args[i] == '--chapter' and i + 1 < len(args):
            chapter = int(args[i + 1])
            i += 2
        else:
            i += 1

    if volume is None or chapter is None:
        return {'error': 'Usage: track --volume N --chapter N'}

    try:
        from core.entity_tracker import track_chapter_entities
        track_chapter_entities(volume, chapter)
        return {'message': f'Entity states for ch {chapter} tracked.'}
    except Exception as e:
        return {'error': f'track failed: {e}'}