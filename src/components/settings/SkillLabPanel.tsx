import { useCallback, useEffect, useMemo, useRef, useState } from "react";
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
  IconSparkles,
  IconX,
} from "@tabler/icons-react";
import { SettingsPagination, usePagedItems } from "./SettingsPagination";

/** 实验室草稿摘要 */
interface SkillLabSummary {
  id: string;
  name: string;
  status: string;
  iterationCount: number;
}

/** 实验室草稿完整内容 */
interface SkillLabScoreRecord {
  iteration: number;
  totalScore: number;
  clarity: number;
  robustness: number;
  executability: number;
  maintainability: number;
}

interface SkillLabDetail {
  id: string;
  name: string;
  goal: string;
  content: string;
  testPrompt: string;
  status: string;
  iterationCount: number;
  maxIterations?: number;
  lastTestResult: string | null;
  lastEvaluation: string | null;
  bestScore: number | null;
  scoreHistory: SkillLabScoreRecord[];
  bestEvaluation: string | null;
  stableRounds: number;
}

interface PythonEnvCheckResult {
  available: boolean;
  executable: string | null;
  version: string | null;
  installHint: string;
}

interface SkillLabGeneratedScript {
  path: string;
  content: string;
}

interface SkillLabGenerateResult {
  name: string;
  content: string;
  testPrompt: string;
  scripts: SkillLabGeneratedScript[];
}

/** 运行测试闭环的返回结果 */
interface SkillLabTestResult {
  status: string;
  iterations: number;
  lastOutput: string;
  evaluation: string;
  finalContent: string;
  bestScore: number;
  scoreHistory: SkillLabScoreRecord[];
  stableRounds: number;
}

interface SkillLabProgressEvent {
  skillId: string;
  phase: string;
  iteration?: number;
  maxIterations?: number;
  status?: string;
  bestScore?: number;
  score?: number;
  logType?: string;
  logSnippet?: string;
  retryAttempt?: number;
  retryMax?: number;
  retryDelayMs?: number;
  retryReason?: string;
}

interface ActiveSkillRun {
  skillId: string;
  phase: string;
  iteration: number;
  maxIterations: number;
  score: number | null;
  retryAttempt: number | null;
  retryMax: number | null;
  retryDelayMs: number | null;
  logs: string[];
  updatedAt: number;
}

type LabStatus =
  | "idle"
  | "testing"
  | "evaluating"
  | "rewriting"
  | "passed"
  | "failed"
  | "deployed"
  | "promoted";

const STATUS_LABELS: Record<LabStatus, string> = {
  idle: "settings.skillLab.status.idle",
  testing: "settings.skillLab.status.testing",
  evaluating: "settings.skillLab.status.evaluating",
  rewriting: "settings.skillLab.status.rewriting",
  passed: "settings.skillLab.status.passed",
  failed: "settings.skillLab.status.failed",
  deployed: "settings.skillLab.deployed",
  promoted: "settings.skillLab.deployed",
};

const STATUS_COLORS: Record<LabStatus, string> = {
  idle: "text-[var(--text-muted)]",
  testing: "text-yellow-500",
  evaluating: "text-blue-500",
  rewriting: "text-orange-500",
  passed: "text-green-500",
  failed: "text-red-500",
  deployed: "text-purple-500",
  promoted: "text-purple-500",
};

const FALLBACK_MAX_ITERATIONS = 3;
const MIN_MAX_ITERATIONS = 1;
const MAX_MAX_ITERATIONS = 20;
const ACTIVE_RUN_LOG_LIMIT = 30;
const SCORE_HISTORY_PAGE_SIZE = 10;

function normalizeMaxIterations(value: number | null | undefined): number {
  if (value == null || Number.isNaN(value) || value <= 0) {
    return FALLBACK_MAX_ITERATIONS;
  }
  return Math.min(MAX_MAX_ITERATIONS, Math.max(MIN_MAX_ITERATIONS, Math.floor(value)));
}

function appendRunLog(logs: string[], snippet: string | null | undefined): string[] {
  const text = snippet?.trim();
  if (!text) {
    return logs;
  }
  if (logs.length > 0 && logs[logs.length - 1] === text) {
    return logs;
  }
  return [...logs, text].slice(-ACTIVE_RUN_LOG_LIMIT);
}

function statusFromRaw(status: string | null | undefined): LabStatus {
  if (!status) {
    return "idle";
  }
  if (status in STATUS_LABELS) {
    return status as LabStatus;
  }
  return "idle";
}

function deployState(status: string | null | undefined): {
  isDeployed: boolean;
  canDeploy: boolean;
  disabledReasonId: string | null;
} {
  const normalized = statusFromRaw(status);
  if (normalized === "deployed" || normalized === "promoted") {
    return {
      isDeployed: true,
      canDeploy: false,
      disabledReasonId: "settings.skillLab.deployDisabledAlready",
    };
  }
  if (normalized === "passed") {
    return { isDeployed: false, canDeploy: true, disabledReasonId: null };
  }
  return {
    isDeployed: false,
    canDeploy: false,
    disabledReasonId: "settings.skillLab.deployDisabledNeedPass",
  };
}

function phaseLabelId(phase: string | null): string {
  if (phase === "testing") {
    return "settings.skillLab.status.testing";
  }
  if (phase === "evaluating") {
    return "settings.skillLab.status.evaluating";
  }
  if (phase === "rewriting") {
    return "settings.skillLab.status.rewriting";
  }
  return "settings.skillLab.testing";
}

function isLiveRunStatus(status: string | null | undefined): boolean {
  const normalized = statusFromRaw(status);
  return normalized === "testing" || normalized === "evaluating" || normalized === "rewriting";
}

