"""Constants for CLI commands."""
from pathlib import Path

# Path to .env file in project root
ENV_PATH = Path(__file__).parent.parent.parent / "env"

# Default prompt for AI review
DEFAULT_PROMPT_REVIEW = """你是一个专业的网文审稿编辑。用户会给一份或多份文件的内容，以及他们的修改意见（instruction）。
请根据修改意见，输出修改后的完整内容。修改时要注意：
1. 保持原有的格式和结构
2. 只修改与意见相关的部分，不要改动其他内容
3. 如果认为不需要修改，请原样输出该文件
4. 严格按照指定的格式输出每个文件的修改后内容

当前修改意见：{instruction}"""