import {
  IconChevronDown,
  IconChevronRight,
  IconTrash,
  IconUpload,
  IconAlertCircle,
  IconRefresh,
  IconFolder,
  IconPencil,
  IconCheck,
  IconX,
} from "@tabler/icons-react";
import { useCallback, useEffect, useState } from "react";
import { useIntl } from "react-intl";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { SettingsPagination, usePagedItems } from "./SettingsPagination";

interface KnowledgeEntry {
  doc_id: string;
  source_file: string;
  source_type: string;
  title: string;
  description?: string;
  added_at: number;
  chunk_count: number;
  categories: string[];
  domain?: string;
  relative_path?: string;
  source_group?: string;
}

interface OkfFrontmatter {
  type: string;
  title?: string;
  description?: string;
  tags?: string[];
  timestamp?: string;
}

interface FolderUploadFailure {
  relative_path: string;
  error: string;
}

interface FolderUploadResponse {
  status: string;
  requires_confirmation?: boolean;
  candidate_count?: number;
  skipped_count?: number;
  threshold?: number;
  preview_paths?: string[];
  imported_count?: number;
  failed_count?: number;
  failures?: FolderUploadFailure[];
}

const KNOWLEDGE_ALLOWED_EXTENSIONS = [
  "pdf",
  "docx",
  "xlsx",
  "xls",
  "md",
  "txt",
  "json",
  "yaml",
  "yml",
  "toml",
  "csv",
  "html",
  "xml",
];

function formatDate(ts: number): string {
  if (ts <= 0) return "-";
  return new Date(ts * 1000).toLocaleDateString();
}

