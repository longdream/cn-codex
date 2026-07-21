import {
  IconAlertTriangle,
  IconCheck,
  IconDatabase,
  IconDatabaseOff,
  IconEye,
  IconEyeOff,
  IconPlus,
  IconRefresh,
  IconSparkles,
  IconTrash,
} from "@tabler/icons-react";
import { useCallback, useEffect, useState } from "react";
import { useIntl } from "react-intl";
import {
  applyDatabaseNameToConnectionUri,
  createDefaultPermissionPolicy,
  createDefaultTablePermission,
  createEmptyPermissionRule,
  createEmptySmartbrainDbSource,
  inferDefaultPort,
  isSourceEffectivelyEnabled,
  listSmartbrainDatabases,
  testSmartbrainDatabaseConnection,
  loadSmartbrainDbSettings,
  loadSmartbrainDbSources,
  mergeSmartbrainDbParsedFields,
  parseSmartbrainConnectionUri,
  parseSmartbrainConnectionUriLocally,
  saveSmartbrainDbSources,
  sourceHasAnyPermission,
  type SmartbrainDbPermissionPolicy,
  type SmartbrainDbPermissionRule,
  type SmartbrainDbSource,
  type SmartbrainDbTablePermission,
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

function ensurePermissionPolicy(
  permissions?: SmartbrainDbPermissionPolicy | null,
): SmartbrainDbPermissionPolicy {
  return permissions?.version === 2
    ? {
        ...createDefaultPermissionPolicy(),
        ...permissions,
        defaults: {
          ...createDefaultPermissionPolicy().defaults,
          ...(permissions.defaults ?? {}),
        },
        tables: permissions.tables ?? {},
        rules: Array.isArray(permissions.rules) ? permissions.rules : createDefaultPermissionPolicy().rules,
        aiNotes: permissions.aiNotes ?? "",
      }
    : createDefaultPermissionPolicy();
}

function parseCsvList(value: string): string[] {
  return value
    .split(/[,，\s]+/)
    .map((item) => item.trim())
    .filter(Boolean);
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
  const [databaseOptions, setDatabaseOptions] = useState<string[]>([]);
  const [refreshingDatabases, setRefreshingDatabases] = useState(false);
  const [testingConnection, setTestingConnection] = useState(false);
  const [newTableName, setNewTableName] = useState("");

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
        setSelectedId((prev) =>
          prev && loadedSources.some((item) => item.id === prev) ? prev : loadedSources[0].id,
        );
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
      setDraft({
        ...selected,
        permissions: ensurePermissionPolicy(selected.permissions),
      });
      setDatabaseOptions([]);
    }
  }, [selectedId, sources]);

  const handleCreateNew = useCallback(() => {
    setSelectedId(null);
    setDraft(createEmptySmartbrainDbSource());
    setShowPassword(false);
    setDatabaseOptions([]);
    setNotice(null);
  }, []);

  const handleParse = useCallback(async () => {
    setParsing(true);
    try {
      const parsed = await parseSmartbrainConnectionUri(draft.dbType, draft.connectionUri);
      setDraft((prev) => ({
        ...prev,
        ...mergeSmartbrainDbParsedFields(prev, parsed),
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
    if (!trimmedUri && draft.dbType !== "sqlite") {
      setNotice({
        kind: "error",
        text: intl.formatMessage({ id: "settings.smartbrain.database.uriRequired" }),
      });
      return;
    }

    let nextDraft: SmartbrainDbSource = {
      ...draft,
      connectionUri: trimmedUri,
      host: draft.host.trim(),
      databaseName: draft.databaseName.trim(),
      username: draft.username.trim(),
      password: draft.password,
      filePath: draft.filePath.trim(),
      schema: draft.schema.trim(),
      permissions: ensurePermissionPolicy(draft.permissions),
      updatedAt: Math.floor(Date.now() / 1000),
    };

    if (trimmedUri) {
      try {
        const parsed = await parseSmartbrainConnectionUri(nextDraft.dbType, trimmedUri);
        // Keep user-edited values when present; only fill blanks from parse result.
        nextDraft = {
          ...nextDraft,
          ...mergeSmartbrainDbParsedFields(nextDraft, parsed),
        };
      } catch (error) {
        // If the user already filled structured fields, allow save without parse.
        if (!nextDraft.host && !nextDraft.databaseName && !nextDraft.filePath) {
          setNotice({
            kind: "error",
            text: typeof error === "string" ? error : (error as Error).message,
          });
          return;
        }
      }
    }

    if (!nextDraft.name.trim()) {
      nextDraft.name =
        nextDraft.dbType === "sqlite"
          ? nextDraft.databaseName || nextDraft.filePath || "SQLite"
          : `${nextDraft.dbType}:${nextDraft.host || nextDraft.databaseName || "database"}`;
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
        text: intl.formatMessage(
          { id: "settings.smartbrain.database.saveFailed" },
          { error: String(error) },
        ),
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
        setDatabaseOptions([]);
      }
      setNotice({
        kind: "success",
        text: intl.formatMessage({ id: "settings.smartbrain.database.deleteSuccess" }),
      });
    } catch (error) {
      setNotice({
        kind: "error",
        text: intl.formatMessage(
          { id: "settings.smartbrain.database.saveFailed" },
          { error: String(error) },
        ),
      });
    } finally {
      setSaving(false);
    }
  }, [intl, selectedId, sources]);

  const handleRefreshDatabases = useCallback(async () => {
    let working = { ...draft };

    if (working.connectionUri.trim()) {
      try {
        const parsed = parseSmartbrainConnectionUriLocally(working.dbType, working.connectionUri);
        working = {
          ...working,
          ...mergeSmartbrainDbParsedFields(working, parsed),
        };
        setDraft((prev) => ({
          ...prev,
          ...mergeSmartbrainDbParsedFields(prev, parsed),
        }));
      } catch {
        // Keep manual fields when the connection string cannot be parsed locally.
      }
    }

    if (working.dbType === "sqlite") {
      const path = working.filePath.trim() || working.databaseName.trim() || working.connectionUri.trim();
      if (!path) {
        setNotice({
          kind: "error",
          text: intl.formatMessage({ id: "settings.smartbrain.database.refreshNeedConnection" }),
        });
        return;
      }
      const fileName = path.split(/[\\/]/).filter(Boolean).pop() || path;
      setDatabaseOptions([fileName]);
      setDraft((prev) => ({
        ...prev,
        databaseName: prev.databaseName.trim() || fileName,
        filePath: prev.filePath.trim() || path,
      }));
      setNotice({
        kind: "success",
        text: intl.formatMessage({ id: "settings.smartbrain.database.refreshSuccess" }, { count: 1 }),
      });
      return;
    }

    if (!working.host.trim() && !working.connectionUri.trim()) {
      setNotice({
        kind: "error",
        text: intl.formatMessage({ id: "settings.smartbrain.database.refreshNeedConnection" }),
      });
      return;
    }

    setRefreshingDatabases(true);
    try {
      const databases = await listSmartbrainDatabases({
        dbType: working.dbType,
        host: working.host,
        port: working.port,
        username: working.username,
        password: working.password,
        connectionUri: working.connectionUri,
        databaseName: working.databaseName,
        filePath: working.filePath,
      });
      setDatabaseOptions(databases);

      if (databases.length === 0) {
        setNotice({
          kind: "warning",
          text: intl.formatMessage({ id: "settings.smartbrain.database.refreshEmpty" }),
        });
        return;
      }

      setNotice({
        kind: "success",
        text: intl.formatMessage(
          { id: "settings.smartbrain.database.refreshSuccess" },
          { count: databases.length },
        ),
      });

      const currentName = working.databaseName.trim();
      if (!currentName || !databases.includes(currentName)) {
        const nextName = databases[0];
        setDraft((prev) => ({
          ...prev,
          databaseName: nextName,
          connectionUri: applyDatabaseNameToConnectionUri(prev.dbType, prev.connectionUri, nextName),
        }));
      }
    } catch (error) {
      setNotice({
        kind: "error",
        text: intl.formatMessage(
          { id: "settings.smartbrain.database.refreshFailed" },
          { error: typeof error === "string" ? error : (error as Error).message },
        ),
      });
    } finally {
      setRefreshingDatabases(false);
    }
  }, [draft, intl]);

  const handleTestConnection = useCallback(async () => {
    let working = { ...draft };

    if (working.connectionUri.trim()) {
      try {
        const parsed = parseSmartbrainConnectionUriLocally(working.dbType, working.connectionUri);
        working = {
          ...working,
          ...mergeSmartbrainDbParsedFields(working, parsed),
        };
        setDraft((prev) => ({
          ...prev,
          ...mergeSmartbrainDbParsedFields(prev, parsed),
        }));
      } catch {
        // Keep manual fields when the connection string cannot be parsed locally.
      }
    }

    if (working.dbType === "sqlite") {
      const path =
        working.filePath.trim() || working.databaseName.trim() || working.connectionUri.trim();
      if (!path) {
        setNotice({
          kind: "error",
          text: intl.formatMessage({ id: "settings.smartbrain.database.testNeedConnection" }),
        });
        return;
      }
    } else if (!working.host.trim() && !working.connectionUri.trim()) {
      setNotice({
        kind: "error",
        text: intl.formatMessage({ id: "settings.smartbrain.database.testNeedConnection" }),
      });
      return;
    } else if (!working.databaseName.trim() && !working.connectionUri.trim()) {
      setNotice({
        kind: "error",
        text: intl.formatMessage({ id: "settings.smartbrain.database.testNeedDatabase" }),
      });
      return;
    }

    setTestingConnection(true);
    try {
      const result = await testSmartbrainDatabaseConnection({
        dbType: working.dbType,
        host: working.host,
        port: working.port,
        username: working.username,
        password: working.password,
        connectionUri: working.connectionUri,
        databaseName: working.databaseName,
        filePath: working.filePath,
        timeoutSec: 10,
      });
      setNotice({
        kind: result.ok ? "success" : "error",
        text: result.ok
          ? intl.formatMessage(
              { id: "settings.smartbrain.database.testSuccess" },
              { message: result.message },
            )
          : intl.formatMessage(
              { id: "settings.smartbrain.database.testFailed" },
              { error: result.message },
            ),
      });
    } catch (error) {
      setNotice({
        kind: "error",
        text: intl.formatMessage(
          { id: "settings.smartbrain.database.testFailed" },
          { error: typeof error === "string" ? error : (error as Error).message },
        ),
      });
    } finally {
      setTestingConnection(false);
    }
  }, [draft, intl]);

  const updatePermissions = useCallback(
    (updater: (prev: SmartbrainDbPermissionPolicy) => SmartbrainDbPermissionPolicy) => {
      setDraft((prev) => {
        const current = ensurePermissionPolicy(prev.permissions);
        return {
          ...prev,
          permissions: updater(current),
        };
      });
    },
    [],
  );

  const updateDefaultPermission = useCallback(
    (key: keyof SmartbrainDbPermissionPolicy["defaults"], checked: boolean) => {
      updatePermissions((prev) => ({
        ...prev,
        defaults: {
          ...prev.defaults,
          [key]: checked,
        },
      }));
    },
    [updatePermissions],
  );

  const handleAddTablePermission = useCallback(() => {
    const name = newTableName.trim();
    if (!name) {
      return;
    }
    updatePermissions((prev) => {
      if (prev.tables[name]) {
        return prev;
      }
      return {
        ...prev,
        tables: {
          ...prev.tables,
          [name]: createDefaultTablePermission(),
        },
      };
    });
    setNewTableName("");
  }, [newTableName, updatePermissions]);

  const updateTablePermission = useCallback(
    (tableName: string, key: keyof SmartbrainDbTablePermission, checked: boolean) => {
      updatePermissions((prev) => {
        const current = prev.tables[tableName] ?? createDefaultTablePermission();
        return {
          ...prev,
          tables: {
            ...prev.tables,
            [tableName]: {
              ...current,
              [key]: checked,
            },
          },
        };
      });
    },
    [updatePermissions],
  );

  const removeTablePermission = useCallback(
    (tableName: string) => {
      updatePermissions((prev) => {
        const nextTables = { ...prev.tables };
        delete nextTables[tableName];
        return {
          ...prev,
          tables: nextTables,
        };
      });
    },
    [updatePermissions],
  );

  const updateRule = useCallback(
    (index: number, patch: Partial<SmartbrainDbPermissionRule>) => {
      updatePermissions((prev) => {
        const rules = prev.rules.map((rule, ruleIndex) =>
          ruleIndex === index
            ? {
                ...rule,
                ...patch,
                match: {
                  ...rule.match,
                  ...(patch.match ?? {}),
                },
              }
            : rule,
        );
        return {
          ...prev,
          rules,
        };
      });
    },
    [updatePermissions],
  );

  const addRule = useCallback(() => {
    updatePermissions((prev) => ({
      ...prev,
      rules: [...prev.rules, createEmptyPermissionRule()],
    }));
  }, [updatePermissions]);

  const removeRule = useCallback(
    (index: number) => {
      updatePermissions((prev) => ({
        ...prev,
        rules: prev.rules.filter((_, ruleIndex) => ruleIndex !== index),
      }));
    },
    [updatePermissions],
  );

  const effectiveEnabled = isSourceEffectivelyEnabled(draft, {
    defaultRowLimit: 0,
    defaultTimeoutSec: 0,
    requireReadonlyReminder: true,
    skipWhenNoPermission,
    denyDdl: true,
    denyDrop: true,
    denyDeleteWithoutWritePermission: true,
    rulesMarkdown: "",
  });

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
            className={`rounded-[var(--radius-md)] border px-3 py-2 text-xs ${
              notice.kind === "success"
                ? "border-green-500/25 bg-green-500/10 text-green-400"
                : notice.kind === "warning"
                  ? "border-amber-500/25 bg-amber-500/10 text-amber-400"
                  : "border-red-500/25 bg-red-500/10 text-red-400"
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
                const itemEnabled = isSourceEffectivelyEnabled(source, {
                  defaultRowLimit: 0,
                  defaultTimeoutSec: 0,
                  requireReadonlyReminder: true,
                  skipWhenNoPermission,
                  denyDdl: true,
                  denyDrop: true,
                  denyDeleteWithoutWritePermission: true,
                  rulesMarkdown: "",
                });
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
                    setDraft((prev) => ({
                      ...prev,
                      dbType: event.target.value as SmartbrainDbType,
                      port: inferDefaultPort(event.target.value as SmartbrainDbType),
                    }))
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
                <input
                  value={draft.host}
                  onChange={(event) => setDraft((prev) => ({ ...prev, host: event.target.value }))}
                  className="app-input w-full"
                />
              </div>
              <div className="space-y-1">
                <label className="text-[11px] text-[var(--text-faint)]">Port</label>
                <input
                  type="number"
                  min={1}
                  value={draft.port ?? ""}
                  onChange={(event) =>
                    setDraft((prev) => ({
                      ...prev,
                      port: event.target.value ? Number(event.target.value) : null,
                    }))
                  }
                  className="app-input w-full"
                />
              </div>

              <div className="space-y-1">
                <label className="text-[11px] text-[var(--text-faint)]">
                  {intl.formatMessage({ id: "settings.smartbrain.database.databaseName" })}
                </label>
                <div className="flex items-center gap-2">
                  <input
                    value={draft.databaseName}
                    onChange={(event) =>
                      setDraft((prev) => ({
                        ...prev,
                        databaseName: event.target.value,
                        connectionUri: applyDatabaseNameToConnectionUri(
                          prev.dbType,
                          prev.connectionUri,
                          event.target.value,
                        ),
                      }))
                    }
                    list="smartbrain-database-options"
                    placeholder={intl.formatMessage({
                      id: "settings.smartbrain.database.databaseNamePlaceholder",
                    })}
                    className="app-input min-w-0 flex-1"
                  />
                  <button
                    type="button"
                    onClick={() => void handleRefreshDatabases()}
                    disabled={refreshingDatabases}
                    className="app-button-secondary flex h-9 w-9 items-center justify-center px-0 disabled:opacity-50"
                    title={intl.formatMessage({ id: "settings.smartbrain.database.refreshDatabases" })}
                  >
                    <IconRefresh
                      size={14}
                      stroke={1.8}
                      className={refreshingDatabases ? "animate-spin" : undefined}
                    />
                  </button>
                </div>
                {databaseOptions.length > 0 && (
                  <select
                    value={databaseOptions.includes(draft.databaseName) ? draft.databaseName : ""}
                    onChange={(event) => {
                      const nextName = event.target.value;
                      if (!nextName) {
                        return;
                      }
                      setDraft((prev) => ({
                        ...prev,
                        databaseName: nextName,
                        connectionUri: applyDatabaseNameToConnectionUri(
                          prev.dbType,
                          prev.connectionUri,
                          nextName,
                        ),
                      }));
                    }}
                    className="app-select w-full"
                  >
                    <option value="">
                      {intl.formatMessage({ id: "settings.smartbrain.database.selectDatabase" })}
                    </option>
                    {databaseOptions.map((name) => (
                      <option key={name} value={name}>
                        {name}
                      </option>
                    ))}
                  </select>
                )}
                <datalist id="smartbrain-database-options">
                  {databaseOptions.map((name) => (
                    <option key={name} value={name} />
                  ))}
                </datalist>
              </div>

              <div className="space-y-1">
                <label className="text-[11px] text-[var(--text-faint)]">
                  {intl.formatMessage({ id: "settings.smartbrain.database.username" })}
                </label>
                <input
                  value={draft.username}
                  onChange={(event) => setDraft((prev) => ({ ...prev, username: event.target.value }))}
                  className="app-input w-full"
                />
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
                <input
                  value={draft.filePath}
                  onChange={(event) => setDraft((prev) => ({ ...prev, filePath: event.target.value }))}
                  className="app-input w-full"
                />
              </div>
            </div>

            <div className="space-y-2 rounded-[var(--radius-md)] border border-[var(--border-subtle)] bg-[var(--surface-main)] px-3 py-3">
              <div className="text-xs font-medium text-[var(--text-strong)]">
                {intl.formatMessage({ id: "settings.smartbrain.database.permissions" })}
              </div>
              <div className="text-[11px] text-[var(--text-faint)]">
                {intl.formatMessage({ id: "settings.smartbrain.database.permissionHint" })}
              </div>

              <div className="space-y-2">
                <div className="text-[11px] font-medium text-[var(--text-muted)]">
                  {intl.formatMessage({ id: "settings.smartbrain.database.permission.defaults" })}
                </div>
                <div className="grid gap-2 sm:grid-cols-2 lg:grid-cols-3">
                  {(
                    [
                      ["readSchema", "settings.smartbrain.database.permission.readSchema"],
                      ["readData", "settings.smartbrain.database.permission.readData"],
                      ["allowInsert", "settings.smartbrain.database.permission.allowInsert"],
                      ["allowUpdate", "settings.smartbrain.database.permission.allowUpdate"],
                      ["allowDelete", "settings.smartbrain.database.permission.allowDelete"],
                      ["allowDdl", "settings.smartbrain.database.permission.allowDdl"],
                    ] as const
                  ).map(([key, labelId]) => (
                    <label
                      key={key}
                      className="flex items-center gap-2 text-xs text-[var(--text-muted)]"
                    >
                      <input
                        type="checkbox"
                        checked={draft.permissions.defaults[key]}
                        onChange={(event) => updateDefaultPermission(key, event.target.checked)}
                        className="accent-[var(--accent)]"
                      />
                      {intl.formatMessage({ id: labelId })}
                    </label>
                  ))}
                </div>
              </div>

              <div className="space-y-2 border-t border-[var(--border-subtle)] pt-3">
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <div className="text-[11px] font-medium text-[var(--text-muted)]">
                    {intl.formatMessage({ id: "settings.smartbrain.database.permission.tables" })}
                  </div>
                  <div className="flex items-center gap-2">
                    <input
                      value={newTableName}
                      onChange={(event) => setNewTableName(event.target.value)}
                      onKeyDown={(event) => {
                        if (event.key === "Enter") {
                          event.preventDefault();
                          handleAddTablePermission();
                        }
                      }}
                      placeholder={intl.formatMessage({
                        id: "settings.smartbrain.database.permission.tableNamePlaceholder",
                      })}
                      className="app-input w-40 text-xs"
                    />
                    <button
                      type="button"
                      onClick={handleAddTablePermission}
                      className="app-button-secondary text-xs"
                    >
                      {intl.formatMessage({ id: "settings.smartbrain.database.permission.addTable" })}
                    </button>
                  </div>
                </div>
                <div className="text-[11px] text-[var(--text-faint)]">
                  {intl.formatMessage({ id: "settings.smartbrain.database.permission.tablesHint" })}
                </div>
                {Object.keys(draft.permissions.tables).length === 0 ? (
                  <div className="text-[11px] text-[var(--text-faint)]">
                    {intl.formatMessage({ id: "settings.smartbrain.database.permission.tablesEmpty" })}
                  </div>
                ) : (
                  <div className="space-y-2">
                    {Object.entries(draft.permissions.tables).map(([tableName, tablePerm]) => (
                      <div
                        key={tableName}
                        className="rounded-md border border-[var(--border-subtle)] bg-[var(--bg-subtle)] px-2.5 py-2"
                      >
                        <div className="mb-2 flex items-center justify-between gap-2">
                          <div className="text-xs font-medium text-[var(--text-strong)]">{tableName}</div>
                          <button
                            type="button"
                            onClick={() => removeTablePermission(tableName)}
                            className="text-[11px] text-red-400 hover:text-red-300"
                          >
                            {intl.formatMessage({
                              id: "settings.smartbrain.database.permission.removeTable",
                            })}
                          </button>
                        </div>
                        <div className="grid gap-2 sm:grid-cols-4">
                          {(
                            [
                              ["read", "settings.smartbrain.database.permission.tableRead"],
                              ["insert", "settings.smartbrain.database.permission.tableInsert"],
                              ["update", "settings.smartbrain.database.permission.tableUpdate"],
                              ["delete", "settings.smartbrain.database.permission.tableDelete"],
                            ] as const
                          ).map(([key, labelId]) => (
                            <label
                              key={key}
                              className="flex items-center gap-2 text-[11px] text-[var(--text-muted)]"
                            >
                              <input
                                type="checkbox"
                                checked={tablePerm[key]}
                                onChange={(event) =>
                                  updateTablePermission(tableName, key, event.target.checked)
                                }
                                className="accent-[var(--accent)]"
                              />
                              {intl.formatMessage({ id: labelId })}
                            </label>
                          ))}
                        </div>
                      </div>
                    ))}
                  </div>
                )}
              </div>

              <div className="space-y-2 border-t border-[var(--border-subtle)] pt-3">
                <div className="flex items-center justify-between gap-2">
                  <div className="text-[11px] font-medium text-[var(--text-muted)]">
                    {intl.formatMessage({ id: "settings.smartbrain.database.permission.rules" })}
                  </div>
                  <button type="button" onClick={addRule} className="app-button-secondary text-xs">
                    {intl.formatMessage({ id: "settings.smartbrain.database.permission.addRule" })}
                  </button>
                </div>
                <div className="text-[11px] text-[var(--text-faint)]">
                  {intl.formatMessage({ id: "settings.smartbrain.database.permission.rulesHint" })}
                </div>
                {draft.permissions.rules.length === 0 ? (
                  <div className="text-[11px] text-[var(--text-faint)]">
                    {intl.formatMessage({ id: "settings.smartbrain.database.permission.rulesEmpty" })}
                  </div>
                ) : (
                  <div className="space-y-2">
                    {draft.permissions.rules.map((rule, index) => (
                      <div
                        key={`${rule.id}-${index}`}
                        className="space-y-2 rounded-md border border-[var(--border-subtle)] bg-[var(--bg-subtle)] px-2.5 py-2"
                      >
                        <div className="grid gap-2 md:grid-cols-3">
                          <div className="space-y-1">
                            <label className="text-[11px] text-[var(--text-faint)]">
                              {intl.formatMessage({
                                id: "settings.smartbrain.database.permission.ruleId",
                              })}
                            </label>
                            <input
                              value={rule.id}
                              onChange={(event) => updateRule(index, { id: event.target.value })}
                              className="app-input w-full text-xs"
                            />
                          </div>
                          <div className="space-y-1">
                            <label className="text-[11px] text-[var(--text-faint)]">
                              {intl.formatMessage({
                                id: "settings.smartbrain.database.permission.ruleEffect",
                              })}
                            </label>
                            <select
                              value={rule.effect}
                              onChange={(event) =>
                                updateRule(index, {
                                  effect: event.target.value === "allow" ? "allow" : "deny",
                                })
                              }
                              className="app-input w-full text-xs"
                            >
                              <option value="deny">
                                {intl.formatMessage({
                                  id: "settings.smartbrain.database.permission.effectDeny",
                                })}
                              </option>
                              <option value="allow">
                                {intl.formatMessage({
                                  id: "settings.smartbrain.database.permission.effectAllow",
                                })}
                              </option>
                            </select>
                          </div>
                          <div className="flex items-end justify-end">
                            <button
                              type="button"
                              onClick={() => removeRule(index)}
                              className="text-[11px] text-red-400 hover:text-red-300"
                            >
                              {intl.formatMessage({
                                id: "settings.smartbrain.database.permission.removeRule",
                              })}
                            </button>
                          </div>
                        </div>
                        <div className="grid gap-2 md:grid-cols-2">
                          <div className="space-y-1">
                            <label className="text-[11px] text-[var(--text-faint)]">
                              {intl.formatMessage({
                                id: "settings.smartbrain.database.permission.ruleSqlKinds",
                              })}
                            </label>
                            <input
                              value={rule.match.sqlKinds.join(", ")}
                              onChange={(event) =>
                                updateRule(index, {
                                  match: {
                                    ...rule.match,
                                    sqlKinds: parseCsvList(event.target.value),
                                  },
                                })
                              }
                              placeholder="select, insert, update, delete, ddl"
                              className="app-input w-full text-xs"
                            />
                          </div>
                          <div className="space-y-1">
                            <label className="text-[11px] text-[var(--text-faint)]">
                              {intl.formatMessage({
                                id: "settings.smartbrain.database.permission.ruleTables",
                              })}
                            </label>
                            <input
                              value={rule.match.tables.join(", ")}
                              onChange={(event) =>
                                updateRule(index, {
                                  match: {
                                    ...rule.match,
                                    tables: parseCsvList(event.target.value),
                                  },
                                })
                              }
                              placeholder="contract, party"
                              className="app-input w-full text-xs"
                            />
                          </div>
                        </div>
                        <div className="space-y-1">
                          <label className="text-[11px] text-[var(--text-faint)]">
                            {intl.formatMessage({
                              id: "settings.smartbrain.database.permission.ruleMessage",
                            })}
                          </label>
                          <input
                            value={rule.message}
                            onChange={(event) => updateRule(index, { message: event.target.value })}
                            className="app-input w-full text-xs"
                          />
                        </div>
                      </div>
                    ))}
                  </div>
                )}
              </div>

              <div className="space-y-1 border-t border-[var(--border-subtle)] pt-3">
                <label className="text-[11px] font-medium text-[var(--text-muted)]">
                  {intl.formatMessage({ id: "settings.smartbrain.database.permission.aiNotes" })}
                </label>
                <textarea
                  value={draft.permissions.aiNotes}
                  onChange={(event) =>
                    updatePermissions((prev) => ({
                      ...prev,
                      aiNotes: event.target.value,
                    }))
                  }
                  rows={3}
                  placeholder={intl.formatMessage({
                    id: "settings.smartbrain.database.permission.aiNotesPlaceholder",
                  })}
                  className="app-input w-full resize-y text-xs"
                />
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
                disabled={saving || parsing || refreshingDatabases || testingConnection}
                className="rounded-lg bg-[var(--accent-strong)] px-4 py-1.5 text-xs font-medium text-white transition-colors hover:opacity-90 disabled:opacity-50"
              >
                {saving
                  ? intl.formatMessage({ id: "settings.smartbrain.database.saving" })
                  : intl.formatMessage({ id: "settings.smartbrain.database.save" })}
              </button>
              <button
                type="button"
                onClick={() => void handleTestConnection()}
                disabled={saving || parsing || refreshingDatabases || testingConnection}
                className="app-button-secondary flex items-center gap-1.5 text-xs disabled:opacity-50"
              >
                <IconDatabase size={13} stroke={1.8} />
                {testingConnection
                  ? intl.formatMessage({ id: "settings.smartbrain.database.testing" })
                  : intl.formatMessage({ id: "settings.smartbrain.database.testConnection" })}
              </button>
              <button type="button" onClick={handleCreateNew} className="app-button-secondary text-xs">
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
