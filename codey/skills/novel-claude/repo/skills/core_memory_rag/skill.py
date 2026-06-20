import os
import re
import json
from typing import List

import chromadb
import ahocorasick
from chromadb.utils import embedding_functions
from zhipuai import ZhipuAI

from core.base_skill import BaseSkill
from core.novel_context import NovelContext
from utils.config import MEMORY_DIR, SETTINGS_DIR, ANTHROPIC_API_KEY, _toml_config, register_background_task
from utils.llm_client import _get_client, MODEL_ID

def _resolve_zhipu_key() -> str:
    providers = _toml_config.get("model_providers", {})
    zhipu = providers.get("zhipu", {})
    key = zhipu.get("experimental_bearer_token", "")
    return key if key else ANTHROPIC_API_KEY

class ZhipuEmbeddingFunction(embedding_functions.EmbeddingFunction):
    def __init__(self):
        self.client = ZhipuAI(api_key=_resolve_zhipu_key())
        
    def __call__(self, input: list[str]) -> list[list[float]]:
        embeddings = []
        for text in input:
            res = self.client.embeddings.create(model="embedding-3", input=text)
            embeddings.append(res.data[0].embedding)
        return embeddings

class CoreMemoryRagSkill(BaseSkill):
    """
    原 s04_memory_rag.py 的能力插件化实现。
    负责在生成前检索记忆（on_before_scene_write），以及在生成后将文本切块向量入库（on_after_scene_write）。
    """
    def __init__(self, context: NovelContext):
        super().__init__(context)
        self.name = "CoreMemoryRagSkill"
        self.chroma_client = None
        self.collection = None
        self.automaton = None

    def on_init(self) -> None:
        self.chroma_client = chromadb.PersistentClient(path=MEMORY_DIR)
        emb_fn = ZhipuEmbeddingFunction()
        self.collection = self.chroma_client.get_or_create_collection(name="novel_memory", embedding_function=emb_fn)
        self.automaton = self._build_entity_automaton()

    def _build_entity_automaton(self):
        A = ahocorasick.Automaton()
        entities = []
        
        char_path = os.path.join(SETTINGS_DIR, "main_characters.json")
        if os.path.exists(char_path):
            with open(char_path, "r", encoding="utf-8") as f:
                data = json.load(f)
                for c in data.get("characters", []):
                    entities.append(c["name"])
                    
        fac_path = os.path.join(SETTINGS_DIR, "factions.json")
        if os.path.exists(fac_path):
            with open(fac_path, "r", encoding="utf-8") as f:
                data = json.load(f)
                for fac in data.get("factions", []):
                    entities.append(fac.get("name", ""))
                    
        for idx, entity in enumerate(set(entities)):
            if entity.strip():
                A.add_word(entity, (idx, entity))
                
        A.make_automaton()
        return A

    def _extract_entities_fast(self, text: str) -> list:
        if not text or not self.automaton:
            return []
        found = [item[1][1] for item in self.automaton.iter(text)]
        return list(set(found))

    def _condense_state(self, entity: str, context_chunks: list[str]) -> str:
        prompt = f"你是一个情报总结官。根据以下小说文本片段，极简总结实体【{entity}】的最新状态（例如：伤势、法宝受损情况、对其余人物的恨意等）。不要编造，如果文本没提就回复“状态正常”。\n\n" + "\n\n---\n\n".join(context_chunks)
        messages = [{"role": "user", "content": prompt}]
        res = _get_client().chat.completions.create(model=MODEL_ID, messages=messages, temperature=0.1)
        return res.choices[0].message.content.strip()

    def on_before_scene_write(self, prompt_payload: List[str], beat_data: dict) -> List[str]:
        entities = self._extract_entities_fast(beat_data.get('plot_summary', ''))
        if not entities:
            return prompt_payload
            
        try:
            from rich.console import Console
            Console().print(f"[bold cyan]  [Memory Bus][/bold cyan] 嗅探到关键实体: {entities}，正在检索最新状态...")
        except:
            print(f"  [Memory Bus] 嗅探到关键实体: {entities}，正在检索最新状态...")

        recent_memories = []
        for entity in entities:
            results = self.collection.query(
                query_texts=[entity],
                n_results=5
            )
            
            if results and results['documents'] and len(results['documents'][0]) > 0:
                docs = results['documents'][0]
                metadatas = results['metadatas'][0]
                
                combined = list(zip(docs, metadatas))
                combined.sort(key=lambda x: x[1].get('chapter_id', 0), reverse=True)
                
                recent_chunks = [c[0] for c in combined[:3]]
                condensed = self._condense_state(entity, recent_chunks)
                if condensed and "状态正常" not in condensed:
                    recent_memories.append(f"- {entity}: {condensed}")
                    
        if recent_memories:
            xml_memory = "<recent_memory>\n[实体最新状态同步]\n"
            xml_memory += "\n".join(recent_memories)
            xml_memory += "\n</recent_memory>\n"
            prompt_payload.append(xml_memory)
            
        return prompt_payload

    def on_after_scene_write(self, beat_data: dict, raw_text: str) -> None:
        chapter_id = self.context.current_chapter_id
        # 使用 EventBus 触发后台记录
        register_background_task(self._background_update_task, chapter_id, raw_text)

    def chunk_text(self, text: str) -> list[str]:
        raw_chunks = re.split(r'\*\*\*|\n\s*\n', text)
        chunks = []
        current_chunk = ""
        for rc in raw_chunks:
            rc = rc.strip()
            if not rc: continue
            if len(current_chunk) + len(rc) < 200:
                current_chunk += "\n" + rc if current_chunk else rc
            else:
                if current_chunk:
                    chunks.append(current_chunk)
                current_chunk = rc
        if current_chunk:
            chunks.append(current_chunk)
        return chunks

    def _background_update_task(self, chapter_id: int, final_content: str):
        try:
            from rich.console import Console
            Console().print(f"[dim]  [Background Task] 正在将第 {chapter_id} 章内容向量化并入库...[/dim]")
        except:
            print(f"  [Background Task] 正在将第 {chapter_id} 章内容向量化并入库...")
            
        try:
            chunks = self.chunk_text(final_content)
            ids = []
            documents = []
            metadatas = []
            
            for i, chunk in enumerate(chunks):
                chunk_entities = self._extract_entities_fast(chunk)
                involved = ",".join(chunk_entities) if chunk_entities else ""
                documents.append(chunk)
                ids.append(f"ch_{chapter_id}_chunk_{i}")
                metadatas.append({
                    "chapter_id": chapter_id,
                    "involved_entities": involved
                })
                    
            if documents:
                self.collection.add(
                    ids=ids,
                    documents=documents,
                    metadatas=metadatas
                )
        except Exception as e:
            print(f"[WARN] 后台向量化任务失败 (Ch_{chapter_id}): {e}")