export function KnowledgePanel() {
  const intl = useIntl();

  const [knowledge, setKnowledge] = useState<KnowledgeEntry[]>([]);
  const [knowExpanded, setKnowExpanded] = useState(true);
  const [expandedKnow, setExpandedKnow] = useState<string | null>(null);
  const [knowContent, setKnowContent] = useState<Record<string, string>>({});
  const [knowFrontmatter, setKnowFrontmatter] = useState<Record<string, OkfFrontmatter | null>>({});
  const [uploading, setUploading] = useState(false);
  const [uploadingFolder, setUploadingFolder] = useState(false);
  const [migrating, setMigrating] = useState(false);
  const [editingDocId, setEditingDocId] = useState<string | null>(null);
  const [savingEdit, setSavingEdit] = useState(false);
  const [editForm, setEditForm] = useState({
    title: "",
    description: "",
    tagsText: "",
    domain: "",
    sourceGroup: "",
  });
  const [notice, setNotice] = useState<{
    kind: "success" | "error" | "warning" | "info";
    text: string;
  } | null>(null);
  const {
    page: knowledgePage,
    setPage: setKnowledgePage,
    pageSize: knowledgePageSize,
    totalItems: totalKnowledgeItems,
    totalPages: totalKnowledgePages,
    pagedItems: pagedKnowledge,
  } = usePagedItems(knowledge);

  const loadKnowledge = useCallback(async () => {
    try {
      const result = await invoke<{ entries: KnowledgeEntry[] }>("smartbrain_list_knowledge");
      setKnowledge(result.entries ?? []);
    } catch { /* ignore */ }
  }, []);

  useEffect(() => {
    loadKnowledge();
  }, [loadKnowledge]);

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

  const handleStartEditKnowledge = useCallback(async (doc: KnowledgeEntry) => {
    let frontmatter = knowFrontmatter[doc.doc_id];
    if (!frontmatter) {
      try {
        const result = await invoke<{ content: string; frontmatter?: OkfFrontmatter | null }>(
          "smartbrain_read_knowledge",
          { docId: doc.doc_id },
        );
        frontmatter = result.frontmatter ?? null;
        if (result.frontmatter) {
          setKnowFrontmatter((prev) => ({ ...prev, [doc.doc_id]: result.frontmatter ?? null }));
        }
      } catch {
        frontmatter = null;
      }
    }
    const tags = frontmatter?.tags?.length ? frontmatter.tags : doc.categories;
    setEditForm({
      title: doc.title ?? "",
      description: frontmatter?.description ?? doc.description ?? "",
      tagsText: tags.join(", "),
      domain: doc.domain ?? "",
      sourceGroup: doc.source_group ?? "",
    });
    setEditingDocId(doc.doc_id);
  }, [knowFrontmatter]);

  const handleCancelEditKnowledge = useCallback(() => {
    setEditingDocId(null);
    setSavingEdit(false);
  }, []);

  const handleSaveEditKnowledge = useCallback(async () => {
    if (!editingDocId || savingEdit) return;
    const normalizedTitle = editForm.title.trim();
    if (!normalizedTitle) {
      setNotice({
        kind: "error",
        text: "Title is required.",
      });
      return;
    }
    const tags = editForm.tagsText
      .split(",")
      .map((tag) => tag.trim())
      .filter(Boolean);
    try {
      setSavingEdit(true);
      await invoke("smartbrain_update_knowledge", {
        docId: editingDocId,
        title: normalizedTitle,
        description: editForm.description.trim(),
        tags,
        domain: editForm.domain.trim(),
        sourceGroup: editForm.sourceGroup.trim(),
      });
      await loadKnowledge();
      if (expandedKnow === editingDocId) {
        const result = await invoke<{ content: string; frontmatter?: OkfFrontmatter | null }>(
          "smartbrain_read_knowledge",
          { docId: editingDocId },
        );
        setKnowContent((prev) => ({ ...prev, [editingDocId]: result.content }));
        setKnowFrontmatter((prev) => ({ ...prev, [editingDocId]: result.frontmatter ?? null }));
      }
      setEditingDocId(null);
      setNotice({
        kind: "success",
        text: "Knowledge metadata updated.",
      });
    } catch (err) {
      console.error("Update knowledge metadata failed:", err);
      setNotice({
        kind: "error",
        text: "Failed to update knowledge metadata.",
      });
    } finally {
      setSavingEdit(false);
    }
  }, [editForm, editingDocId, expandedKnow, loadKnowledge, savingEdit]);

  const handleExpandKnowledge = useCallback(async (docId: string) => {
    if (expandedKnow === docId) {
      setExpandedKnow(null);
      return;
    }
    if (!knowContent[docId]) {
      try {
        const result = await invoke<{ content: string; frontmatter?: OkfFrontmatter | null }>("smartbrain_read_knowledge", { docId });
        setKnowContent((prev) => ({ ...prev, [docId]: result.content }));
        if (result.frontmatter) {
          setKnowFrontmatter((prev) => ({ ...prev, [docId]: result.frontmatter ?? null }));
        }
      } catch { /* ignore */ }
    }
    setExpandedKnow(docId);
  }, [expandedKnow, knowContent]);

  const handleMigrateToOkf = useCallback(async () => {
    if (migrating) return;
    setMigrating(true);
    try {
      await invoke("smartbrain_migrate_to_okf");
      await loadKnowledge();
    } catch (err) {
      console.error("Migration failed:", err);
    } finally {
      setMigrating(false);
    }
  }, [migrating, loadKnowledge]);

  const handleUploadKnowledge = useCallback(async () => {
    if (uploading) return;
    try {
      setNotice(null);
      const selected = await open({
        multiple: false,
        filters: [
          { name: "Documents", extensions: ["pdf", "docx", "xlsx", "xls", "md", "txt", "json", "yaml", "yml", "toml", "csv", "html", "xml"] },
        ],
      });
      if (!selected) return;
      const filePath = selected as string;
      if (!filePath) return;

      setUploading(true);
      await invoke("smartbrain_upload_knowledge", { filePath });
      await loadKnowledge();
      setNotice({
        kind: "success",
        text: intl.formatMessage({ id: "settings.smartbrain.knowledge.singleUploadSuccess" }),
      });
    } catch (err) {
      console.error("Upload knowledge failed:", err);
      setNotice({
        kind: "error",
        text: intl.formatMessage({ id: "settings.smartbrain.knowledge.singleUploadFailed" }),
      });
    } finally {
      setUploading(false);
    }
  }, [intl, loadKnowledge, uploading]);

  const handleUploadKnowledgeFolder = useCallback(async () => {
    if (uploadingFolder) return;
    try {
      setNotice(null);
      const selected = await open({ directory: true, multiple: false });
      if (!selected || typeof selected !== "string") return;

      setUploadingFolder(true);
      let response = await invoke<FolderUploadResponse>("smartbrain_upload_knowledge_folder", {
        folderPath: selected,
        recursive: true,
        allowedExtensions: KNOWLEDGE_ALLOWED_EXTENSIONS,
        confirmed: false,
      });

      if (response.requires_confirmation) {
        const preview = (response.preview_paths ?? []).slice(0, 6);
        const previewText = preview.length > 0 ? `\n- ${preview.join("\n- ")}` : "";
        const shouldContinue = confirm(
          intl.formatMessage(
            { id: "settings.smartbrain.knowledge.folderConfirm" },
            {
              count: response.candidate_count ?? 0,
              threshold: response.threshold ?? 0,
              preview: previewText,
            },
          ),
        );
        if (!shouldContinue) {
          setNotice({
            kind: "info",
            text: intl.formatMessage({ id: "settings.smartbrain.knowledge.folderCancelled" }),
          });
          return;
        }

        response = await invoke<FolderUploadResponse>("smartbrain_upload_knowledge_folder", {
          folderPath: selected,
          recursive: true,
          allowedExtensions: KNOWLEDGE_ALLOWED_EXTENSIONS,
          confirmed: true,
        });
      }

      if (response.status !== "ok") {
        setNotice({
          kind: "error",
          text: intl.formatMessage({ id: "settings.smartbrain.knowledge.folderUploadFailed" }),
        });
        return;
      }

      await loadKnowledge();
      setNotice({
        kind: (response.failed_count ?? 0) > 0 ? "warning" : "success",
        text: intl.formatMessage(
          { id: "settings.smartbrain.knowledge.folderResult" },
          {
            imported: response.imported_count ?? 0,
            skipped: response.skipped_count ?? 0,
            failed: response.failed_count ?? 0,
          },
        ),
      });
    } catch (err) {
      console.error("Upload folder failed:", err);
      setNotice({
        kind: "error",
        text: intl.formatMessage({ id: "settings.smartbrain.knowledge.folderUploadFailed" }),
      });
    } finally {
      setUploadingFolder(false);
    }
  }, [intl, loadKnowledge, uploadingFolder]);

  return (
    <div className="space-y-5">
      <div className="flex items-center gap-2 rounded-md border border-[var(--border-muted)] bg-[var(--bg-subtle)] px-3 py-2">
        <IconAlertCircle size={14} stroke={1.5} className="shrink-0 text-[var(--text-faint)]" />
        <p className="text-xs text-[var(--text-faint)]">
          {intl.formatMessage({ id: "settings.smartbrain.toggle.dialogOnly" })}
        </p>
      </div>
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
            <div className="flex items-center gap-2">
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
              <button
                onClick={handleUploadKnowledgeFolder}
                disabled={uploadingFolder}
                className="app-button-secondary flex items-center gap-1.5 text-xs"
              >
                <IconFolder size={13} stroke={1.8} />
                {uploadingFolder
                  ? intl.formatMessage({ id: "settings.smartbrain.knowledge.uploadingFolder" })
                  : intl.formatMessage({ id: "settings.smartbrain.knowledge.uploadFolder" })}
              </button>
              <button
                onClick={handleMigrateToOkf}
                disabled={migrating}
                className="app-button-secondary flex items-center gap-1.5 text-xs"
                title="Migrate existing documents to OKF format"
              >
                <IconRefresh size={13} stroke={1.8} />
                {migrating ? "Migrating..." : "Migrate to OKF"}
              </button>
            </div>
            {notice && (
              <p
                className={`rounded px-2 py-1 text-xs ${
                  notice.kind === "success"
                    ? "bg-green-500/10 text-green-400"
                    : notice.kind === "warning"
                      ? "bg-amber-500/10 text-amber-400"
                      : notice.kind === "info"
                        ? "bg-blue-500/10 text-blue-400"
                        : "bg-red-500/10 text-red-400"
                }`}
              >
                {notice.text}
              </p>
            )}

            {knowledge.length === 0 ? (
              <p className="py-4 text-center text-xs text-[var(--text-faint)]">
                {intl.formatMessage({ id: "settings.smartbrain.knowledge.empty" })}
              </p>
            ) : (
              <>
                {pagedKnowledge.map((doc) => (
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
                          if (editingDocId === doc.doc_id) {
                            handleCancelEditKnowledge();
                          } else {
                            void handleStartEditKnowledge(doc);
                          }
                        }}
                        className="icon-button shrink-0 opacity-50 hover:opacity-100"
                        title={editingDocId === doc.doc_id ? "Cancel edit" : "Edit metadata"}
                      >
                        {editingDocId === doc.doc_id ? (
                          <IconX size={13} stroke={1.6} />
                        ) : (
                          <IconPencil size={13} stroke={1.6} />
                        )}
                      </button>
                      <button
                        onClick={(e) => {
                          e.stopPropagation();
                          if (editingDocId !== doc.doc_id) {
                            handleDeleteKnowledge(doc.doc_id);
                          }
                        }}
                        className="icon-button shrink-0 opacity-50 hover:opacity-100 disabled:opacity-30"
                        disabled={editingDocId === doc.doc_id}
                      >
                        <IconTrash size={13} stroke={1.6} />
                      </button>
                    </div>
                    {editingDocId === doc.doc_id && (
                      <div
                        className="space-y-2 border-t border-[var(--border-subtle)] px-3 py-2"
                        onClick={(e) => e.stopPropagation()}
                      >
                        <div className="grid gap-2 md:grid-cols-2">
                          <input
                            value={editForm.title}
                            onChange={(e) =>
                              setEditForm((prev) => ({ ...prev, title: e.target.value }))
                            }
                            placeholder="Title"
                            className="rounded border border-[var(--border-subtle)] bg-[var(--surface-soft)] px-2 py-1.5 text-xs text-[var(--text-strong)]"
                          />
                          <input
                            value={editForm.tagsText}
                            onChange={(e) =>
                              setEditForm((prev) => ({ ...prev, tagsText: e.target.value }))
                            }
                            placeholder="Tags (comma separated)"
                            className="rounded border border-[var(--border-subtle)] bg-[var(--surface-soft)] px-2 py-1.5 text-xs text-[var(--text-strong)]"
                          />
                          <input
                            value={editForm.domain}
                            onChange={(e) =>
                              setEditForm((prev) => ({ ...prev, domain: e.target.value }))
                            }
                            placeholder="Domain"
                            className="rounded border border-[var(--border-subtle)] bg-[var(--surface-soft)] px-2 py-1.5 text-xs text-[var(--text-strong)]"
                          />
                          <input
                            value={editForm.sourceGroup}
                            onChange={(e) =>
                              setEditForm((prev) => ({ ...prev, sourceGroup: e.target.value }))
                            }
                            placeholder="Source group"
                            className="rounded border border-[var(--border-subtle)] bg-[var(--surface-soft)] px-2 py-1.5 text-xs text-[var(--text-strong)]"
                          />
                        </div>
                        <textarea
                          value={editForm.description}
                          onChange={(e) =>
                            setEditForm((prev) => ({ ...prev, description: e.target.value }))
                          }
                          placeholder="Description"
                          rows={3}
                          className="w-full rounded border border-[var(--border-subtle)] bg-[var(--surface-soft)] px-2 py-1.5 text-xs text-[var(--text-strong)]"
                        />
                        <div className="flex items-center gap-2">
                          <button
                            onClick={() => void handleSaveEditKnowledge()}
                            disabled={savingEdit}
                            className="app-button-secondary flex items-center gap-1.5 text-xs"
                          >
                            <IconCheck size={13} stroke={1.8} />
                            {savingEdit ? "Saving..." : "Save"}
                          </button>
                          <button
                            onClick={handleCancelEditKnowledge}
                            disabled={savingEdit}
                            className="app-button-secondary flex items-center gap-1.5 text-xs"
                          >
                            <IconX size={13} stroke={1.8} />
                            Cancel
                          </button>
                        </div>
                      </div>
                    )}
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
                      {doc.domain && (
                        <span className="rounded bg-purple-500/10 px-1.5 py-0.5 text-[9px] text-purple-400">
                          {intl.formatMessage(
                            { id: "settings.smartbrain.knowledge.domainLabel" },
                            { domain: doc.domain },
                          )}
                        </span>
                      )}
                    </div>
                    {expandedKnow === doc.doc_id && knowContent[doc.doc_id] && (
                      <div className="thin-scrollbar max-h-60 overflow-y-auto border-t border-[var(--border-subtle)] px-3 py-2">
                        {knowFrontmatter[doc.doc_id] && (
                          <div className="mb-2 flex flex-wrap items-center gap-2 text-[10px] text-[var(--text-faint)]">
                            <span className="rounded bg-blue-500/10 px-1.5 py-0.5 text-blue-400">
                              OKF: {knowFrontmatter[doc.doc_id]!.type}
                            </span>
                            {knowFrontmatter[doc.doc_id]!.description && (
                              <span className="italic">{knowFrontmatter[doc.doc_id]!.description}</span>
                            )}
                          </div>
                        )}
                        <pre className="whitespace-pre-wrap text-[11px] text-[var(--text-muted)]">
                          {knowContent[doc.doc_id]}
                        </pre>
                      </div>
                    )}
                  </div>
                ))}
                <SettingsPagination
                  page={knowledgePage}
                  onPageChange={setKnowledgePage}
                  pageSize={knowledgePageSize}
                  totalItems={totalKnowledgeItems}
                  totalPages={totalKnowledgePages}
                />
              </>
            )}
          </div>
        )}
      </section>
    </div>
  );
}
