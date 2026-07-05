import { useCallback, useEffect, useState } from "react";
import { useIntl } from "react-intl";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  IconFlask,
  IconPlus,
  IconPlayerPlay,
  IconCheck,
  IconTrash,
  IconUpload,
  IconLoader2,
} from "@tabler/icons-react";

/** 实验室草稿摘要 */
interface SkillLabSummary {
  id: string;
  name: string;
  status: string;
  iterationCount: number;
}

/** 实验室草稿完整内容 */
interface SkillLabDetail {
  id: string;
  name: string;
  content: string;
  testPrompt: string;
  status: string;
  iterationCount: number;
  lastTestResult: string | null;
  lastEvaluation: string | null;
}

type LabStatus = "idle" | "testing" | "evaluating" | "rewriting" | "passed" | "failed" | "promoted";

const STATUS_LABELS: Record<LabStatus, string> = {
  idle: "settings.skillLab.status.idle",
  testing: "settings.skillLab.status.testing",
  evaluating: "settings.skillLab.status.evaluating",
  rewriting: "settings.skillLab.status.rewriting",
  passed: "settings.skillLab.status.passed",
  failed: "settings.skillLab.status.failed",
  promoted: "settings.skillLab.promoted",
};

const STATUS_COLORS: Record<LabStatus, string> = {
  idle: "text-[var(--text-muted)]",
  testing: "text-yellow-500",
  evaluating: "text-blue-500",
  rewriting: "text-orange-500",
  passed: "text-green-500",
  failed: "text-red-500",
  promoted: "text-purple-500",
};

