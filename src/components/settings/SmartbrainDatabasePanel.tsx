import {
  IconAlertTriangle,
  IconCheck,
  IconDatabase,
  IconDatabaseOff,
  IconEye,
  IconEyeOff,
  IconPlus,
  IconSparkles,
  IconTrash,
} from "@tabler/icons-react";
import { useCallback, useEffect, useState } from "react";
import { useIntl } from "react-intl";
import {
  createEmptySmartbrainDbSource,
  isSourceEffectivelyEnabled,
  loadSmartbrainDbSettings,
  loadSmartbrainDbSources,
  parseSmartbrainConnectionUri,
  saveSmartbrainDbSources,
  sourceHasAnyPermission,
  type SmartbrainDbSource,
  type SmartbrainDbType,
} from "./smartbrainDatabaseState";

function formatSourceSummary(source: SmartbrainDbSource): string {
  if (source.dbType === "sqlite") {
    return source.filePath || source.connectionUri || "-";
  }
  const host = source.host || "-";
  const port = source.port ? `:${source.port}` : "";
  const databaseName = source.databaseName ? `/${source.databaseName}` : "";
  return `${host}${port}${databaseName}`;
}

export function SmartbrainDatabasePanel() {
  const intl = useIntl();
  const [sources, setSources] = useState<SmartbrainDbSource[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [draft, setDraft] = useState<SmartbrainDbSource>(createEmptySmartbrainDbSource());
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [parsing, setParsing] = useState(false);
  const [showPassword, setShowPassword] = useState(false);
  const [notice, setNotice] = useState<{ kind: "success" | "error" | "warning"; text: string } | null>(null);
  const [skipWhenNoPermission, setSkipWhenNoPermission] = useState(true);

  const loadAll = useCallback(async () => {
    setLoading(true);
    try {
      const [loadedSources, loadedSettings] = await Promise.all([
        loadSmartbrainDbSources(),
        loadSmartbrainDbSettings(),
      ]);
      setSources(loadedSources);
      setSkipWhenNoPermission(loadedSettings.skipWhenNoPermission);
      if (loadedSources.length > 0) {
        setSelectedId((prev) => prev && loadedSources.some((item) => item.id === prev) ? prev : loadedSources[0].id);
      } else {
        setSelectedId(null);
        setDraft(createEmptySmartbrainDbSource());
      }
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void loadAll();
  }, [loadAll]);

  useEffect(() => {
    if (!selectedId) {
      return;
    }
    const selected = sources.find((item) => item.id === selectedId);
    if (selected) {
      setDraft(selected);
    }
  }, [selectedId, sources]);

  const handleCreateNew = useCallback(() => {
    setSelectedId(null);
    setDraft(createEmptySmartbrainDbSource());
    setShowPassword(false);
    setNotice(null);
  }, []);

  const handleParse = useCallback(async () => {
    setParsing(true);
    try {
      const parsed = await parseSmartbrainConnectionUri(draft.dbType, draft.connectionUri);
      setDraft((prev) => ({
        ...prev,
        ...parsed,
      }));
      setNotice({
        kind: "success",
        text: intl.formatMessage({ id: "settings.smartbrain.database.parseSuccess" }),
      });
    } catch (error) {
      setNotice({
        kind: "error",
        text: typeof error === "string" ? error : (error as Error).message,
      });
    } finally {
      setParsing(false);
    }
  }, [draft.connectionUri, draft.dbType, intl]);

  const handleSave = useCallback(async () => {
    const trimmedUri = draft.connectionUri.trim();
    if (!trimmedUri) {
      setNotice({
        kind: "error",
        text: intl.formatMessage({ id: "settings.smartbrain.database.uriRequired" }),
      });
      return;
    }

    let nextDraft = {
      ...draft,
      connectionUri: trimmedUri,
      updatedAt: Math.floor(Date.now() / 1000),
    };

    try {
      const parsed = await parseSmartbrainConnectionUri(nextDraft.dbType, trimmedUri);
      nextDraft = {
        ...nextDraft,
        ...parsed,
      };
    } catch (error) {
      setNotice({
        kind: "error",
        text: typeof error === "string" ? error : (error as Error).message,
      });
      return;
    }

    if (!nextDraft.name.trim()) {
      nextDraft.name =
        nextDraft.dbType === "sqlite"
          ? nextDraft.databaseName || "SQLite"
          : `${nextDraft.dbType}:${nextDraft.host || "database"}`;
    }

    const nextSources = selectedId
      ? sources.map((item) => (item.id === selectedId ? nextDraft : item))
      : [nextDraft, ...sources];

    setSaving(true);
    try {
      await saveSmartbrainDbSources(nextSources);
      setSources(nextSources);
      setSelectedId(nextDraft.id);
      setDraft(nextDraft);
      setNotice({
        kind: sourceHasAnyPermission(nextDraft) ? "success" : "warning",
        text: sourceHasAnyPermission(nextDraft)
          ? intl.formatMessage({ id: "settings.smartbrain.database.saveSuccess" })
          : intl.formatMessage({ id: "settings.smartbrain.database.noPermissionSkip" }),
      });
    } catch (error) {
      setNotice({
        kind: "error",
        text: intl.formatMessage({ id: "settings.smartbrain.database.saveFailed" }, { error: String(error) }),
      });
    } finally {
      setSaving(false);
    }
  }, [draft, intl, selectedId, sources]);

  const handleDelete = useCallback(async () => {
    if (!selectedId) {
      return;
    }
    if (!confirm(intl.formatMessage({ id: "settings.smartbrain.database.deleteConfirm" }))) {
      return;
    }
    const nextSources = sources.filter((item) => item.id !== selectedId);
    setSaving(true);
    try {
      await saveSmartbrainDbSources(nextSources);
      setSources(nextSources);
      if (nextSources.length > 0) {
        setSelectedId(nextSources[0].id);
      } else {
        setSelectedId(null);
        setDraft(createEmptySmartbrainDbSource());
        setShowPassword(false);
      }
      setNotice({
        kind: "success",
        text: intl.formatMessage({ id: "settings.smartbrain.database.deleteSuccess" }),
      });
    } catch (error) {
      setNotice({
        kind: "error",
        text: intl.formatMessage({ id: "settings.smartbrain.database.saveFailed" }, { error: String(error) }),
      });
    } finally {
      setSaving(false);
    }
  }, [intl, selectedId, sources]);

  const effectiveEnabled = isSourceEffectivelyEnabled(
    draft,
    {
      defaultRowLimit: 0,
      defaultTimeoutSec: 0,
      requireReadonlyReminder: true,
      skipWhenNoPermission,
      denyDdl: true,
      denyDrop: true,
      denyDeleteWithoutWritePermission: true,
      rulesMarkdown: "",
    },
  );

  return (
    <div className="space-y-5">
      <section className="settings-card space-y-3">
        <div className="flex items-start justify-between gap-3">
          <div>
            <h4 className="text-[13px] font-semibold text-[var(--text-strong)]">
              {intl.formatMessage({ id: "settings.smartbrain.database" })}
            </h4>
            <p className="mt-1 text-xs text-[var(--text-faint)]">
              {intl.formatMessage({ id: "settings.smartbrain.database.description" })}
            </p>
          </div>
          <button
            type="button"
            onClick={handleCreateNew}
            className="app-button-secondary flex shrink-0 items-center gap-1.5 text-xs"
          >
            <IconPlus size={13} stroke={1.8} />
            {intl.formatMessage({ id: "settings.smartbrain.database.new" })}
          </button>
        </div>

        <div className="rounded-[var(--radius-md)] border border-amber-500/30 bg-amber-500/8 px-3 py-2 text-xs text-amber-300">
          {intl.formatMessage({ id: "settings.smartbrain.database.readonlyWarning" })}
        </div>

        {notice && (
          <div
            className={`rounded-[var(--radius-sm)] px-3 py-2 text-xs ${
              notice.kind === "success"
                ? "bg-green-500/10 text-green-400"
                : notice.kind === "warning"
                  ? "bg-amber-500/10 text-amber-400"
                  : "bg-red-500/10 text-red-400"
            }`}
          >
            {notice.text}
          </div>
        )}

        <div className="grid gap-4 lg:grid-cols-[240px_minmax(0,1fr)]">
          <div className="space-y-2">
            {loading ? (
              <div className="rounded-[var(--radius-md)] border border-[var(--border-subtle)] px-3 py-4 text-xs text-[var(--text-faint)]">
                {intl.formatMessage({ id: "common.loading" })}
              </div>
            ) : sources.length === 0 ? (
              <div className="rounded-[var(--radius-md)] border border-dashed border-[var(--border-subtle)] px-3 py-4 text-xs text-[var(--text-faint)]">
                {intl.formatMessage({ id: "settings.smartbrain.database.empty" })}
              </div>
            ) : (
              sources.map((source) => {
                const itemEnabled = isSourceEffectivelyEnabled(
                  source,
                  {
                    defaultRowLimit: 0,
                    defaultTimeoutSec: 0,
                    requireReadonlyReminder: true,
                    skipWhenNoPermission,
                    denyDdl: true,
                    denyDrop: true,
                    denyDeleteWithoutWritePermission: true,
                    rulesMarkdown: "",
                  },
                );
                return (
                  <button
                    key={source.id}
                    type="button"
                    onClick={() => setSelectedId(source.id)}
                    className={`w-full rounded-[var(--radius-md)] border px-3 py-2 text-left transition-colors ${
                      selectedId === source.id
                        ? "border-[var(--accent-border)] bg-[var(--accent-soft)]"
                        : "border-[var(--border-subtle)] bg-[var(--surface-soft)] hover:bg-[var(--surface-elevated)]"
                    }`}
                  >
                    <div className="flex items-center gap-2">
                      {itemEnabled ? (
                        <IconDatabase size={14} stroke={1.8} className="text-[var(--accent-strong)]" />
                      ) : (
                        <IconDatabaseOff size={14} stroke={1.8} className="text-[var(--text-faint)]" />
                      )}
                      <span className="min-w-0 flex-1 truncate text-xs font-medium text-[var(--text-strong)]">
                        {source.name || intl.formatMessage({ id: "settings.smartbrain.database.unnamed" })}
                      </span>
                    </div>
                    <div className="mt-1 truncate text-[10px] text-[var(--text-faint)]">
                      {formatSourceSummary(source)}
                    </div>
                  </button>
                );
              })
            )}
          </div>

          <div className="space-y-3 rounded-[var(--radius-md)] border border-[var(--border-subtle)] bg-[var(--surface-soft)]/55 p-3">
            <div className="grid gap-3 md:grid-cols-2">
              <div className="space-y-1">
                <label className="text-[11px] text-[var(--text-faint)]">
                  {intl.formatMessage({ id: "settings.smartbrain.database.type" })}
                </label>
                <select
                  value={draft.dbType}
                  onChange={(event) =>
                    setDraft((prev) => ({ ...prev, dbType: event.target.value as SmartbrainDbType }))
                  }
                  className="app-select w-full"
                >
                  <option value="postgresql">PostgreSQL</option>
                  <option value="mysql">MySQL</option>
                  <option value="sqlite">SQLite</option>
                  <option value="sqlserver">SQL Server</option>
                </select>
              </div>
              <div className="space-y-1">
                <label className="text-[11px] text-[var(--text-faint)]">
                  {intl.formatMessage({ id: "settings.smartbrain.database.name" })}
                </label>
                <input
                  value={draft.name}
                  onChange={(event) => setDraft((prev) => ({ ...prev, name: event.target.value }))}
                  placeholder={intl.formatMessage({ id: "settings.smartbrain.database.namePlaceholder" })}
                  className="app-input w-full"
                />
              </div>
            </div>

            <div className="space-y-1">
              <label className="text-[11px] text-[var(--text-faint)]">
                {intl.formatMessage({ id: "settings.smartbrain.database.connectionUri" })}
              </label>
              <textarea
                value={draft.connectionUri}
                onChange={(event) => setDraft((prev) => ({ ...prev, connectionUri: event.target.value }))}
                rows={3}
                placeholder={intl.formatMessage({ id: "settings.smartbrain.database.connectionPlaceholder" })}
                className="w-full rounded-[var(--radius-md)] border border-[var(--border-subtle)] bg-[var(--surface-main)] px-3 py-2 text-xs text-[var(--text-base)] outline-none transition-colors focus:border-[var(--accent-border)]"
              />
              <div className="flex flex-wrap items-center gap-2">
                <button
                  type="button"
                  onClick={() => void handleParse()}
                  disabled={parsing}
                  className="app-button-secondary flex items-center gap-1.5 text-xs disabled:opacity-50"
                >
                  <IconSparkles size={13} stroke={1.8} />
                  {parsing
                    ? intl.formatMessage({ id: "common.loading" })
                    : intl.formatMessage({ id: "settings.smartbrain.database.parse" })}
                </button>
                <label className="flex items-center gap-2 text-xs text-[var(--text-muted)]">
                  <input
                    type="checkbox"
                    checked={draft.enabled}
                    onChange={(event) => setDraft((prev) => ({ ...prev, enabled: event.target.checked }))}
                    className="accent-[var(--accent)]"
                  />
                  {intl.formatMessage({ id: "settings.smartbrain.database.enabled" })}
                </label>
              </div>
            </div>

            <div className="grid gap-3 md:grid-cols-2">
              <div className="space-y-1">
                <label className="text-[11px] text-[var(--text-faint)]">Host</label>
                <input value={draft.host} readOnly className="app-input w-full opacity-80" />
              </div>
              <div className="space-y-1">
                <label className="text-[11px] text-[var(--text-faint)]">Port</label>
                <input value={draft.port ?? ""} readOnly className="app-input w-full opacity-80" />
              </div>
              <div className="space-y-1">
                <label className="text-[11px] text-[var(--text-faint)]">
                  {intl.formatMessage({ id: "settings.smartbrain.database.databaseName" })}
                </label>
                <input value={draft.databaseName} readOnly className="app-input w-full opacity-80" />
              </div>
              <div className="space-y-1">
                <label className="text-[11px] text-[var(--text-faint)]">
                  {intl.formatMessage({ id: "settings.smartbrain.database.username" })}
                </label>
                <input value={draft.username} readOnly className="app-input w-full opacity-80" />
              </div>
              <div className="space-y-1">
                <label className="text-[11px] text-[var(--text-faint)]">
                  {intl.formatMessage({ id: "settings.smartbrain.database.password" })}
                </label>
                <div className="flex items-center gap-2">
                  <input
                    type={showPassword ? "text" : "password"}
                    value={draft.password}
                    onChange={(event) => setDraft((prev) => ({ ...prev, password: event.target.value }))}
                    placeholder={intl.formatMessage({ id: "settings.smartbrain.database.passwordPlaceholder" })}
                    className="app-input min-w-0 flex-1"
                    autoComplete="new-password"
                  />
                  <button
                    type="button"
                    onClick={() => setShowPassword((prev) => !prev)}
                    className="app-button-secondary flex h-9 w-9 items-center justify-center px-0"
                    title={intl.formatMessage({
                      id: showPassword
                        ? "settings.smartbrain.database.hidePassword"
                        : "settings.smartbrain.database.showPassword",
                    })}
                  >
                    {showPassword ? <IconEyeOff size={14} stroke={1.8} /> : <IconEye size={14} stroke={1.8} />}
                  </button>
                </div>
                <div className="text-[11px] text-[var(--text-faint)]">
                  {intl.formatMessage({ id: "settings.smartbrain.database.passwordHint" })}
                </div>
              </div>
              <div className="space-y-1">
                <label className="text-[11px] text-[var(--text-faint)]">Schema</label>
                <input
                  value={draft.schema}
                  onChange={(event) => setDraft((prev) => ({ ...prev, schema: event.target.value }))}
                  className="app-input w-full"
                />
              </div>
              <div className="space-y-1">
                <label className="text-[11px] text-[var(--text-faint)]">
                  {intl.formatMessage({ id: "settings.smartbrain.database.filePath" })}
                </label>
                <input value={draft.filePath} readOnly className="app-input w-full opacity-80" />
              </div>
            </div>

            <div className="space-y-2 rounded-[var(--radius-md)] border border-[var(--border-subtle)] bg-[var(--surface-main)] px-3 py-3">
              <div className="text-xs font-medium text-[var(--text-strong)]">
                {intl.formatMessage({ id: "settings.smartbrain.database.permissions" })}
              </div>
              <div className="grid gap-2 md:grid-cols-3">
                <label className="flex items-center gap-2 text-xs text-[var(--text-muted)]">
                  <input
                    type="checkbox"
                    checked={draft.permissions.readSchema}
                    onChange={(event) =>
                      setDraft((prev) => ({
                        ...prev,
                        permissions: { ...prev.permissions, readSchema: event.target.checked },
                      }))
                    }
                    className="accent-[var(--accent)]"
                  />
                  {intl.formatMessage({ id: "settings.smartbrain.database.permission.readSchema" })}
                </label>
                <label className="flex items-center gap-2 text-xs text-[var(--text-muted)]">
                  <input
                    type="checkbox"
                    checked={draft.permissions.readData}
                    onChange={(event) =>
                      setDraft((prev) => ({
                        ...prev,
                        permissions: { ...prev.permissions, readData: event.target.checked },
                      }))
                    }
                    className="accent-[var(--accent)]"
                  />
                  {intl.formatMessage({ id: "settings.smartbrain.database.permission.readData" })}
                </label>
                <label className="flex items-center gap-2 text-xs text-[var(--text-muted)]">
                  <input
                    type="checkbox"
                    checked={draft.permissions.writeData}
                    onChange={(event) =>
                      setDraft((prev) => ({
                        ...prev,
                        permissions: { ...prev.permissions, writeData: event.target.checked },
                      }))
                    }
                    className="accent-[var(--accent)]"
                  />
                  {intl.formatMessage({ id: "settings.smartbrain.database.permission.writeData" })}
                </label>
              </div>
              <div className="text-[11px] text-[var(--text-faint)]">
                {intl.formatMessage({ id: "settings.smartbrain.database.permissionHint" })}
              </div>
            </div>

            <div
              className={`rounded-[var(--radius-md)] border px-3 py-2 text-xs ${
                effectiveEnabled
                  ? "border-green-500/25 bg-green-500/8 text-green-400"
                  : "border-amber-500/25 bg-amber-500/8 text-amber-300"
              }`}
            >
              {effectiveEnabled ? (
                <span className="inline-flex items-center gap-1.5">
                  <IconCheck size={13} stroke={1.8} />
                  {intl.formatMessage({ id: "settings.smartbrain.database.activeHint" })}
                </span>
              ) : (
                <span className="inline-flex items-center gap-1.5">
                  <IconAlertTriangle size={13} stroke={1.8} />
                  {intl.formatMessage({ id: "settings.smartbrain.database.noPermissionSkip" })}
                </span>
              )}
            </div>

            <div className="flex flex-wrap items-center gap-2">
              <button
                type="button"
                onClick={() => void handleSave()}
                disabled={saving || parsing}
                className="rounded-lg bg-[var(--accent-strong)] px-4 py-1.5 text-xs font-medium text-white transition-colors hover:opacity-90 disabled:opacity-50"
              >
                {saving
                  ? intl.formatMessage({ id: "settings.smartbrain.database.saving" })
                  : intl.formatMessage({ id: "settings.smartbrain.database.save" })}
              </button>
              <button
                type="button"
                onClick={handleCreateNew}
                className="app-button-secondary text-xs"
              >
                {intl.formatMessage({ id: "settings.smartbrain.database.resetDraft" })}
              </button>
              {selectedId && (
                <button
                  type="button"
                  onClick={() => void handleDelete()}
                  disabled={saving}
                  className="app-button-secondary flex items-center gap-1.5 text-xs text-red-400 disabled:opacity-50"
                >
                  <IconTrash size={13} stroke={1.8} />
                  {intl.formatMessage({ id: "common.delete" })}
                </button>
              )}
            </div>
          </div>
        </div>
      </section>
    </div>
  );
}
