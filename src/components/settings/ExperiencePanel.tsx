import { IconChevronDown, IconChevronRight, IconTrash, IconAlertCircle } from "@tabler/icons-react";
import { useCallback, useEffect, useState } from "react";
import { useIntl } from "react-intl";
import { invoke } from "@tauri-apps/api/core";

interface ExperienceEntry {
  thread_id: string;
  extracted_at: number;
  usage_count: number;
  last_used_at: number | null;
  summary_slug: string | null;
  title: string | null;
  summary: string | null;
  categories: string[];
}

interface OkfFrontmatter {
  type: string;
  title?: string;
  description?: string;
  tags?: string[];
  timestamp?: string;
}

function formatDate(ts: number): string {
  if (ts <= 0) return "-";
  return new Date(ts * 1000).toLocaleDateString();
}

export function ExperiencePanel() {
  const intl = useIntl();

  const [enabled, setEnabled] = useState(true);
  const [experiences, setExperiences] = useState<ExperienceEntry[]>([]);
  const [expExpanded, setExpExpanded] = useState(true);
  const [expandedExp, setExpandedExp] = useState<string | null>(null);
  const [expContent, setExpContent] = useState<Record<string, string>>({});
  const [expFrontmatter, setExpFrontmatter] = useState<Record<string, OkfFrontmatter | null>>({});

  useEffect(() => {
    invoke<{ config?: { smartbrain?: { enabled?: boolean } } }>("standalone_config_read")
      .then((result) => {
        setEnabled(result?.config?.smartbrain?.enabled ?? false);
      })
      .catch(() => {});
  }, []);

  const loadExperiences = useCallback(async () => {
    try {
      const result = await invoke<{ entries: ExperienceEntry[] }>("smartbrain_list_experiences");
      setExperiences(result.entries ?? []);
    } catch { /* ignore */ }
  }, []);

  useEffect(() => {
    loadExperiences();
  }, [loadExperiences]);

  const handleDeleteExperience = useCallback(async (threadId: string) => {
    if (!confirm(intl.formatMessage({ id: "settings.smartbrain.experience.deleteConfirm" }))) return;
    try {
      await invoke("smartbrain_delete_experience", { threadId });
      setExperiences((prev) => prev.filter((e) => e.thread_id !== threadId));
      setExpandedExp(null);
    } catch (err) {
      console.error("Delete experience failed:", err);
    }
  }, [intl]);

  const handleExpandExperience = useCallback(async (threadId: string) => {
    if (expandedExp === threadId) {
      setExpandedExp(null);
      return;
    }
    if (!expContent[threadId]) {
      try {
        const result = await invoke<{ content: string; frontmatter?: OkfFrontmatter | null }>("smartbrain_read_experience", { threadId });
        setExpContent((prev) => ({ ...prev, [threadId]: result.content }));
        if (result.frontmatter) {
          setExpFrontmatter((prev) => ({ ...prev, [threadId]: result.frontmatter ?? null }));
        }
      } catch { /* ignore */ }
    }
    setExpandedExp(threadId);
  }, [expandedExp, expContent]);

  return (
    <div className="space-y-5">
      {!enabled && (
        <div className="flex items-center gap-2 rounded-md border border-amber-500/30 bg-amber-500/5 px-3 py-2.5">
          <IconAlertCircle size={15} stroke={1.8} className="shrink-0 text-amber-500" />
          <p className="text-xs text-[var(--text-muted)]">
            {intl.formatMessage({ id: "settings.smartbrain.enableHint" })}
          </p>
        </div>
      )}
      {enabled && (
        <div className="flex items-center gap-2 rounded-md border border-[var(--border-muted)] bg-[var(--bg-subtle)] px-3 py-2">
          <IconAlertCircle size={14} stroke={1.5} className="shrink-0 text-[var(--text-faint)]" />
          <p className="text-xs text-[var(--text-faint)]">
            {intl.formatMessage({ id: "settings.smartbrain.enabledNote" })}
          </p>
        </div>
      )}
      <section className="settings-card space-y-3">
        <button
          onClick={() => setExpExpanded(!expExpanded)}
          className="flex w-full items-center gap-2 text-left"
        >
          {expExpanded ? <IconChevronDown size={14} /> : <IconChevronRight size={14} />}
          <h4 className="text-[13px] font-semibold text-[var(--text-strong)]">
            {intl.formatMessage({ id: "settings.smartbrain.experience" })}
          </h4>
          <span className="ml-auto text-xs text-[var(--text-faint)]">({experiences.length})</span>
        </button>
        <p className="text-xs text-[var(--text-faint)]">
          {intl.formatMessage({ id: "settings.smartbrain.experience.description" })}
        </p>

        {expExpanded && (
          <div className="space-y-1">
            {experiences.length === 0 ? (
              <p className="py-4 text-center text-xs text-[var(--text-faint)]">
                {intl.formatMessage({ id: "settings.smartbrain.experience.empty" })}
              </p>
            ) : (
              experiences.map((exp) => (
                <div
                  key={exp.thread_id}
                  className="rounded-md border border-[var(--border-subtle)] bg-[var(--surface-soft)]/50"
                >
                  <div
                    className="flex cursor-pointer items-center gap-2 px-3 py-2"
                    onClick={() => handleExpandExperience(exp.thread_id)}
                  >
                    <span className="h-1.5 w-1.5 shrink-0 rounded-full bg-[var(--accent)]" />
                    <div className="flex min-w-0 flex-1 flex-col gap-0.5">
                      <span className="truncate text-xs font-medium text-[var(--text-strong)]">
                        {exp.title ?? exp.summary_slug ?? exp.thread_id}
                      </span>
                      {exp.summary && (
                        <span className="truncate text-[10px] text-[var(--text-muted)]">
                          {exp.summary}
                        </span>
                      )}
                    </div>
                    <span className="shrink-0 text-[10px] text-[var(--text-faint)]">
                      {formatDate(exp.extracted_at)}
                    </span>
                    <button
                      onClick={(e) => {
                        e.stopPropagation();
                        handleDeleteExperience(exp.thread_id);
                      }}
                      className="icon-button shrink-0 opacity-50 hover:opacity-100"
                    >
                      <IconTrash size={13} stroke={1.6} />
                    </button>
                  </div>
                  <div className="flex items-center gap-2 px-3 pb-1.5">
                    <span className="text-[10px] text-[var(--text-faint)]">
                      {intl.formatMessage(
                        { id: "settings.smartbrain.experience.usageCount" },
                        { count: exp.usage_count }
                      )}
                    </span>
                    {exp.categories.length > 0 && (
                      <div className="flex gap-1">
                        {exp.categories.slice(0, 4).map((cat) => (
                          <span
                            key={cat}
                            className="rounded bg-[var(--border-subtle)] px-1.5 py-0.5 text-[9px] text-[var(--text-faint)]"
                          >
                            {cat}
                          </span>
                        ))}
                      </div>
                    )}
                  </div>
                  {expandedExp === exp.thread_id && expContent[exp.thread_id] && (
                    <div className="thin-scrollbar max-h-60 overflow-y-auto border-t border-[var(--border-subtle)] px-3 py-2">
                      {expFrontmatter[exp.thread_id] && (
                        <div className="mb-2 flex flex-wrap items-center gap-2 text-[10px] text-[var(--text-faint)]">
                          <span className="rounded bg-[var(--accent)]/10 px-1.5 py-0.5 text-[var(--accent)]">
                            OKF: {expFrontmatter[exp.thread_id]!.type}
                          </span>
                          {expFrontmatter[exp.thread_id]!.timestamp && (
                            <span>{expFrontmatter[exp.thread_id]!.timestamp}</span>
                          )}
                        </div>
                      )}
                      <pre className="whitespace-pre-wrap text-[11px] text-[var(--text-muted)]">
                        {expContent[exp.thread_id]}
                      </pre>
                    </div>
                  )}
                </div>
              ))
            )}
          </div>
        )}
      </section>
    </div>
  );
}
