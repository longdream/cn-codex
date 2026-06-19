import click
import os
import sys

# Check for interactive mode flag BEFORE importing other modules
if '--interactive' in sys.argv or '-i' in sys.argv:
    from cli.repl import start_repl
    start_repl()
    sys.exit(0)

from world_builder import run_world_builder
from volume_planner import run_volume_planner, plan_macro_outlines
from scene_writer import run_scene_writer
from utils.config import wait_for_background_tasks

@click.group()
def cli():
    """🚀 Novel-Claude V3 — 微内核插件生态网文生成系统
    
    集成世界观初始化、分卷大纲规划、多智能体场景并写作、RAG 动态记忆管理。
    支持标准 API 实时码字、Batch API 大规模并发码字（5 折优惠）、以及 V3 Skill 插件生态。
    """
    from utils.config import NOVEL_NAME, NOVEL_DIR
    current_proj = NOVEL_NAME if NOVEL_NAME else "Default (未指定)"
    click.echo(click.style(f"当前激活项目: {current_proj}", fg="cyan", bold=True))
    
    # ── V3 核心挂载 ──
    from core.novel_context import NovelContext
    from core.plugin_manager import PluginManager
    from utils.workspace import WorkspaceManager
    
    workspace = WorkspaceManager(NOVEL_DIR)
    context = NovelContext(workspace)
    plugin_mgr = PluginManager(context)
    plugin_mgr.scan_and_load()

@cli.command()
@click.argument('logline')
def init(logline):
    """【阶段 1】初始化世界观与核心设定。
    
    LOGLINE: 一句关于该小说的核心创意或简介。系统将据此生成规则、力量体系、主要角色与势力分布。
    
    示例: uv run python cli.py init "一个在修真界利用赛博插件强开灵根的科幻转玄幻故事"
    """
    run_world_builder(logline)
    wait_for_background_tasks()

@cli.command()
@click.option('--volume', type=int, default=None, help='指定的卷号(1-10)。如果为空，则执行全书 10 卷的宏观大纲规划。')
def plan(volume):
    """【阶段 2】生成宏观卷大纲 或 指定卷的微观章节细纲(Beats)。
    
    说明：
    1. 不带 --volume 时，生成全书 10 卷的宏观框架（核心冲突、战力上限）。
    2. 带 --volume 时，为该卷生成 50 章的微观打点(Scene Beats)，并自动归一化每章字数为 5000 字。
    """
    if volume is None:
        plan_macro_outlines()
    else:
        run_volume_planner(volume)
    wait_for_background_tasks()

@cli.command()
@click.option('--volume', type=int, required=True, help='目标卷号。')
@click.option('--chapters', type=str, required=True, help='范围格式如 "1-5" 或单章 "1"。')
def write(volume, chapters):
    """【阶段 3】实时码字：启动场景子智能体集群进行创作。
    
    该模式使用标准 Chat API，支持实时流式输出和动态 RAG 记忆更新。
    每个场景生成前会自动检查 Checkpoint，跳过已生成的稿件。
    
    示例: uv run python cli.py write --volume 1 --chapters 1-10
    """
    if '-' in chapters:
        start, end = map(int, chapters.split('-'))
    else:
        start = end = int(chapters)
        
    run_scene_writer(volume, start, end)
    wait_for_background_tasks()

@cli.command()
@click.option('--volume', type=int, required=True, help='目标卷号。')
@click.option('--chapters', type=str, required=True, help='章节范围 (如 1-50)。')
def batch_build(volume, chapters):
    """【Batch API】第一步：构建批量请求 JSONL 文件。
    
    读取指定章节的 Beats 数据，注入 RAG 记忆，并打包成智谱 Batch API 所需的格式。
    生成的请求文件将保存在配置的 BATCH_DIR 目录中。
    """
    from scene_writer import generate_batch_jsonl
    from utils.config import BATCH_DIR
    
    if '-' in chapters:
        start, end = map(int, chapters.split('-'))
    else:
        start = end = int(chapters)
        
    output_path = os.path.join(BATCH_DIR, f"vol_{volume:02d}_ch_{start}_{end}_req.jsonl")
    generate_batch_jsonl(volume, start, end, output_path)

@cli.command()
@click.argument('jsonl_path')
def batch_submit(jsonl_path):
    """【Batch API】第二步：上传 JSONL 并提交异步任务。
    
    JSONL_PATH: 第一步生成的 .jsonl 文件路径。
    提交后会返回 Batch ID，请务必记录该 ID，因为智谱 API 目前由于异步特性，需要手动 ID 才能进行第三步同步。
    """
    from utils.batch_client import submit_batch_task
    import os
    
    if not os.path.exists(jsonl_path):
        print(f"[ERROR] 找不到文件: {jsonl_path}")
        return
        
    batch_id = submit_batch_task(jsonl_path, desc=f"Submit: {os.path.basename(jsonl_path)}")
    print(f"\n[✓] 任务提交成功！\nBatch ID: {batch_id}\n请妥善保存此 ID，后续同步需使用。")

