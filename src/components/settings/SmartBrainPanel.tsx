import { IconChevronDown, IconChevronRight, IconTrash, IconUpload } from "@tabler/icons-react";
import { useCallback, useEffect, useState } from "react";
import { useIntl } from "react-intl";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";

interface ExperienceEntry {
  thread_id: string;
  extracted_at: number;
  usage_count: number;
  last_used_at: number | null;
  summary_slug: string | null;
  categories: string[];
}

interface KnowledgeEntry {
  doc_id: string;
  source_file: string;
  source_type: string;
  title: string;
  added_at: number;
  chunk_count: number;
  categories: string[];
}

function formatDate(ts: number): string {
  if (ts <= 0) return "-";
  return new Date(ts * 1000).toLocaleDateString();
}

export function SmartBrainPanel() {
  const intl = useIntl();

  const [enabled, setEnabled] = useState(false);
  const [toggleLoading, setToggleLoading] = useState(false);

  const [experiences, setExperiences] = useState<ExperienceEntry[]>([]);
  const [knowledge, setKnowledge] = useState<KnowledgeEntry[]>([]);

  const [expExpanded, setExpExpanded] = useState(true);
  const [knowExpanded, setKnowExpanded] = useState(true);

  const [expandedExp, setExpandedExp] = useState<string | null>(null);
  const [expandedKnow, setExpandedKnow] = useState<string | null>(null);
  const [expContent, setExpContent] = useState<Record<string, string>>({});
  const [knowContent, setKnowContent] = useState<Record<string, string>>({});

  const [uploading, setUploading] = useState(false);

  const loadConfig = useCallback(async () => {
    try {
      const result = await invoke<{ config?: { smartbrain?: { enabled?: boolean } } }>(
        "standalone_config_read"
      );
      if (result?.config?.smartbrain?.enabled !== undefined) {
        setEnabled(result.config.smartbrain.enabled);
      }
    } catch { /* ignore */ }
  }, []);

  const loadExperiences = useCallback(async () => {
    try {
      const result = await invoke<{ entries: ExperienceEntry[] }>("smartbrain_list_experiences");
      setExperiences(result.entries ?? []);
    } catch { /* ignore */ }
  }, []);

  const loadKnowledge = useCallback(async () => {
    try {
      const result = await invoke<{ entries: KnowledgeEntry[] }>("smartbrain_list_knowledge");
      setKnowledge(result.entries ?? []);
    } catch { /* ignore */ }
  }, []);

  useEffect(() => {
    loadConfig();
    loadExperiences();
    loadKnowledge();
  }, [loadConfig, loadExperiences, loadKnowledge]);

  const handleToggle = useCallback(async () => {
    if (toggleLoading) return;
    setToggleLoading(true);
    try {
      const newValue = !enabled;
      await invoke("standalone_config_write", {
        edits: [{ keyPath: "smartbrain.enabled", value: newValue }],
      });
      setEnabled(newValue);
    } catch (err) {
      console.error("SmartBrain toggle failed:", err);
    } finally {
      setToggleLoading(false);
    }
  }, [enabled, toggleLoading]);

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
        const result = await invoke<{ content: string }>("smartbrain_read_experience", { threadId });
        setExpContent((prev) => ({ ...prev, [threadId]: result.content }));
      } catch { /* ignore */ }
    }
    setExpandedExp(threadId);
  }, [expandedExp, expContent]);

  const handleDeleteKnowledge = useCallback(async (docId: string) => {
    if (!confirm(intl.formatMessage({ id: "settings.smartbrain.knowledge.deleteConfirm" }))) return;
    try {
      await invoke("smartbrain_delete_knowledge", { docId });
      setKnowledge((prev) => prev.filter((k) => k.doc_id !== docId));
      setExpandedKnow(null);
    } catch (err) {
      console.error("Delete knowledge failed:", err);
    }
  }, [intl]);

  const handleExpandKnowledge = useCallback(async (docId: string) => {
    if (expandedKnow === docId) {
      setExpandedKnow(null);
      return;
    }
    if (!knowContent[docId]) {
      try {
        const result = await invoke<{ content: string }>("smartbrain_read_knowledge", { docId });
        setKnowContent((prev) => ({ ...prev, [docId]: result.content }));
      } catch { /* ignore */ }
    }
    setExpandedKnow(docId);
  }, [expandedKnow, knowContent]);

  const handleUploadKnowledge = useCallback(async () => {
    if (uploading) return;
    try {
      const selected = await open({
        multiple: false,
        filters: [
          { name: "Documents", extensions: ["md", "txt", "pdf", "json", "yaml", "yml", "toml", "csv", "html", "xml"] },
        ],
      });
      if (!selected) return;
      const filePath = selected as string;
      if (!filePath) return;

      setUploading(true);
      await invoke("smartbrain_upload_knowledge", { filePath });
      await loadKnowledge();
    } catch (err) {
      console.error("Upload knowledge failed:", err);
    } finally {
      setUploading(false);
    }
  }, [uploading, loadKnowledge]);

  return (
    <div className="space-y-5">
      {/* Master toggle */}
      <section className="settings-card space-y-3">
        <div className="flex items-center justify-between">
          <h4 className="text-[13px] font-semibold text-[var(--text-strong)]">
            SmartBrain ({intl.formatMessage({ id: "settings.smartbrain" })})
          </h4>
          <div className="flex items-center gap-3">
            <button
              onClick={handleToggle}
              disabled={toggleLoading}
              className={`relative inline-flex h-5 w-9 shrink-0 cursor-pointer items-center rounded-full transition-colors ${
                enabled ? "bg-[var(--accent)]" : "bg-[var(--border-subtle)]"
              } ${toggleLoading ? "opacity-50" : ""}`}
            >
              <span
                className={`inline-block h-3.5 w-3.5 rounded-full bg-white shadow-sm transition-transform ${
                  enabled ? "translate-x-[18px]" : "translate-x-[3px]"
                }`}
              />
            </button>
            <span className="text-xs text-[var(--text-muted)]">
              {toggleLoading
                ? "..."
                : enabled
                  ? intl.formatMessage({ id: "settings.smartbrain.enabled" })
                  : intl.formatMessage({ id: "settings.smartbrain.disabled" })}
            </span>
          </div>
        </div>
        <p className="text-xs text-[var(--text-faint)]">
          {intl.formatMessage({ id: "settings.smartbrain.toggle.description" })}
        </p>
      </section>

      {enabled && (
        <>
          {/* Experience section */}
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
                        <span className="flex-1 truncate text-xs font-medium text-[var(--text-strong)]">
                          {exp.summary_slug ?? exp.thread_id}
                        </span>
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

          {/* Knowledge section */}
          <section className="settings-card space-y-3">
            <button
              onClick={() => setKnowExpanded(!knowExpanded)}
              className="flex w-full items-center gap-2 text-left"
            >
              {knowExpanded ? <IconChevronDown size={14} /> : <IconChevronRight size={14} />}
              <h4 className="text-[13px] font-semibold text-[var(--text-strong)]">
                {intl.formatMessage({ id: "settings.smartbrain.knowledge" })}
              </h4>
              <span className="ml-auto text-xs text-[var(--text-faint)]">({knowledge.length})</span>
            </button>
            <p className="text-xs text-[var(--text-faint)]">
              {intl.formatMessage({ id: "settings.smartbrain.knowledge.description" })}
            </p>

            {knowExpanded && (
              <div className="space-y-2">
                <button
                  onClick={handleUploadKnowledge}
                  disabled={uploading}
                  className="app-button-secondary flex items-center gap-1.5 text-xs"
                >
                  <IconUpload size={13} stroke={1.8} />
                  {uploading
                    ? intl.formatMessage({ id: "settings.smartbrain.knowledge.uploading" })
                    : intl.formatMessage({ id: "settings.smartbrain.knowledge.upload" })}
                </button>

                {knowledge.length === 0 ? (
                  <p className="py-4 text-center text-xs text-[var(--text-faint)]">
                    {intl.formatMessage({ id: "settings.smartbrain.knowledge.empty" })}
                  </p>
                ) : (
                  knowledge.map((doc) => (
                    <div
                      key={doc.doc_id}
                      className="rounded-md border border-[var(--border-subtle)] bg-[var(--surface-soft)]/50"
                    >
                      <div
                        className="flex cursor-pointer items-center gap-2 px-3 py-2"
                        onClick={() => handleExpandKnowledge(doc.doc_id)}
                      >
                        <span className="h-1.5 w-1.5 shrink-0 rounded-full bg-blue-500" />
                        <span className="flex-1 truncate text-xs font-medium text-[var(--text-strong)]">
                          {doc.title}
                        </span>
                        <span className="shrink-0 rounded bg-[var(--border-subtle)] px-1 py-0.5 text-[9px] text-[var(--text-faint)]">
                          {doc.source_type}
                        </span>
                        <span className="shrink-0 text-[10px] text-[var(--text-faint)]">
                          {formatDate(doc.added_at)}
                        </span>
                        <button
                          onClick={(e) => {
                            e.stopPropagation();
                            handleDeleteKnowledge(doc.doc_id);
                          }}
                          className="icon-button shrink-0 opacity-50 hover:opacity-100"
                        >
                          <IconTrash size={13} stroke={1.6} />
                        </button>
                      </div>
                      <div className="flex items-center gap-2 px-3 pb-1.5">
                        <span className="text-[10px] text-[var(--text-faint)]">
                          {intl.formatMessage(
                            { id: "settings.smartbrain.knowledge.chunks" },
                            { count: doc.chunk_count }
                          )}
                        </span>
                        {doc.categories.length > 0 && (
                          <div className="flex gap-1">
                            {doc.categories.slice(0, 4).map((cat) => (
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
                      {expandedKnow === doc.doc_id && knowContent[doc.doc_id] && (
                        <div className="thin-scrollbar max-h-60 overflow-y-auto border-t border-[var(--border-subtle)] px-3 py-2">
                          <pre className="whitespace-pre-wrap text-[11px] text-[var(--text-muted)]">
                            {knowContent[doc.doc_id]}
                          </pre>
                        </div>
                      )}
                    </div>
                  ))
                )}
              </div>
            )}
          </section>
        </>
      )}
    </div>
  );
}
