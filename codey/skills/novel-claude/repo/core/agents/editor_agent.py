import json
import os
from utils.llm_client import _get_client, MODEL_ID

class EditorAgent:
    """
    负责对 s03 生成的初稿进行严厉审查和修改的复杂智能体。
    它具有多轮思考 (ReAct) 的能力。
    """
    def __init__(self, max_iterations=3):
        self.name = "ToxicEditorAgent"
        self.max_iterations = max_iterations
        
        default_prompt = """你是一位极其严苛的白金网文主编。
        你的任务是审查作者提交的多个场景拼接成的初稿，并对其进行整体润色修改。
        你需要消除场景之间的割裂感，平滑自然段过渡，并修复视角跳跃。
        如果需要，直接重写不合理的部分。你必须思考 (thought) 然后采取行动 (action)。"""
        
        self.system_prompt = os.getenv("PROMPT_S03_EDITOR", default_prompt)

    def get_tools(self):
        return [{
            "type": "function",
            "function": {
                "name": "submit_final_revision",
                "description": "当你完成了审稿、润色、修改（消除割裂感和视角跳跃）后，调用此工具提交符合字数要求的最终定稿小说正文。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "critique": {"type": "string", "description": "你对刚才这段草稿的评价和做出的修改说明"},
                        "final_text": {"type": "string", "description": "修改润色后可以直接发布的小说正文内容（不要包含任何评论）"}
                    },
                    "required": ["critique", "final_text"]
                }
            }
        }]

    def run(self, raw_draft: str, beat_requirements: str) -> str:
        messages = [
            {"role": "system", "content": self.system_prompt},
            {"role": "user", "content": f"【大纲要求】：\\n{beat_requirements}\\n\\n【流水线初稿内容】：\\n{raw_draft}\\n\\n请严格审查，抹除 '***' 分界符，并调用 submit_final_revision 工具提交最终修改结果。"}
        ]
        
        print(f"\\n[🤖 {self.name}] 开始审阅并精修文稿...")
        
        for iteration in range(self.max_iterations):
            print(f"  -> 第 {iteration + 1} 轮推理...")
            
            response = _get_client().chat.completions.create(
                model=MODEL_ID,
                messages=messages,
                tools=self.get_tools(),
                temperature=0.3
            )
            
            msg = response.choices[0].message
            
            # 记录思考过程
            if msg.content:
                print(f"  [思考]: {msg.content.strip()}")
                messages.append({"role": "assistant", "content": msg.content})
                
            # 执行工具调用
            if getattr(msg, 'tool_calls', None):
                tool_call = msg.tool_calls[0]
                args = json.loads(tool_call.function.arguments)
                
                if tool_call.function.name == "submit_final_revision":
                    try:
                        from rich.console import Console
                        Console().print(f"[bold green]  [主编长评]:[/bold green] {args.get('critique', '')}")
                    except:
                        print(f"  [主编长评]: {args.get('critique', '')}")
                    
                    print(f"[🤖 {self.name}] 定稿完成。")
                    return args.get("final_text", raw_draft)
                    
                messages.append({
                    "role": "assistant",
                    "content": msg.content,
                    "tool_calls": msg.tool_calls
                })
                messages.append({
                    "role": "tool",
                    "tool_call_id": tool_call.id,
                    "name": tool_call.function.name,
                    "content": "工具不符合预期，请专门调用 'submit_final_revision'。"
                })
                continue
                
            # 如果没调用工具也结束了
            if iteration == self.max_iterations - 1:
                return msg.content if msg.content else raw_draft

        print(f"[🤖 {self.name}] 达到最大交互次数兜底返回。")
        return raw_draft