@cli.command()
@click.argument('batch_id')
def batch_sync(batch_id):
    """【Batch API】第三步：轮询状态并同步合并成稿。
    
    BATCH_ID: 第二步返回的任务 ID。
    
    特点：
    1. 自动重试与轮询（每分钟一次）。
    2. 任务完成后自动下载结果文件。
    3. 自动调用 Editor Agent 对场景片段进行平滑合并。
    4. 自动更新 RAG 向量记忆库。
    """
    from utils.batch_client import get_batch_status, download_batch_results
    from scene_writer import process_batch_results
    from utils.config import BATCH_DIR
    import time
    
    print(f"[Batch] 正在监听任务 {batch_id} ...")
    while True:
        status = get_batch_status(batch_id)
        current_status = status.status
        print(f"  > 状态: {current_status}")
        
        if current_status == "completed":
            res_path = os.path.join(BATCH_DIR, f"{batch_id}_results.jsonl")
            err_path = os.path.join(BATCH_DIR, f"{batch_id}_errors.jsonl")
            if download_batch_results(batch_id, res_path, err_path):
                process_batch_results(res_path)
                wait_for_background_tasks()
                print("\n[✓] 批量同步任务全部完成！")
            return
        elif current_status in ["failed", "cancelled", "expired"]:
            print(f"[ERROR] 任务状态异常: {current_status}")
            return
            
        time.sleep(60)

@cli.command()
@click.option('--volume', type=int, required=True, help='目标卷号。')
@click.option('--chapters', type=str, required=True, help='范围格式如 "1-5"。')
def reindex(volume, chapters):
    """【工具】补救措施：手动将已生成的成稿重新导入 RAG 记忆库。
    
    如果由于网络报错等原因导致某些章节没有成功入库，可使用此命令通过 Aho-Corasick 自动机重新提取实体并入库。
    """
    from scene_writer import run_scene_writer
    from core.event_bus import event_bus
    from utils.config import MANUSCRIPTS_DIR
    import os
    
    if '-' in chapters:
        start, end = map(int, chapters.split('-'))
    else:
        start = end = int(chapters)
        
    for chap in range(start, end + 1):
        path = os.path.join(MANUSCRIPTS_DIR, f"vol_{volume:02d}", f"ch_{chap:03d}_final.md")
        if os.path.exists(path):
            with open(path, "r", encoding="utf-8") as f:
                content = f.read()
                print(f"[REINDEX] 正在补全第 {chap} 章的记忆...")
                beat_mock = {"chapter_id": chap, "beats": []}
                event_bus.emit("on_after_scene_write", beat_mock, content)
        else:
            print(f"[WARN] 找不到成稿文件: {path}")
            
    wait_for_background_tasks()

@cli.command()
@click.option('-f', '--files', multiple=True, required=True, help='需要修改的目标文件，支持多次使用 -f 指定多个文件。')
@click.option('-i', '--instruction', required=True, help='修改意见与指令。')
def review(files, instruction):
    """【工具】多文件 AI 辅助审阅并自动修改文件内容。

    示例: uv run python cli.py review -f "factions.json" -f "power_levels.json" -i "将金丹期统一改为结丹期"
    """
    from cli.commands.agent_commands import perform_multi_file_review_impl
    from utils.config import wait_for_background_tasks
    import sys

    success = perform_multi_file_review_impl(list(files), instruction)
    wait_for_background_tasks()
    if not success:
        sys.exit(1)

@cli.group()
def skills():
    """【V3 插件管理】查看、重载、或自动生成 Skill 插件。"""
    pass

@skills.command("list")
def skills_list():
    """列出所有已加载的 V3 插件和 skills/ 目录下可被发现的插件文件夹。"""
    from core.novel_context import NovelContext
    from core.plugin_manager import PluginManager
    from utils.workspace import WorkspaceManager
    from utils.config import NOVEL_DIR

    workspace = WorkspaceManager(NOVEL_DIR)
    context = NovelContext(workspace)
    mgr = PluginManager(context)
    mgr.scan_and_load()

    if context.active_skills:
        click.echo(click.style(f"\n🔌 已加载 {len(context.active_skills)} 个动态插件:", fg="green", bold=True))
        for name, skill in context.active_skills.items():
            click.echo(f"  🟢 {skill.name}  (skills/{name}/skill.py)")
    else:
        click.echo(click.style("\n🔌 (暂无已加载的动态插件)", fg="yellow"))

    # 列出 skills/ 中的其他文件夹
    skills_dir = "skills"
    if os.path.exists(skills_dir):
        all_dirs = [d for d in os.listdir(skills_dir) if os.path.isdir(os.path.join(skills_dir, d)) and not d.startswith("__") and not d.startswith(".")]
        unloaded = [d for d in all_dirs if d not in context.active_skills]
        
        disabled = []
        errors = []
        
        for d in unloaded:
            if os.path.exists(os.path.join(skills_dir, d, ".disabled")):
                disabled.append(d)
            else:
                errors.append(d)
                
        if disabled:
            click.echo(click.style(f"\n⏸️ 以下插件已被禁用:", fg="blue"))
            for d in disabled:
                click.echo(f"  🔴 skills/{d}/")
                
        if errors:
            click.echo(click.style(f"\n⚠️ 以下目录未成功加载 (可能缺少 skill.py 或有报错):", fg="yellow"))
            for d in errors:
                click.echo(f"  ⚪ skills/{d}/")

