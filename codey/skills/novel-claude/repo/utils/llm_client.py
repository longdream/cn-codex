"""
LLM Client with progressive saving support
"""

import json
import time
from openai import OpenAI
from openai import APIError, APITimeoutError
from pydantic import ValidationError
from rich.live import Live
from rich.markdown import Markdown
from utils.config import MINIMAX_API_KEY, MINIMAX_BASE_URL, MODEL_ID, FLASH_MODEL_ID
from utils.config_loader import get_config

# Handle ConnectionError properly
try:
    from openai import ConnectionError as OpenAIConnectionError
except ImportError:
    OpenAIConnectionError = Exception

client = None

DONE_MARKER = '{"op":"done"}'


def _get_client():
    global client
    if client is None:
        client = OpenAI(
            api_key=MINIMAX_API_KEY,
            base_url=MINIMAX_BASE_URL,
            timeout=get_config("generation.timeout", 120),
            default_headers={"User-Agent": "cn-codex/1.0"},
        )
    return client


def _clean_response_content(content: str) -> str:
    if content is None:
        return ""
    # Remove the "done" marker and everything after it
    if DONE_MARKER in content:
        content = content.split(DONE_MARKER)[0].strip()
    if content.startswith("```json"):
        content = content[7:]
    elif content.startswith("```"):
        content = content[3:]
    if content.endswith("```"):
        content = content[:-3]
    return content.strip()


def generate_json(prompt: str, schema_model, system_message: str = "[\u4e13\u4e1a\u6570\u636e\u7ed3\u6784\u5316\u52a9\u624b]") -> dict:
    """Mode A: JSON structured output with retry"""
    max_retries = get_config("generation.max_retries", 3)
    for attempt in range(max_retries):
        try:
            messages = [
                {"role": "system", "content": system_message + "\nPlease output pure JSON strictly. Follow this JSON Schema:\n" + json.dumps(schema_model.model_json_schema(), ensure_ascii=False)},
                {"role": "user", "content": prompt}
            ]
            response = _get_client().chat.completions.create(
                model=MODEL_ID,
                messages=messages,
                temperature=0.1
            )
            content = _clean_response_content(response.choices[0].message.content)
            parsed_data = json.loads(content)
            schema_model.model_validate(parsed_data)
            return parsed_data

        except (json.JSONDecodeError, ValidationError) as e:
            if attempt == max_retries - 1:
                raise RuntimeError(f"Failed to generate valid JSON after {max_retries} attempts. Error: {str(e)}")
            prompt += f"\n\nPrevious output had format error: {str(e)}. Please fix and re-output pure JSON."
        except (APIError, APITimeoutError, OpenAIConnectionError) as e:
            if attempt == max_retries - 1:
                raise RuntimeError(f"API failed after retries: {str(e)}")
            print(f"\n[API中断] 第 {attempt + 1}/{max_retries} 次尝试失败，等待重试...")
            time.sleep(get_config("generation.retry_delay", 5) * (attempt + 1))


class ProgressiveWriter:
    """
    Streaming generator with progressive save support.
    """

    def __init__(self, on_progress=None, chunk_size: int = 1000):
        self.on_progress = on_progress
        self.chunk_size = chunk_size
        self.accumulated = []
        self.last_callback_count = 0

    def write(self, prompt, system_message: str = "You are a top web novel writer.", chapter_id: int = None):
        max_retries = get_config("generation.max_retries", 3)
        retry_delay = get_config("generation.retry_delay", 5)

        for attempt in range(max_retries):
            try:
                return self._write_impl(prompt, system_message, chapter_id)
            except (APIError, APITimeoutError, OpenAIConnectionError) as e:
                print(f"\n[API中断] 第 {attempt + 1}/{max_retries} 次尝试失败: {str(e)[:100]}")
                if attempt < max_retries - 1:
                    print(f"[等待] {retry_delay} 秒后重试...")
                    time.sleep(retry_delay)
                    retry_delay *= 2
                else:
                    print(f"[放弃] 已达到最大重试次数 {max_retries}")
                    raise RuntimeError(f"API failed after retries: {str(e)}")

    def _write_impl(self, prompt, system_message: str, chapter_id: int = None):
        if isinstance(prompt, list):
            prompt_content = "\n".join(prompt)
        else:
            prompt_content = str(prompt)

        messages = [
            {"role": "system", "content": system_message},
            {"role": "user", "content": prompt_content}
        ]

        kwargs = {
            "model": MODEL_ID,
            "messages": messages,
            "temperature": get_config("generation.temperature", 0.85),
            "stream": True
        }

        response = _get_client().chat.completions.create(**kwargs)

        self.accumulated = []
        self.last_callback_count = 0

        with Live(auto_refresh=False, vertical_overflow="visible") as live:
            for chunk in response:
                delta = chunk.choices[0].delta

                if hasattr(delta, 'content') and delta.content:
                    self.accumulated.append(delta.content)
                    accumulated_text = "".join(self.accumulated)

                    if self.on_progress and len(accumulated_text) - self.last_callback_count >= self.chunk_size:
                        self.last_callback_count = len(accumulated_text)
                        self.on_progress(chapter_id, accumulated_text, len(accumulated_text))

                    live.update(Markdown(accumulated_text), refresh=True)

        final_result = "".join(self.accumulated)
        if DONE_MARKER in final_result:
            final_result = final_result.split(DONE_MARKER)[0].strip()

        if self.on_progress:
            self.on_progress(chapter_id, final_result, len(final_result))

        return final_result


def generate_stream(prompt, system_message: str = "You are a top web novel writer.", tools: list = None):
    writer = ProgressiveWriter()
    return writer.write(prompt, system_message, None)


def extract_entities(prompt: str) -> list[str]:
    messages = [
        {"role": "system", "content": "Entity extraction engine. Extract person names, skills, artifacts, locations from text. Output JSON list like [\"name1\", \"name2\"]. No explanations."},
        {"role": "user", "content": f"Extract entities from: {prompt}"}
    ]

    try:
        response = _get_client().chat.completions.create(
            model=FLASH_MODEL_ID,
            messages=messages,
            temperature=0.1
        )
        content = _clean_response_content(response.choices[0].message.content)
    except Exception as e:
        if "1301" in str(e):
            return []
        print(f"[WARN] Entity extraction failed: {e}")
        return []

    try:
        entities = json.loads(content)
        if isinstance(entities, list):
            return entities
        return []
    except json.JSONDecodeError:
        return [e.strip() for e in content.split(",") if e.strip()]