export function SkillLabPanel() {
  const intl = useIntl();
  const [skills, setSkills] = useState<SkillLabSummary[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [detail, setDetail] = useState<SkillLabDetail | null>(null);

  // 编辑状态
  const [name, setName] = useState("");
  const [content, setContent] = useState("");
  const [testPrompt, setTestPrompt] = useState("");
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [testing, setTesting] = useState(false);
  const [promoting, setPromoting] = useState(false);
  // 测试闭环实时进度
  const [testPhase, setTestPhase] = useState<string | null>(null);
  const [testIteration, setTestIteration] = useState(0);

  // 加载列表
  const loadList = useCallback(async () => {
    try {
      const list = await invoke<SkillLabSummary[]>("skill_lab_list");
      setSkills(list);
    } catch (err) {
      console.error("Failed to load skill lab list:", err);
    }
  }, []);

  useEffect(() => {
    loadList();
  }, [loadList]);

  // 监听测试进度事件
  useEffect(() => {
    let unlisten: UnlistenFn | null = null;
    listen<{
      skillId: string;
      phase: string;
      iteration?: number;
      status?: string;
    }>("skill-lab-progress", (e) => {
      if (e.payload.skillId !== selectedId) return;
      setTestPhase(e.payload.phase);
      if (e.payload.iteration) setTestIteration(e.payload.iteration);
      if (e.payload.phase === "done") {
        setTestPhase(null);
      }
    }).then((fn) => {
      unlisten = fn;
    });
    return () => {
      unlisten?.();
    };
  }, [selectedId]);

  // 选中一个草稿
  const handleSelect = useCallback(async (id: string) => {
    setSelectedId(id);
    try {
      const d = await invoke<SkillLabDetail>("skill_lab_read", { skillId: id });
      setDetail(d);
      setName(d.name);
      setContent(d.content);
      setTestPrompt(d.testPrompt);
    } catch (err) {
      console.error("Failed to load skill lab detail:", err);
    }
  }, []);

  // 创建新草稿
  const handleCreate = useCallback(() => {
    const id = `lab-${Date.now()}`;
    setSelectedId(id);
    setDetail(null);
    setName("");
    setContent("");
    setTestPrompt("");
    setSaved(false);
  }, []);

  // 保存草稿
  const handleSave = useCallback(async () => {
    if (!selectedId || !name.trim()) return;
    setSaving(true);
    try {
      await invoke("skill_lab_save", {
        params: {
          skillId: selectedId,
          name: name.trim(),
          content,
          testPrompt,
        },
      });
      setSaved(true);
      setTimeout(() => setSaved(false), 2000);
      await loadList();
    } catch (err) {
      console.error("Failed to save skill lab:", err);
    } finally {
      setSaving(false);
    }
  }, [selectedId, name, content, testPrompt, loadList]);

  // 运行完整的测试闭环：测试 → 评估 → 改写 → 重复
  const handleRunTest = useCallback(async () => {
    if (!selectedId) return;
    setTesting(true);

    try {
      // 先保存当前内容
      await invoke("skill_lab_save", {
        params: {
          skillId: selectedId,
          name: name.trim(),
          content,
          testPrompt,
        },
      });

      await loadList();

      // 调用后端自动测试闭环
      const result = await invoke<{
        status: string;
        iterations: number;
        lastOutput: string;
        evaluation: string;
        finalContent: string;
      }>("skill_lab_run_test", { skillId: selectedId });

      // 用最终结果刷新 UI
      const refreshed = await invoke<SkillLabDetail>("skill_lab_read", {
        skillId: selectedId,
      });
      setDetail(refreshed);
      setContent(result.finalContent);
      await loadList();
    } catch (err) {
      console.error("Skill lab test failed:", err);
    } finally {
      setTesting(false);
    }
  }, [selectedId, name, content, testPrompt, loadList]);

  // 推广为正式 Skill
  const handlePromote = useCallback(async () => {
    if (!selectedId) return;
    setPromoting(true);
    try {
      await invoke("skill_lab_promote", { skillId: selectedId });
      await loadList();
      if (selectedId) {
        await handleSelect(selectedId);
      }
    } catch (err) {
      console.error("Failed to promote skill:", err);
    } finally {
      setPromoting(false);
    }
  }, [selectedId, loadList, handleSelect]);

  // 删除草稿
  const handleDelete = useCallback(async (id: string) => {
    try {
      await invoke("skill_lab_delete", { skillId: id });
      if (selectedId === id) {
        setSelectedId(null);
        setDetail(null);
      }
      await loadList();
    } catch (err) {
      console.error("Failed to delete skill lab entry:", err);
    }
  }, [selectedId, loadList]);

  const currentStatus = (detail?.status ?? "idle") as LabStatus;
  const statusLabel = STATUS_LABELS[currentStatus] ?? STATUS_LABELS.idle;
  const statusColor = STATUS_COLORS[currentStatus] ?? STATUS_COLORS.idle;

  return (
    <div className="space-y-5">
      {/* 标题 + 创建按钮 */}
      <section className="settings-card space-y-3">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2">
            <IconFlask size={18} stroke={1.5} />
            <h4 className="text-[13px] font-semibold text-[var(--text-strong)]">
              {intl.formatMessage({ id: "settings.skillLab" })}
            </h4>
          </div>
          <button
            onClick={handleCreate}
            className="flex items-center gap-1 rounded-lg px-3 py-1.5 text-xs font-medium
                       bg-[var(--accent)] text-white hover:opacity-80 transition-opacity"
          >
            <IconPlus size={14} stroke={2} />
            {intl.formatMessage({ id: "settings.skillLab.create" })}
          </button>
        </div>

        <p className="text-xs text-[var(--text-muted)]">
          {intl.formatMessage({ id: "settings.skillLab.description" })}
        </p>
      </section>

      <div className="flex gap-4">
        {/* 左侧列表 */}
        <div className="w-48 shrink-0 space-y-1">
          {skills.length === 0 && !selectedId && (
            <p className="text-xs text-[var(--text-faint)] px-2 py-4">
              {intl.formatMessage({ id: "settings.skillLab.empty" })}
            </p>
          )}
          {skills.map((s) => (
            <button
              key={s.id}
              onClick={() => handleSelect(s.id)}
              className={`w-full text-left rounded-lg px-3 py-2 text-xs transition-colors
                ${selectedId === s.id
                  ? "bg-[var(--accent-muted)] text-[var(--text-strong)]"
                  : "hover:bg-[var(--surface-hover)] text-[var(--text-base)]"
                }`}
            >
              <div className="font-medium truncate">{s.name || s.id}</div>
              <div className={`text-[10px] mt-0.5 ${STATUS_COLORS[(s.status as LabStatus) || "idle"]}`}>
                {intl.formatMessage({ id: STATUS_LABELS[(s.status as LabStatus) || "idle"] })}
                {s.iterationCount > 0 && ` · ${intl.formatMessage({ id: "settings.skillLab.iterationCount" }, { count: s.iterationCount })}`}
              </div>
            </button>
          ))}
        </div>

        {/* 右侧编辑区域 */}
        {selectedId && (
          <div className="flex-1 space-y-4 min-w-0">
            {/* 名称 */}
            <div>
              <label className="block text-xs font-medium text-[var(--text-base)] mb-1">
                {intl.formatMessage({ id: "settings.skillLab.name" })}
              </label>
              <input
                type="text"
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder={intl.formatMessage({ id: "settings.skillLab.namePlaceholder" })}
                className="w-full rounded-lg border border-[var(--border-subtle)] bg-[var(--surface-main)]
                           px-3 py-2 text-xs text-[var(--text-base)]
                           placeholder:text-[var(--text-faint)]
                           focus:border-[var(--accent-strong)] focus:outline-none"
              />
            </div>

            {/* Skill 内容 */}
            <div>
              <label className="block text-xs font-medium text-[var(--text-base)] mb-1">
                {intl.formatMessage({ id: "settings.skillLab.content" })}
              </label>
              <textarea
                value={content}
                onChange={(e) => setContent(e.target.value)}
                placeholder={intl.formatMessage({ id: "settings.skillLab.contentPlaceholder" })}
                className="w-full min-h-[200px] rounded-lg border border-[var(--border-subtle)] bg-[var(--surface-main)]
                           p-3 font-mono text-xs text-[var(--text-base)]
                           placeholder:text-[var(--text-faint)]
                           focus:border-[var(--accent-strong)] focus:outline-none resize-y"
              />
            </div>

            {/* 测试提示词 */}
            <div>
              <label className="block text-xs font-medium text-[var(--text-base)] mb-1">
                {intl.formatMessage({ id: "settings.skillLab.testPrompt" })}
              </label>
              <textarea
                value={testPrompt}
                onChange={(e) => setTestPrompt(e.target.value)}
                placeholder={intl.formatMessage({ id: "settings.skillLab.testPromptPlaceholder" })}
                className="w-full min-h-[80px] rounded-lg border border-[var(--border-subtle)] bg-[var(--surface-main)]
                           p-3 text-xs text-[var(--text-base)]
                           placeholder:text-[var(--text-faint)]
                           focus:border-[var(--accent-strong)] focus:outline-none resize-y"
              />
            </div>

            {/* 状态和迭代次数 */}
            {detail && (
              <div className="flex items-center gap-4 text-xs">
                <span className={statusColor}>
                  {intl.formatMessage({ id: statusLabel })}
                </span>
                {detail.iterationCount > 0 && (
                  <span className="text-[var(--text-muted)]">
                    {intl.formatMessage({ id: "settings.skillLab.iterationCount" }, { count: detail.iterationCount })}
                  </span>
                )}
              </div>
            )}

            {/* 测试结果区域 */}
            {detail?.lastTestResult && (
              <div className="rounded-lg border border-[var(--border-subtle)] bg-[var(--surface-main)] p-3">
                <h5 className="text-xs font-medium text-[var(--text-base)] mb-1">
                  {intl.formatMessage({ id: "settings.skillLab.testResult" })}
                </h5>
                <pre className="text-[11px] text-[var(--text-muted)] whitespace-pre-wrap max-h-[200px] overflow-y-auto">
                  {detail.lastTestResult}
                </pre>
              </div>
            )}

            {/* AI 评估意见 */}
            {detail?.lastEvaluation && (
              <div className="rounded-lg border border-[var(--border-subtle)] bg-[var(--surface-main)] p-3">
                <h5 className="text-xs font-medium text-[var(--text-base)] mb-1">
                  AI 评估
                </h5>
                <pre className="text-[11px] text-[var(--text-muted)] whitespace-pre-wrap max-h-[200px] overflow-y-auto">
                  {detail.lastEvaluation}
                </pre>
              </div>
            )}

            {/* 操作按钮 */}
            <div className="flex items-center gap-2 flex-wrap">
              <button
                onClick={handleSave}
                disabled={saving || !name.trim()}
                className="flex items-center gap-1 rounded-lg px-3 py-1.5 text-xs font-medium
                           border border-[var(--border-subtle)] text-[var(--text-base)]
                           hover:bg-[var(--surface-hover)] transition-colors disabled:opacity-50"
              >
                {saved ? <IconCheck size={14} stroke={2} /> : null}
                {intl.formatMessage({ id: saved ? "settings.skillLab.saved" : "settings.skillLab.save" })}
              </button>

              <button
                onClick={handleRunTest}
                disabled={testing || !content.trim() || !testPrompt.trim()}
                className="flex items-center gap-1 rounded-lg px-3 py-1.5 text-xs font-medium
                           bg-[var(--accent)] text-white hover:opacity-80 transition-opacity
                           disabled:opacity-50"
              >
                {testing ? (
                  <IconLoader2 size={14} stroke={2} className="animate-spin" />
                ) : (
                  <IconPlayerPlay size={14} stroke={2} />
                )}
                {testing && testPhase
                  ? `${intl.formatMessage({ id: `settings.skillLab.status.${testPhase}` as keyof typeof STATUS_LABELS })} #${testIteration}`
                  : intl.formatMessage({ id: testing ? "settings.skillLab.testing" : "settings.skillLab.runTest" })}
              </button>

              {currentStatus === "passed" && (
                <button
                  onClick={handlePromote}
                  disabled={promoting}
                  className="flex items-center gap-1 rounded-lg px-3 py-1.5 text-xs font-medium
                             bg-green-600 text-white hover:opacity-80 transition-opacity
                             disabled:opacity-50"
                >
                  <IconUpload size={14} stroke={2} />
                  {intl.formatMessage({ id: promoting ? "settings.skillLab.promoted" : "settings.skillLab.promote" })}
                </button>
              )}

              <button
                onClick={() => handleDelete(selectedId)}
                className="flex items-center gap-1 rounded-lg px-3 py-1.5 text-xs font-medium
                           text-red-500 hover:bg-red-50 dark:hover:bg-red-900/20 transition-colors"
              >
                <IconTrash size={14} stroke={2} />
              </button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