@skills.command("enable")
@click.argument("name")
def skills_enable(name):
    """启用指定的插件。

    NAME: skills/ 目录下的文件夹名 (如 ext_gold_finger)。
    """
    from core.novel_context import NovelContext
    from core.plugin_manager import PluginManager
    from utils.workspace import WorkspaceManager
    from utils.config import NOVEL_DIR

    workspace = WorkspaceManager(NOVEL_DIR)
    context = NovelContext(workspace)
    mgr = PluginManager(context)
    
    click.echo(f"[INFO] 正在启用插件: {name}...")
    mgr.enable_skill(name)
    click.echo(click.style(f"[✓] 插件 {name} 已启用。请重载/重启依赖项以生效。", fg="green"))

@skills.command("disable")
@click.argument("name")
def skills_disable(name):
    """禁用指定的插件。

    NAME: skills/ 目录下的文件夹名 (如 ext_gold_finger)。
    """
    from core.novel_context import NovelContext
    from core.plugin_manager import PluginManager
    from utils.workspace import WorkspaceManager
    from utils.config import NOVEL_DIR

    workspace = WorkspaceManager(NOVEL_DIR)
    context = NovelContext(workspace)
    mgr = PluginManager(context)
    
    click.echo(f"[INFO] 正在禁用插件: {name}...")
    mgr.disable_skill(name)
    click.echo(click.style(f"[✓] 插件 {name} 已被禁用。请重载/重启依赖项以彻底卸载。", fg="yellow"))

@skills.command("reload")
@click.argument("name", required=False, default=None)
def skills_reload(name):
    """热重载插件。不指定 NAME 则重载全部，指定 NAME 则只重载单个。
    
    NAME: 可选，skills/ 目录下的文件夹名 (如 core_memory_rag)。
    """
    from core.novel_context import NovelContext
    from core.plugin_manager import PluginManager
    from utils.workspace import WorkspaceManager
    from utils.config import NOVEL_DIR

    workspace = WorkspaceManager(NOVEL_DIR)
    context = NovelContext(workspace)
    mgr = PluginManager(context)

    if name:
        click.echo(f"[INFO] 热重载插件: {name}")
        mgr.scan_and_load()  # 先全量加载
        mgr.hot_reload(name)
    else:
        click.echo("[INFO] 重载全部插件...")
        mgr.scan_and_load()

    click.echo(click.style(f"[✓] 当前共加载 {len(context.active_skills)} 个插件。", fg="green"))

@skills.command("build")
@click.argument("request")
def skills_build(request):
    """让 SkillBuilder Agent 根据自然语言需求自动生成插件代码。
    
    REQUEST: 用自然语言描述你想要的插件功能。
    
    示例: uv run python cli.py skills build "帮我写一个Skill，在每次生成前注入一句主角很帅"
    """
    from core.novel_context import NovelContext
    from core.plugin_manager import PluginManager
    from core.agents.skill_builder_agent import SkillBuilderAgent
    from utils.workspace import WorkspaceManager
    from utils.config import NOVEL_DIR

    workspace = WorkspaceManager(NOVEL_DIR)
    context = NovelContext(workspace)
    mgr = PluginManager(context)
    mgr.scan_and_load()

    agent = SkillBuilderAgent(context, mgr)
    success = agent.build_skill(request)
    if success:
        click.echo(click.style("[✓] 插件已生成并热重载成功！", fg="green", bold=True))
    else:
        click.echo(click.style("[✗] 插件生成失败，请查看上方日志。", fg="red"))

if __name__ == '__main__':
    try:
        cli()
    except KeyboardInterrupt:
        print("\n[WARN] 收到终端停止信号。正在尝试保存并退出后台任务...")
        wait_for_background_tasks()
    except Exception as e:
        import traceback
        traceback.print_exc()
        print(f"\n[ERROR] 执行失败: {e}")
        wait_for_background_tasks()