export function SkillLabPanel() {
  const intl = useIntl();
  const [skills, setSkills] = useState<SkillLabSummary[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [detail, setDetail] = useState<SkillLabDetail | null>(null);
  const [editorOpen, setEditorOpen] = useState(false);
  const [resultDialogOpen, setResultDialogOpen] = useState(false);

  // 编辑状态
  const [name, setName] = useState("");
  const [goal, setGoal] = useState("");
  const [content, setContent] = useState("");
  const [testPrompt, setTestPrompt] = useState("");
  const [maxIterations, setMaxIterations] = useState(FALLBACK_MAX_ITERATIONS);
  const [saving, setSaving] = useState(false);
  const [generating, setGenerating] = useState(false);
  const [checkingPython, setCheckingPython] = useState(false);
  const [saved, setSaved] = useState(false);
  const [testing, setTesting] = useState(false);
  const [testError, setTestError] = useState<string | null>(null);
  const [testResult, setTestResult] = useState<SkillLabTestResult | null>(null);
  const [deployingId, setDeployingId] = useState<string | null>(null);
  const [deployFeedback, setDeployFeedback] = useState<{
    kind: "success" | "error";
    text: string;
  } | null>(null);
  const [pythonInfo, setPythonInfo] = useState("");
  const [generationError, setGenerationError] = useState("");
  // 测试闭环实时进度
  const [testPhase, setTestPhase] = useState<string | null>(null);
  const [testIteration, setTestIteration] = useState(0);
  const [activeRunsBySkillId, setActiveRunsBySkillId] = useState<
    Record<string, ActiveSkillRun>
  >({});
  const [progressDockHidden, setProgressDockHidden] = useState(false);
  const [scoreHistoryPage, setScoreHistoryPage] = useState(0);
  // 自动保存开关：开启后编辑内容会自动防抖落盘
  const [autoSave, setAutoSave] = useState(true);
  // 上次已保存字段快照，用于判断是否存在未保存改动
  const [savedSnapshot, setSavedSnapshot] = useState<{
    name: string;
    goal: string;
    content: string;
    testPrompt: string;
    maxIterations: number;
  } | null>(null);

  // 用于异步操作期间校验当前选中草稿是否仍为最初触发的那个，
  // 避免线程仍在进行时切换草稿导致结果被写入错误目标。
  const selectedIdRef = useRef<string | null>(selectedId);
  useEffect(() => {
    selectedIdRef.current = selectedId;
  }, [selectedId]);

  // 加载列表
  const loadList = useCallback(async () => {
    try {
      const list = await invoke<SkillLabSummary[]>("skill_lab_list");
      setSkills(list);
      setActiveRunsBySkillId((previous) => {
        const next: Record<string, ActiveSkillRun> = {};
        list.forEach((skill) => {
          if (!isLiveRunStatus(skill.status)) {
            return;
          }
          const existing = previous[skill.id];
          next[skill.id] = {
            skillId: skill.id,
            phase: statusFromRaw(skill.status),
            iteration: existing?.iteration ?? 0,
            maxIterations: existing?.maxIterations ?? FALLBACK_MAX_ITERATIONS,
            score: existing?.score ?? null,
            retryAttempt: existing?.retryAttempt ?? null,
            retryMax: existing?.retryMax ?? null,
            retryDelayMs: existing?.retryDelayMs ?? null,
            logs: existing?.logs ?? [],
            updatedAt: existing?.updatedAt ?? Date.now(),
          };
        });
        return next;
      });
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
    listen<SkillLabProgressEvent>("skill-lab-progress", (event) => {
      const payload = event.payload;
      const skillId = payload.skillId;
      if (!skillId) {
        return;
      }
      const phase = payload.phase || "testing";
      const iteration = payload.iteration ?? 0;
      const maxIterations = payload.maxIterations ?? FALLBACK_MAX_ITERATIONS;
      setActiveRunsBySkillId((previous) => {
        if (phase === "done") {
          if (!previous[skillId]) {
            return previous;
          }
          const next = { ...previous };
          delete next[skillId];
          return next;
        }
        const current = previous[skillId];
        const retrying = payload.logType === "retry";
        const nextRun: ActiveSkillRun = {
          skillId,
          phase,
          iteration: iteration || current?.iteration || 0,
          maxIterations: maxIterations || current?.maxIterations || FALLBACK_MAX_ITERATIONS,
          score: payload.score ?? current?.score ?? null,
          retryAttempt: retrying ? payload.retryAttempt ?? current?.retryAttempt ?? null : null,
          retryMax: retrying ? payload.retryMax ?? current?.retryMax ?? null : null,
          retryDelayMs: retrying ? payload.retryDelayMs ?? current?.retryDelayMs ?? null : null,
          logs: appendRunLog(current?.logs ?? [], payload.logSnippet),
          updatedAt: Date.now(),
        };
        return { ...previous, [skillId]: nextRun };
      });
      if (skillId === selectedId) {
        if (phase === "done") {
          setTestPhase(null);
          setTestIteration(0);
          setTesting(false);
        } else {
          setTesting(true);
          setTestPhase(phase);
          if (iteration > 0) {
            setTestIteration(iteration);
          }
        }
      }
      if (phase === "done") {
        void loadList();
      }
    }).then((fn) => {
      unlisten = fn;
    });
    return () => {
      unlisten?.();
    };
  }, [selectedId, loadList]);

  // 保存当前草稿（单一落盘入口，供手动保存与自动保存复用）
  const persistCurrent = useCallback(
    async (showSaved: boolean): Promise<boolean> => {
      if (!selectedId || !name.trim()) return false;
      setSaving(true);
      try {
        await invoke("skill_lab_save", {
          params: {
            skillId: selectedId,
            name: name.trim(),
            goal,
            content,
            testPrompt,
            maxIterations: normalizeMaxIterations(maxIterations),
          },
        });
        setSavedSnapshot({
          name: name.trim(),
          goal,
          content,
          testPrompt,
          maxIterations: normalizeMaxIterations(maxIterations),
        });
        if (showSaved) {
          setSaved(true);
          setTimeout(() => setSaved(false), 2000);
        }
        await loadList();
        return true;
      } catch (err) {
        console.error("Failed to save skill lab:", err);
        return false;
      } finally {
        setSaving(false);
      }
    },
    [selectedId, name, goal, content, testPrompt, maxIterations, loadList],
  );

  // 是否存在未保存改动：用于脏标记与切换前自动保存
  const dirty = useMemo(() => {
    if (!selectedId) return false;
    if (!savedSnapshot) {
      return (
        name.trim() !== "" ||
        goal.trim() !== "" ||
        content !== "" ||
        testPrompt !== "" ||
        normalizeMaxIterations(maxIterations) !== FALLBACK_MAX_ITERATIONS
      );
    }
    return (
      savedSnapshot.name !== name.trim() ||
      savedSnapshot.goal !== goal.trim() ||
      savedSnapshot.content !== content ||
      savedSnapshot.testPrompt !== testPrompt ||
      savedSnapshot.maxIterations !== normalizeMaxIterations(maxIterations)
    );
  }, [selectedId, savedSnapshot, name, goal, content, testPrompt, maxIterations]);

  // 选中一个草稿
  const handleSelect = useCallback(async (id: string) => {
    if (id === selectedId) {
      setEditorOpen(true);
      return;
    }
    // 切换前自动保存当前草稿，避免录入内容在切换目录后丢失
    if (dirty && selectedId && name.trim()) {
      await persistCurrent(true);
    }
    // 切换草稿时重置进行中的旋转态，避免按钮停留在错误的进行中状态
    const nextRun = activeRunsBySkillId[id];
    setGenerating(false);
    setCheckingPython(false);
    setSaving(false);
    setTesting(Boolean(nextRun));
    setTestPhase(nextRun?.phase ?? null);
    setTestIteration(nextRun?.iteration ?? 0);
    setSaved(false);
    setTestError(null);
    setTestResult(null);
    setDeployFeedback(null);
    setResultDialogOpen(false);
    setSelectedId(id);
    setEditorOpen(true);
    try {
      const d = await invoke<SkillLabDetail>("skill_lab_read", { skillId: id });
      setDetail(d);
      setName(d.name);
      setGoal(d.goal ?? "");
      setContent(d.content);
      setTestPrompt(d.testPrompt);
      const nextMaxIterations = normalizeMaxIterations(d.maxIterations);
      setMaxIterations(nextMaxIterations);
      setSavedSnapshot({
        name: d.name,
        goal: d.goal ?? "",
        content: d.content,
        testPrompt: d.testPrompt,
        maxIterations: nextMaxIterations,
      });
      setGenerationError("");
      setPythonInfo("");
    } catch (err) {
      console.error("Failed to load skill lab detail:", err);
    }
  }, [selectedId, dirty, name, persistCurrent, activeRunsBySkillId]);

  // 创建新草稿
  const handleCreate = useCallback(() => {
    const id = `lab-${Date.now()}`;
    setSelectedId(id);
    setDetail(null);
    setEditorOpen(true);
    setResultDialogOpen(false);
    setName("");
    setGoal("");
    setContent("");
    setTestPrompt("");
    setMaxIterations(FALLBACK_MAX_ITERATIONS);
    setGenerationError("");
    setPythonInfo("");
    setSaved(false);
    setSavedSnapshot(null);
    setGenerating(false);
    setCheckingPython(false);
    setSaving(false);
    setTesting(false);
    setTestPhase(null);
    setTestIteration(0);
    setTestError(null);
    setTestResult(null);
    setDeployingId(null);
    setDeployFeedback(null);
  }, []);

  // 手动保存草稿
  const handleSave = useCallback(() => {
    void persistCurrent(true);
  }, [persistCurrent]);

  const handleGenerate = useCallback(async () => {
    if (!selectedId || !goal.trim()) return;
    const targetId = selectedId;
    setGenerating(true);
    setCheckingPython(true);
    setDeployFeedback(null);
    setGenerationError("");
    setPythonInfo("");
    try {
      const env = await invoke<PythonEnvCheckResult>("skill_lab_check_python_env");
      setCheckingPython(false);
      if (!env.available) {
        setGenerationError(
          env.installHint ||
            intl.formatMessage({ id: "settings.skillLab.pythonMissingHint" }),
        );
        return;
      }

      const pythonText = [env.executable, env.version].filter(Boolean).join(" · ");
      setPythonInfo(
        pythonText ||
          intl.formatMessage({ id: "settings.skillLab.pythonReadyFallback" }),
      );

      const generated = await invoke<SkillLabGenerateResult>(
        "skill_lab_generate_from_goal",
        {
          params: {
            skillId: targetId,
            goal: goal.trim(),
            nameHint: name.trim() || undefined,
          },
        },
      );

      const nextName = generated.name?.trim() || name.trim() || targetId;
      const nextContent = generated.content || "";
      const nextTestPrompt = generated.testPrompt || "";

      // 无论如何先落盘，避免生成结果丢失
      await invoke("skill_lab_save", {
        params: {
          skillId: targetId,
          name: nextName,
          content: nextContent,
          testPrompt: nextTestPrompt,
          maxIterations: normalizeMaxIterations(maxIterations),
        },
      });

      // 若生成期间已切换到其它草稿，则不覆盖当前编辑区，但仍保留后端内容
      if (targetId !== selectedIdRef.current) return;

      setName(nextName);
      setContent(nextContent);
      setTestPrompt(nextTestPrompt);
      setSavedSnapshot({
        name: nextName,
        goal: goal.trim(),
        content: nextContent,
        testPrompt: nextTestPrompt,
        maxIterations: normalizeMaxIterations(maxIterations),
      });

      await loadList();
      const refreshed = await invoke<SkillLabDetail>("skill_lab_read", {
        skillId: targetId,
      });
      if (targetId !== selectedIdRef.current) return;
      setDetail(refreshed);
    } catch (err) {
      const errorText =
        `${intl.formatMessage({ id: "settings.skillLab.generateFailed" })}: ${String(err)}`;
      setGenerationError(errorText);
      console.error("Failed to auto-generate skill lab draft:", err);
    } finally {
      setCheckingPython(false);
      setGenerating(false);
    }
  }, [selectedId, goal, intl, name, maxIterations, loadList]);

  // 运行完整的测试闭环：测试 → 评估 → 改写 → 重复
  const handleRunTest = useCallback(async () => {
    if (!selectedId) return;
    const targetId = selectedId;
    setTesting(true);
    setTestError(null);
    setTestResult(null);
    setProgressDockHidden(false);
    setActiveRunsBySkillId((previous) => ({
      ...previous,
      [targetId]: {
        skillId: targetId,
        phase: "testing",
        iteration: 0,
        maxIterations: normalizeMaxIterations(maxIterations),
        score: null,
        retryAttempt: null,
        retryMax: null,
        retryDelayMs: null,
        logs: [],
        updatedAt: Date.now(),
      },
    }));

    try {
      // 先保存当前内容
      await invoke("skill_lab_save", {
        params: {
          skillId: targetId,
          name: name.trim(),
          goal,
          content,
          testPrompt,
          maxIterations: normalizeMaxIterations(maxIterations),
        },
      });
      setSavedSnapshot({
        name: name.trim(),
        goal,
        content,
        testPrompt,
        maxIterations: normalizeMaxIterations(maxIterations),
      });

      await loadList();

      // 调用后端自动测试闭环
      const result = await invoke<SkillLabTestResult>("skill_lab_run_test", {
        skillId: targetId,
      });
      setTestResult(result);

      // 用最终结果刷新 UI（若已切换草稿则不覆盖）
      const refreshed = await invoke<SkillLabDetail>("skill_lab_read", {
        skillId: targetId,
      });
      if (targetId !== selectedIdRef.current) return;
      setDetail(refreshed);
      setContent(result.finalContent);
      await loadList();
      setResultDialogOpen(true);
    } catch (err) {
      setActiveRunsBySkillId((previous) => {
        if (!previous[targetId]) {
          return previous;
        }
        const next = { ...previous };
        delete next[targetId];
        return next;
      });
      setTestPhase(null);
      setTestIteration(0);
      setTestError(
        `${intl.formatMessage({ id: "settings.skillLab.testError" })}: ${String(err)}`,
      );
      setResultDialogOpen(true);
      console.error("Skill lab test failed:", err);
    } finally {
      setTesting(false);
    }
  }, [selectedId, name, goal, content, testPrompt, maxIterations, loadList, intl]);

  // 部署为正式 Skill（写入 codey/skills/<id>/，之后 /skill 可用）
  const handleDeploy = useCallback(
    async (id?: string) => {
      const targetId = id ?? selectedId;
      if (!targetId) return;
      const targetStatus = id
        ? skills.find((item) => item.id === targetId)?.status
        : detail?.status;
      if (activeRunsBySkillId[targetId]) {
        setDeployFeedback({
          kind: "error",
          text: intl.formatMessage({ id: "settings.skillLab.deployDisabledRunning" }),
        });
        return;
      }
      const targetDeployState = deployState(targetStatus);
      if (!targetDeployState.canDeploy) {
        if (targetDeployState.disabledReasonId) {
          setDeployFeedback({
            kind: "error",
            text: intl.formatMessage({ id: targetDeployState.disabledReasonId }),
          });
        }
        return;
      }
      setDeployFeedback(null);
      setDeployingId(targetId);
      try {
        await invoke("skill_lab_deploy", { skillId: targetId });
        await loadList();
        if (targetId === selectedId) {
          const refreshed = await invoke<SkillLabDetail>("skill_lab_read", {
            skillId: targetId,
          });
          setDetail(refreshed);
          setName(refreshed.name);
          setGoal(refreshed.goal ?? "");
          setContent(refreshed.content);
          setTestPrompt(refreshed.testPrompt);
          setMaxIterations(normalizeMaxIterations(refreshed.maxIterations));
          setSavedSnapshot({
            name: refreshed.name,
            goal: refreshed.goal ?? "",
            content: refreshed.content,
            testPrompt: refreshed.testPrompt,
            maxIterations: normalizeMaxIterations(refreshed.maxIterations),
          });
        }
        setDeployFeedback({
          kind: "success",
          text: intl.formatMessage({ id: "settings.skillLab.deploySuccess" }),
        });
      } catch (err) {
        setDeployFeedback({
          kind: "error",
          text: `${intl.formatMessage({ id: "settings.skillLab.deployError" })}: ${String(err)}`,
        });
        console.error("Failed to deploy skill:", err);
      } finally {
        setDeployingId(null);
      }
    },
    [selectedId, skills, detail?.status, activeRunsBySkillId, loadList, intl],
  );

  // 删除草稿
  const handleDelete = useCallback(async (id: string) => {
    try {
      await invoke("skill_lab_delete", { skillId: id });
      setActiveRunsBySkillId((previous) => {
        if (!previous[id]) {
          return previous;
        }
        const next = { ...previous };
        delete next[id];
        return next;
      });
      if (selectedId === id) {
        setSelectedId(null);
        setEditorOpen(false);
        setResultDialogOpen(false);
        setDetail(null);
        setGoal("");
        setContent("");
        setTestPrompt("");
        setName("");
        setPythonInfo("");
        setGenerationError("");
        setDeployFeedback(null);
      }
      await loadList();
    } catch (err) {
      console.error("Failed to delete skill lab entry:", err);
    }
  }, [selectedId, loadList]);

  const closeEditor = useCallback(async () => {
    if (dirty && selectedId && name.trim()) {
      await persistCurrent(true);
    }
    setEditorOpen(false);
  }, [dirty, selectedId, name, persistCurrent]);

  // 自动保存：开启后编辑内容防抖落盘，避免离开或切换时丢失录入
  useEffect(() => {
    if (!autoSave || !dirty || !selectedId || !name.trim()) return;
    const timer = setTimeout(() => {
      void persistCurrent(true);
    }, 600);
    return () => clearTimeout(timer);
  }, [autoSave, dirty, selectedId, name, content, testPrompt, persistCurrent]);

  useEffect(() => {
    if (!editorOpen && !resultDialogOpen) {
      return;
    }
    const handler = (event: KeyboardEvent) => {
      if (event.key !== "Escape") {
        return;
      }
      if (resultDialogOpen) {
        setResultDialogOpen(false);
        return;
      }
      void closeEditor();
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [editorOpen, resultDialogOpen, closeEditor]);

  const selectedSummary = useMemo(
    () => skills.find((item) => item.id === selectedId) ?? null,
    [skills, selectedId],
  );
  const selectedActiveRun = selectedId ? activeRunsBySkillId[selectedId] : undefined;
  const currentStatus = statusFromRaw(
    selectedActiveRun?.phase ?? detail?.status ?? selectedSummary?.status,
  );
  const statusLabel = STATUS_LABELS[currentStatus] ?? STATUS_LABELS.idle;
  const statusColor = STATUS_COLORS[currentStatus] ?? STATUS_COLORS.idle;
  const currentDeployState = deployState(detail?.status ?? selectedSummary?.status);
  const currentDeployDisabledReasonId = selectedActiveRun
    ? "settings.skillLab.deployDisabledRunning"
    : currentDeployState.disabledReasonId;
  const currentCanDeploy = currentDeployState.canDeploy && !selectedActiveRun;
  const promoting = deployingId !== null;
  const totalIterations =
    detail?.iterationCount ?? selectedSummary?.iterationCount ?? 0;
  const selectedRunMax = selectedActiveRun?.maxIterations ?? FALLBACK_MAX_ITERATIONS;
  const testingProgressText = selectedActiveRun
    ? intl.formatMessage(
        { id: "settings.skillLab.progressRound" },
        {
          current: selectedActiveRun.iteration || 0,
          max: selectedRunMax,
        },
      )
    : "";
  const activeRuns = useMemo(
    () =>
      Object.values(activeRunsBySkillId).sort(
        (left, right) => right.updatedAt - left.updatedAt,
      ),
    [activeRunsBySkillId],
  );
  const {
    page: skillsPage,
    setPage: setSkillsPage,
    pageSize: skillsPageSize,
    totalItems: totalSkills,
    totalPages: totalSkillPages,
    pagedItems: pagedSkills,
  } = usePagedItems(skills);
  const {
    page: activeRunsPage,
    setPage: setActiveRunsPage,
    pageSize: activeRunsPageSize,
    totalItems: totalActiveRuns,
    totalPages: totalActiveRunPages,
    pagedItems: pagedActiveRuns,
  } = usePagedItems(activeRuns);
  const scoreHistoryTotal = detail?.scoreHistory.length ?? 0;
  const scoreHistoryTotalPages = Math.max(1, Math.ceil(scoreHistoryTotal / SCORE_HISTORY_PAGE_SIZE));
  const scoreHistoryPageSafe = Math.min(scoreHistoryPage, Math.max(0, scoreHistoryTotalPages - 1));
  const pagedScoreHistory = detail
    ? detail.scoreHistory.slice(
      scoreHistoryPageSafe * SCORE_HISTORY_PAGE_SIZE,
      (scoreHistoryPageSafe + 1) * SCORE_HISTORY_PAGE_SIZE,
    )
    : [];
  const hasActiveRuns = activeRuns.length > 0;
  const runRuleText = intl.formatMessage(
    { id: "settings.skillLab.ruleSummary" },
    { max: normalizeMaxIterations(maxIterations) },
  );
  const effectiveTestingPhase = testPhase ?? selectedActiveRun?.phase ?? null;
  const testingLabel = testing
    ? `${intl.formatMessage({ id: phaseLabelId(effectiveTestingPhase) })}${testingProgressText ? ` · ${testingProgressText}` : testIteration > 0 ? ` #${testIteration}` : ""}`
    : intl.formatMessage({ id: "settings.skillLab.runTest" });

  useEffect(() => {
    setScoreHistoryPage(0);
  }, [selectedId]);

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
        <p className="text-[11px] text-[var(--text-faint)]">
          {runRuleText}
        </p>
        {skills.length === 0 && !selectedId ? (
          <p className="text-xs text-[var(--text-faint)] px-2 py-4">
            {intl.formatMessage({ id: "settings.skillLab.empty" })}
          </p>
        ) : (
          <>
            <div className="overflow-hidden rounded-lg border border-[var(--border-subtle)]">
              <table className="w-full text-xs">
                <thead>
                  <tr className="bg-[var(--surface-soft)] text-[var(--text-faint)]">
                    <th className="px-2 py-2 text-left font-medium">
                      {intl.formatMessage({ id: "settings.skillLab.colName" })}
                    </th>
                    <th className="px-2 py-2 text-left font-medium">
                      {intl.formatMessage({ id: "settings.skillLab.colStatus" })}
                    </th>
                    <th className="px-2 py-2 text-right font-medium">
                      {intl.formatMessage({ id: "settings.skillLab.colIterations" })}
                    </th>
                    <th className="px-2 py-2 text-left font-medium">
                      {intl.formatMessage({ id: "settings.skillLab.colDeployed" })}
                    </th>
                    <th className="px-2 py-2 text-right font-medium">
                      {intl.formatMessage({ id: "settings.skillLab.colActions" })}
                    </th>
                  </tr>
                </thead>
                <tbody>
                  {pagedSkills.map((s) => {
                  const liveRun = activeRunsBySkillId[s.id];
                  const rowStatus = statusFromRaw(liveRun?.phase ?? s.status);
                  const rowStatusLabel = STATUS_LABELS[rowStatus] ?? STATUS_LABELS.idle;
                  const rowStatusColor = STATUS_COLORS[rowStatus] ?? STATUS_COLORS.idle;
                  const rowDeployState = deployState(s.status);
                  const rowCanDeploy = rowDeployState.canDeploy && !liveRun;
                  const rowDeploying = deployingId === s.id;
                  const rowDeployLabelId = rowDeploying
                    ? "settings.skillLab.deploying"
                    : rowDeployState.isDeployed
                      ? "settings.skillLab.deployed"
                      : "settings.skillLab.deploy";
                  const rowDeployDisabledReasonId = liveRun
                    ? "settings.skillLab.deployDisabledRunning"
                    : rowDeployState.disabledReasonId;
                  const rowDeployDisabledReason = rowDeployDisabledReasonId
                    ? intl.formatMessage({ id: rowDeployDisabledReasonId })
                    : undefined;
                  return (
                    <tr
                      key={s.id}
                      onClick={() => void handleSelect(s.id)}
                      className={`cursor-pointer border-t border-[var(--border-subtle)] transition-colors
                        ${selectedId === s.id ? "bg-[var(--accent-muted)]" : "hover:bg-[var(--surface-hover)]"}`}
                    >
                      <td className="px-2 py-2">
                        <div
                          className="max-w-[180px] truncate font-medium text-[var(--text-strong)]"
                          title={s.name || s.id}
                        >
                          {s.name || s.id}
                        </div>
                      </td>
                      <td className="px-2 py-2">
                        <div className="space-y-0.5">
                          <span className={rowStatusColor}>
                            {intl.formatMessage({ id: rowStatusLabel })}
                          </span>
                          {liveRun ? (
                            <div className="text-[10px] text-[var(--text-faint)]">
                              {intl.formatMessage(
                                { id: "settings.skillLab.progressRound" },
                                {
                                  current: liveRun.iteration,
                                  max: liveRun.maxIterations,
                                },
                              )}
                            </div>
                          ) : null}
                        </div>
                      </td>
                      <td className="px-2 py-2 text-right tabular-nums text-[var(--text-muted)]">
                        <span className="rounded-full bg-[var(--surface-hover)] px-2 py-0.5">
                          {s.iterationCount}
                        </span>
                      </td>
                      <td className="px-2 py-2">
                        {rowDeployState.isDeployed ? (
                          <span className="rounded-full bg-purple-100 px-2 py-0.5 text-[10px] text-purple-700 dark:bg-purple-900/30 dark:text-purple-300">
                            {intl.formatMessage({ id: "settings.skillLab.deployedYes" })}
                          </span>
                        ) : (
                          <span className="text-[var(--text-faint)]">
                            {intl.formatMessage({ id: "settings.skillLab.deployedNo" })}
                          </span>
                        )}
                      </td>
                      <td className="px-2 py-2">
                        <div className="flex items-center justify-end gap-1">
                          <button
                            onClick={(event) => {
                              event.stopPropagation();
                              if (!rowCanDeploy || promoting) {
                                return;
                              }
                              void handleDeploy(s.id);
                            }}
                            disabled={!rowCanDeploy || promoting}
                            title={rowDeployDisabledReason}
                            className="flex items-center gap-1 rounded-md bg-[var(--accent)] px-2 py-1 text-[11px] font-medium text-white hover:opacity-80 disabled:opacity-40"
                          >
                            {rowDeploying ? (
                              <IconLoader2 size={12} stroke={2} className="animate-spin" />
                            ) : (
                              <IconUpload size={12} stroke={2} />
                            )}
                            {intl.formatMessage({ id: rowDeployLabelId })}
                          </button>
                          <button
                            onClick={(event) => {
                              event.stopPropagation();
                              void handleDelete(s.id);
                            }}
                            className="rounded-md px-1.5 py-1 text-[11px] text-red-500 hover:bg-red-50 dark:hover:bg-red-900/20"
                          >
                            <IconTrash size={13} stroke={2} />
                          </button>
                        </div>
                      </td>
                    </tr>
                  );
                  })}
                </tbody>
              </table>
            </div>
            <SettingsPagination
              page={skillsPage}
              onPageChange={setSkillsPage}
              pageSize={skillsPageSize}
              totalItems={totalSkills}
              totalPages={totalSkillPages}
              className="flex items-center justify-between px-1 pt-2"
            />
          </>
        )}
        <p className="pt-2 text-[11px] text-[var(--text-faint)]">
          {intl.formatMessage({ id: "settings.skillLab.rowClickHint" })}
        </p>
      </section>

      {editorOpen && selectedId && (
        <div
          className="fixed bottom-0 left-0 right-0 top-8 z-50 flex items-center justify-center bg-black/45 p-4 backdrop-blur-sm"
          onClick={() => void closeEditor()}
        >
          <div
            className="thin-scrollbar max-h-[86vh] w-full max-w-[1080px] overflow-y-auto rounded-[var(--radius-lg)] border border-[var(--border-subtle)] bg-[var(--surface-panel)] p-5 shadow-[var(--shadow-strong)]"
            onClick={(event) => event.stopPropagation()}
          >
            <div className="flex items-start justify-between gap-3">
              <div className="space-y-1">
                <h4 className="text-sm font-semibold text-[var(--text-strong)]">
                  {intl.formatMessage({ id: "settings.skillLab.editorTitle" })}
                </h4>
                <p className="font-mono text-[11px] text-[var(--text-faint)]">
                  {selectedId}
                </p>
                <div className="flex flex-wrap items-center gap-2 text-[11px]">
                  <span className={statusColor}>
                    {intl.formatMessage({ id: statusLabel })}
                  </span>
                  <span className="rounded-full bg-[var(--surface-soft)] px-2 py-0.5 text-[var(--text-muted)]">
                    {intl.formatMessage(
                      { id: "settings.skillLab.totalIterations" },
                      { count: totalIterations },
                    )}
                  </span>
                  {testResult ? (
                    <span className="rounded-full bg-[var(--accent-muted)] px-2 py-0.5 text-[var(--text-base)]">
                      {intl.formatMessage(
                        { id: "settings.skillLab.currentIterations" },
                        { count: testResult.iterations },
                      )}
                    </span>
                  ) : null}
                </div>
              </div>
              <button
                type="button"
                onClick={() => void closeEditor()}
                className="icon-button"
              >
                <IconX size={16} stroke={2} />
              </button>
            </div>

            <div className="mt-4 space-y-4">
              <div>
                <label className="block text-xs font-medium text-[var(--text-base)] mb-1">
                  {intl.formatMessage({ id: "settings.skillLab.name" })}
                </label>
                <input
                  type="text"
                  value={name}
                  onChange={(event) => setName(event.target.value)}
                  placeholder={intl.formatMessage({ id: "settings.skillLab.namePlaceholder" })}
                  className="w-full rounded-lg border border-[var(--border-subtle)] bg-[var(--surface-main)]
                             px-3 py-2 text-xs text-[var(--text-base)]
                             placeholder:text-[var(--text-faint)]
                             focus:border-[var(--accent-strong)] focus:outline-none"
                />
              </div>

              <div>
                <label className="block text-xs font-medium text-[var(--text-base)] mb-1">
                  {intl.formatMessage({ id: "settings.skillLab.goal" })}
                </label>
                <textarea
                  value={goal}
                  onChange={(event) => setGoal(event.target.value)}
                  placeholder={intl.formatMessage({ id: "settings.skillLab.goalPlaceholder" })}
                  className="w-full min-h-[72px] rounded-lg border border-[var(--border-subtle)] bg-[var(--surface-main)]
                             p-3 text-xs text-[var(--text-base)]
                             placeholder:text-[var(--text-faint)]
                             focus:border-[var(--accent-strong)] focus:outline-none resize-y"
                />
                {pythonInfo ? (
                  <p className="mt-1 text-[11px] text-[var(--text-muted)]">
                    {intl.formatMessage({ id: "settings.skillLab.pythonReady" }, { python: pythonInfo })}
                  </p>
                ) : null}
                {checkingPython ? (
                  <p className="mt-1 text-[11px] text-[var(--text-muted)]">
                    {intl.formatMessage({ id: "settings.skillLab.checkingPython" })}
                  </p>
                ) : null}
                {generationError ? (
                  <p className="mt-1 text-[11px] text-red-500">{generationError}</p>
                ) : null}
              </div>

              <div>
                <label className="block text-xs font-medium text-[var(--text-base)] mb-1">
                  {intl.formatMessage({ id: "settings.skillLab.content" })}
                </label>
                <textarea
                  value={content}
                  onChange={(event) => setContent(event.target.value)}
                  placeholder={intl.formatMessage({ id: "settings.skillLab.contentPlaceholder" })}
                  className="w-full min-h-[200px] rounded-lg border border-[var(--border-subtle)] bg-[var(--surface-main)]
                             p-3 font-mono text-xs text-[var(--text-base)]
                             placeholder:text-[var(--text-faint)]
                             focus:border-[var(--accent-strong)] focus:outline-none resize-y"
                />
              </div>

              <div>
                <label className="block text-xs font-medium text-[var(--text-base)] mb-1">
                  {intl.formatMessage({ id: "settings.skillLab.testPrompt" })}
                </label>
                <textarea
                  value={testPrompt}
                  onChange={(event) => setTestPrompt(event.target.value)}
                  placeholder={intl.formatMessage({ id: "settings.skillLab.testPromptPlaceholder" })}
                  className="w-full min-h-[80px] rounded-lg border border-[var(--border-subtle)] bg-[var(--surface-main)]
                             p-3 text-xs text-[var(--text-base)]
                             placeholder:text-[var(--text-faint)]
                             focus:border-[var(--accent-strong)] focus:outline-none resize-y"
                />
              </div>

              <div>
                <label className="block text-xs font-medium text-[var(--text-base)] mb-1">
                  {intl.formatMessage({ id: "settings.skillLab.maxIterations" })}
                </label>
                <div className="flex flex-wrap items-center gap-3">
                  <input
                    type="number"
                    min={MIN_MAX_ITERATIONS}
                    max={MAX_MAX_ITERATIONS}
                    step={1}
                    value={maxIterations}
                    disabled={testing}
                    onChange={(event) => {
                      const next = Number(event.target.value);
                      setMaxIterations(
                        Number.isFinite(next)
                          ? normalizeMaxIterations(next)
                          : FALLBACK_MAX_ITERATIONS,
                      );
                    }}
                    className="w-28 rounded-lg border border-[var(--border-subtle)] bg-[var(--surface-main)]
                               px-3 py-2 text-xs text-[var(--text-base)]
                               focus:border-[var(--accent-strong)] focus:outline-none
                               disabled:opacity-50"
                  />
                  <span className="text-[11px] text-[var(--text-muted)]">
                    {intl.formatMessage(
                      { id: "settings.skillLab.maxIterationsHint" },
                      { min: MIN_MAX_ITERATIONS, max: MAX_MAX_ITERATIONS },
                    )}
                  </span>
                </div>
              </div>

              {detail?.scoreHistory?.length ? (
                <div className="rounded-lg border border-[var(--border-subtle)] bg-[var(--surface-main)] p-3 space-y-2">
                  <div className="flex flex-wrap items-center gap-3 text-xs">
                    <span className="text-[var(--text-base)]">
                      {intl.formatMessage({ id: "settings.skillLab.bestScore" })}:{" "}
                      <strong>{(detail.bestScore ?? 0).toFixed(1)}</strong>
                    </span>
                    <span className="rounded-full bg-green-100 px-2 py-0.5 text-[10px] text-green-700 dark:bg-green-900/30 dark:text-green-300">
                      {intl.formatMessage({ id: "settings.skillLab.bestVersion" })}
                    </span>
                    <span className="text-[var(--text-muted)]">
                      {intl.formatMessage(
                        { id: "settings.skillLab.stableRounds" },
                        { count: detail.stableRounds ?? 0 },
                      )}
                    </span>
                  </div>
                  <div className="max-h-[120px] overflow-y-auto text-[11px] text-[var(--text-muted)] space-y-1">
                    {pagedScoreHistory.map((score) => (
                      <div key={`score-${score.iteration}`}>
                        {intl.formatMessage(
                          { id: "settings.skillLab.scoreLine" },
                          {
                            iteration: score.iteration,
                            total: score.totalScore.toFixed(1),
                            clarity: score.clarity.toFixed(1),
                            robustness: score.robustness.toFixed(1),
                            executability: score.executability.toFixed(1),
                            maintainability: score.maintainability.toFixed(1),
                          },
                        )}
                      </div>
                    ))}
                  </div>
                  <SettingsPagination
                    page={scoreHistoryPageSafe}
                    onPageChange={setScoreHistoryPage}
                    pageSize={SCORE_HISTORY_PAGE_SIZE}
                    totalItems={scoreHistoryTotal}
                    totalPages={scoreHistoryTotalPages}
                    className="flex items-center justify-between pt-1"
                  />
                </div>
              ) : null}

              {detail?.lastTestResult && (
                <div className="rounded-lg border border-[var(--border-subtle)] bg-[var(--surface-main)] p-3">
                  <h5 className="text-xs font-medium text-[var(--text-base)] mb-1">
                    {intl.formatMessage({ id: "settings.skillLab.testResult" })}
                  </h5>
                  <pre className="text-[11px] text-[var(--text-muted)] whitespace-pre-wrap max-h-[180px] overflow-y-auto">
                    {detail.lastTestResult}
                  </pre>
                </div>
              )}

              {detail?.lastEvaluation && (
                <div className="rounded-lg border border-[var(--border-subtle)] bg-[var(--surface-main)] p-3">
                  <h5 className="text-xs font-medium text-[var(--text-base)] mb-1">
                    {intl.formatMessage({ id: "settings.skillLab.aiEval" })}
                  </h5>
                  <pre className="text-[11px] text-[var(--text-muted)] whitespace-pre-wrap max-h-[180px] overflow-y-auto">
                    {detail.lastEvaluation}
                  </pre>
                </div>
              )}

              {deployFeedback ? (
                <div
                  className={`rounded-lg border px-3 py-2 text-[11px] ${
                    deployFeedback.kind === "success"
                      ? "border-[var(--accent-border)] bg-[var(--accent-soft)] text-[var(--accent-strong)]"
                      : "border-red-300 bg-red-50 text-red-600 dark:border-red-900/50 dark:bg-red-900/20 dark:text-red-300"
                  }`}
                >
                  {deployFeedback.text}
                </div>
              ) : null}

              <div className="flex items-center gap-3 text-xs">
                <label className="flex items-center gap-1.5 cursor-pointer select-none text-[var(--text-base)]">
                  <input
                    type="checkbox"
                    checked={autoSave}
                    onChange={(event) => setAutoSave(event.target.checked)}
                    className="accent-[var(--accent)]"
                  />
                  {intl.formatMessage({ id: "settings.skillLab.autoSave" })}
                </label>
                {dirty && !saving ? (
                  <span className="text-[var(--text-muted)]">
                    {intl.formatMessage({ id: "settings.skillLab.unsaved" })}
                  </span>
                ) : null}
              </div>

              <div className="flex items-center gap-2 flex-wrap">
                <button
                  onClick={handleGenerate}
                  disabled={generating || checkingPython || !goal.trim()}
                  className="flex items-center gap-1 rounded-lg px-3 py-1.5 text-xs font-medium
                             bg-indigo-600 text-white hover:opacity-80 transition-opacity
                             disabled:opacity-50"
                >
                  {generating || checkingPython ? (
                    <IconLoader2 size={14} stroke={2} className="animate-spin" />
                  ) : (
                    <IconSparkles size={14} stroke={2} />
                  )}
                  {checkingPython
                    ? intl.formatMessage({ id: "settings.skillLab.checkingPython" })
                    : intl.formatMessage({
                        id: generating
                          ? "settings.skillLab.generating"
                          : "settings.skillLab.generate",
                      })}
                </button>

                <button
                  onClick={handleSave}
                  disabled={saving || !name.trim()}
                  className="flex items-center gap-1 rounded-lg px-3 py-1.5 text-xs font-medium
                             border border-[var(--border-subtle)] text-[var(--text-base)]
                             hover:bg-[var(--surface-hover)] transition-colors disabled:opacity-50"
                >
                  {saving ? (
                    <IconLoader2 size={14} stroke={2} className="animate-spin" />
                  ) : saved ? (
                    <IconCheck size={14} stroke={2} />
                  ) : null}
                  {saving
                    ? intl.formatMessage({ id: "settings.skillLab.saving" })
                    : intl.formatMessage({
                        id: saved ? "settings.skillLab.saved" : "settings.skillLab.save",
                      })}
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
                  {testingLabel}
                </button>

                <button
                  onClick={() => void handleDeploy()}
                  disabled={promoting || !currentCanDeploy}
                  title={
                    currentDeployDisabledReasonId
                      ? intl.formatMessage({ id: currentDeployDisabledReasonId })
                      : undefined
                  }
                  className="flex items-center gap-1 rounded-lg px-3 py-1.5 text-xs font-medium
                             bg-green-600 text-white hover:opacity-80 transition-opacity
                             disabled:opacity-50"
                >
                  {deployingId === selectedId ? (
                    <IconLoader2 size={14} stroke={2} className="animate-spin" />
                  ) : (
                    <IconUpload size={14} stroke={2} />
                  )}
                  {intl.formatMessage({
                    id: deployingId === selectedId
                      ? "settings.skillLab.deploying"
                      : currentDeployState.isDeployed
                        ? "settings.skillLab.deployed"
                        : "settings.skillLab.deploy",
                  })}
                </button>

                <button
                  onClick={() => void handleDelete(selectedId)}
                  className="flex items-center gap-1 rounded-lg px-3 py-1.5 text-xs font-medium
                             text-red-500 hover:bg-red-50 dark:hover:bg-red-900/20 transition-colors"
                >
                  <IconTrash size={14} stroke={2} />
                </button>
              </div>

              {!currentCanDeploy && currentDeployDisabledReasonId ? (
                <p className="text-[11px] text-[var(--text-muted)]">
                  {intl.formatMessage({ id: currentDeployDisabledReasonId })}
                </p>
              ) : null}
            </div>
          </div>
        </div>
      )}

      {resultDialogOpen && (
        <div
          className="fixed bottom-0 left-0 right-0 top-8 z-[60] flex items-center justify-center bg-black/50 p-4 backdrop-blur-sm"
          onClick={() => setResultDialogOpen(false)}
        >
          <div
            className="thin-scrollbar max-h-[84vh] w-full max-w-3xl overflow-y-auto rounded-[var(--radius-lg)] border border-[var(--border-subtle)] bg-[var(--surface-panel)] p-5 shadow-[var(--shadow-strong)]"
            onClick={(event) => event.stopPropagation()}
          >
            <div className="mb-4 flex items-center justify-between gap-3">
              <h4 className="text-sm font-semibold text-[var(--text-strong)]">
                {intl.formatMessage({ id: "settings.skillLab.resultDialogTitle" })}
              </h4>
              <button
                type="button"
                onClick={() => setResultDialogOpen(false)}
                className="icon-button"
              >
                <IconX size={16} stroke={2} />
              </button>
            </div>

            {testError ? (
              <div className="rounded-lg border border-red-300 bg-red-50 px-3 py-2 text-[11px] text-red-600 dark:border-red-900/50 dark:bg-red-900/20 dark:text-red-300">
                {testError}
              </div>
            ) : null}

            {testResult ? (
              <div className="space-y-3">
                <div className="rounded-lg border border-[var(--border-subtle)] bg-[var(--surface-main)] p-3">
                  <div className="flex flex-wrap items-center gap-3 text-xs">
                    <span className="text-[var(--text-base)] font-medium">
                      {intl.formatMessage({ id: "settings.skillLab.testSummary" })}
                    </span>
                    <span className={STATUS_COLORS[statusFromRaw(testResult.status)]}>
                      {intl.formatMessage({ id: STATUS_LABELS[statusFromRaw(testResult.status)] })}
                    </span>
                    <span className="rounded-full bg-[var(--accent-muted)] px-2 py-0.5 text-[var(--text-base)]">
                      {intl.formatMessage(
                        { id: "settings.skillLab.currentIterations" },
                        { count: testResult.iterations },
                      )}
                    </span>
                    <span className="rounded-full bg-[var(--surface-hover)] px-2 py-0.5 text-[var(--text-muted)]">
                      {intl.formatMessage(
                        { id: "settings.skillLab.totalIterations" },
                        { count: totalIterations },
                      )}
                    </span>
                    <span className="text-[var(--text-muted)]">
                      {intl.formatMessage({ id: "settings.skillLab.bestScore" })}:{" "}
                      <strong>{(testResult.bestScore ?? 0).toFixed(1)}</strong>
                    </span>
                  </div>
                </div>

                <div className="rounded-lg border border-[var(--border-subtle)] bg-[var(--surface-main)] p-3">
                  <h5 className="mb-1 text-xs font-medium text-[var(--text-base)]">
                    {intl.formatMessage({ id: "settings.skillLab.aiOutput" })}
                  </h5>
                  <pre className="text-[11px] text-[var(--text-muted)] whitespace-pre-wrap max-h-[240px] overflow-y-auto">
                    {testResult.lastOutput}
                  </pre>
                </div>

                {testResult.evaluation ? (
                  <div className="rounded-lg border border-[var(--border-subtle)] bg-[var(--surface-main)] p-3">
                    <h5 className="mb-1 text-xs font-medium text-[var(--text-base)]">
                      {intl.formatMessage({ id: "settings.skillLab.aiEval" })}
                    </h5>
                    <pre className="text-[11px] text-[var(--text-muted)] whitespace-pre-wrap max-h-[240px] overflow-y-auto">
                      {testResult.evaluation}
                    </pre>
                  </div>
                ) : null}
              </div>
            ) : null}

            {!testError && !testResult ? (
              <p className="text-xs text-[var(--text-muted)]">
                {intl.formatMessage({ id: "settings.skillLab.resultEmpty" })}
              </p>
            ) : null}

            <div className="mt-4 flex justify-end">
              <button
                type="button"
                onClick={() => setResultDialogOpen(false)}
                className="rounded-[var(--radius-sm)] border border-[var(--border-subtle)] px-3 py-1.5 text-xs text-[var(--text-muted)] transition-colors hover:bg-[var(--surface-elevated)]"
              >
                {intl.formatMessage({ id: "common.close" })}
              </button>
            </div>
          </div>
        </div>
      )}

      {hasActiveRuns && !progressDockHidden ? (
        <div className="fixed bottom-4 right-4 z-[58] w-[380px] max-w-[calc(100vw-2rem)] rounded-[var(--radius-lg)] border border-[var(--border-subtle)] bg-[var(--surface-panel)] shadow-[var(--shadow-strong)]">
          <div className="flex items-center justify-between border-b border-[var(--border-subtle)] px-3 py-2">
            <div className="text-xs font-medium text-[var(--text-strong)]">
              {intl.formatMessage({ id: "settings.skillLab.progressDockTitle" })}
            </div>
            <button
              type="button"
              onClick={() => setProgressDockHidden(true)}
              className="icon-button"
            >
              <IconX size={14} stroke={2} />
            </button>
          </div>
          <div className="thin-scrollbar max-h-[340px] space-y-2 overflow-y-auto p-3">
            {pagedActiveRuns.map((run) => (
              <div
                key={run.skillId}
                className="rounded-md border border-[var(--border-subtle)] bg-[var(--surface-main)] p-2"
              >
                <div className="mb-1 flex items-center justify-between gap-2">
                  <span className="truncate font-mono text-[11px] text-[var(--text-base)]">
                    {run.skillId}
                  </span>
                  <span className={`text-[11px] ${STATUS_COLORS[statusFromRaw(run.phase)]}`}>
                    {intl.formatMessage({ id: STATUS_LABELS[statusFromRaw(run.phase)] })}
                  </span>
                </div>
                <div className="mb-1 text-[10px] text-[var(--text-faint)]">
                  {intl.formatMessage(
                    { id: "settings.skillLab.progressRound" },
                    { current: run.iteration, max: run.maxIterations },
                  )}
                </div>
                {run.retryAttempt && run.retryMax ? (
                  <div className="mb-1 text-[10px] text-yellow-500">
                    {intl.formatMessage(
                      { id: "settings.skillLab.retrying" },
                      {
                        current: run.retryAttempt,
                        max: run.retryMax,
                        delayMs: run.retryDelayMs ?? 0,
                      },
                    )}
                  </div>
                ) : null}
                {run.score !== null ? (
                  <div className="mb-1 text-[10px] text-[var(--text-muted)]">
                    {intl.formatMessage(
                      { id: "settings.skillLab.progressScore" },
                      { score: run.score.toFixed(1) },
                    )}
                  </div>
                ) : null}
                <div className="space-y-1 text-[10px] text-[var(--text-muted)]">
                  {run.logs.length ? (
                    run.logs.slice(-4).map((line, index) => (
                      <p key={`${run.skillId}-log-${index}`} className="whitespace-pre-wrap break-words">
                        {line}
                      </p>
                    ))
                  ) : (
                    <p>{intl.formatMessage({ id: "settings.skillLab.progressNoLogs" })}</p>
                  )}
                </div>
              </div>
            ))}
            <SettingsPagination
              page={activeRunsPage}
              onPageChange={setActiveRunsPage}
              pageSize={activeRunsPageSize}
              totalItems={totalActiveRuns}
              totalPages={totalActiveRunPages}
            />
          </div>
        </div>
      ) : null}

      {hasActiveRuns && progressDockHidden ? (
        <button
          type="button"
          onClick={() => setProgressDockHidden(false)}
          className="fixed bottom-4 right-4 z-[58] rounded-full border border-[var(--border-subtle)] bg-[var(--surface-panel)] px-3 py-1.5 text-[11px] text-[var(--text-base)] shadow-[var(--shadow-strong)]"
        >
          {intl.formatMessage({ id: "settings.skillLab.progressDockRestore" })}
        </button>
      ) : null}
    </div>
  );
}
