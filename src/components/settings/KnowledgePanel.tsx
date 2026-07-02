import { IconChevronDown, IconChevronRight, IconTrash, IconUpload, IconAlertCircle, IconRefresh, IconFolder } from "@tabler/icons-react";
import { useCallback, useEffect, useState } from "react";
import { useIntl } from "react-intl";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";

interface KnowledgeEntry {
  doc_id: string;
  source_file: string;
  source_type: string;
  title: string;
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

  const [enabled, setEnabled] = useState(true);
  const [knowledge, setKnowledge] = useState<KnowledgeEntry[]>([]);
  const [knowExpanded, setKnowExpanded] = useState(true);
  const [expandedKnow, setExpandedKnow] = useState<string | null>(null);
  const [knowContent, setKnowContent] = useState<Record<string, string>>({});
  const [knowFrontmatter, setKnowFrontmatter] = useState<Record<string, OkfFrontmatter | null>>({});
  const [uploading, setUploading] = useState(false);
  const [uploadingFolder, setUploadingFolder] = useState(false);
  const [migrating, setMigrating] = useState(false);
  const [notice, setNotice] = useState<{
    kind: "success" | "error" | "warning" | "info";
    text: string;
  } | null>(null);

  useEffect(() => {
    invoke<{ config?: { smartbrain?: { enabled?: boolean } } }>("standalone_config_read")
      .then((result) => {
        setEnabled(result?.config?.smartbrain?.enabled ?? false);
      })
      .catch(() => {});
  }, []);

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
              ))
            )}
          </div>
        )}
      </section>
    </div>
  );
}
