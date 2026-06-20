"""Settings commands for Novel-Claude CLI."""
from typing import List, Any, Dict
from dotenv import set_key, load_dotenv
import os


def handle(args: List[str]) -> Dict[str, Any]:
    """Handle settings command with no subcommand."""
    return {'message': 'Use: settings show, settings set <key> <value>'}


def show(args: List[str]) -> Dict[str, Any]:
    """Show current settings."""
    try:
        from utils.config import (
            ANTHROPIC_BASE_URL, ANTHROPIC_API_KEY, MODEL_ID,
            NOVEL_NAME, NOVEL_DIR
        )

        output = ["Current Configuration:"]
        output.append(f"  ANTHROPIC_BASE_URL: {ANTHROPIC_BASE_URL}")
        output.append(f"  MODEL_ID: {MODEL_ID}")
        output.append(f"  NOVEL_NAME: {NOVEL_NAME or '(not set)'}")
        output.append(f"  NOVEL_DIR: {NOVEL_DIR}")

        return {'message': '\n'.join(output)}
    except Exception as e:
        return {'error': f'settings show failed: {e}'}


def set_value(args: List[str]) -> Dict[str, Any]:
    """Set a config value."""
    if len(args) < 2:
        return {'error': 'Usage: settings set <key> <value>'}

    key = args[0]
    value = args[1]

    try:
        from cli.commands.constants import ENV_PATH
        set_key(ENV_PATH, key, value)
        load_dotenv(ENV_PATH, override=True)
        return {'message': f'{key} set to {value}'}
    except Exception as e:
        return {'error': f'settings set failed: {e}'}