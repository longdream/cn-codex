import json
import time
from zhipuai import ZhipuAI
from utils.config import _toml_config, ANTHROPIC_API_KEY

def _resolve_zhipu_key() -> str:
    """Prefer the zhipu provider key from config.toml; fall back to active provider key."""
    providers = _toml_config.get("model_providers", {})
    zhipu = providers.get("zhipu", {})
    key = zhipu.get("experimental_bearer_token", "")
    return key if key else ANTHROPIC_API_KEY

client = ZhipuAI(api_key=_resolve_zhipu_key())

def submit_batch_task(jsonl_file_path: str, endpoint: str = "/v4/chat/completions", desc: str = "") -> str:
    """上传文件并创建 Batch 任务，返回 batch_id"""
    # 1. 上传文件
    with open(jsonl_file_path, "rb") as f:
        file_object = client.files.create(file=f, purpose="batch")
    
    # 2. 创建 Batch 任务
    batch = client.batches.create(
        input_file_id=file_object.id,
        endpoint=endpoint,
        auto_delete_input_file=True,
        metadata={"description": desc}
    )
    return batch.id

def get_batch_status(batch_id: str):
    """获取任务当前状态"""
    return client.batches.retrieve(batch_id)

def download_batch_results(batch_id: str, output_path: str, error_path: str = None):
    """下载结果文件"""
    status = client.batches.retrieve(batch_id)
    if status.status == "completed":
        # 下载成功结果
        if status.output_file_id:
            content = client.files.content(status.output_file_id)
            content.write_to_file(output_path)
            print(f"[Batch] 结果已保存至: {output_path}")
            
        # 下载错误结果
        if status.error_file_id and error_path:
            err_content = client.files.content(status.error_file_id)
            err_content.write_to_file(error_path)
            print(f"[Batch] 错误日志已保存至: {error_path}")
        return True
    return False
