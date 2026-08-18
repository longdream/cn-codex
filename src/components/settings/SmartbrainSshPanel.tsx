import {
  IconAlertTriangle,
  IconCheck,
  IconEye,
  IconEyeOff,
  IconPlus,
  IconServer,
  IconServerOff,
  IconTrash,
} from "@tabler/icons-react";
import { useCallback, useEffect, useState } from "react";
import { useIntl } from "react-intl";
import {
  createEmptySmartbrainSshSource,
  formatSshTarget,
  loadSmartbrainSshSources,
  normalizeSmartbrainSshSource,
  saveSmartbrainSshSources,
  testSmartbrainSshConnection,
  validateSmartbrainSshSource,
  type SmartbrainSshAuthMethod,
  type SmartbrainSshSource,
  type SmartbrainSshValidationError,
} from "./smartbrainSshState";

function validationMessageId(error: SmartbrainSshValidationError): string {
  switch (error) {
    case "hostRequired":
      return "settings.smartbrain.ssh.hostRequired";
    case "usernameRequired":
      return "settings.smartbrain.ssh.usernameRequired";
    case "passwordRequired":
      return "settings.smartbrain.ssh.passwordRequired";
    case "privateKeyRequired":
      return "settings.smartbrain.ssh.privateKeyRequired";
    case "portInvalid":
      return "settings.smartbrain.ssh.portInvalid";
  }
}

export function SmartbrainSshPanel() {
  const intl = useIntl();
  const [sources, setSources] = useState<SmartbrainSshSource[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [draft, setDraft] = useState<SmartbrainSshSource>(createEmptySmartbrainSshSource());
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [testingConnection, setTestingConnection] = useState(false);
  const [showPassword, setShowPassword] = useState(false);
  const [showPassphrase, setShowPassphrase] = useState(false);
  const [notice, setNotice] = useState<{ kind: "success" | "error" | "warning"; text: string } | null>(
    null,
  );

  const loadAll = useCallback(async () => {
    setLoading(true);
    try {
      const loadedSources = await loadSmartbrainSshSources();
      setSources(loadedSources);
      if (loadedSources.length > 0) {
        setSelectedId((prev) =>
          prev && loadedSources.some((item) => item.id === prev) ? prev : loadedSources[0].id,
        );
      } else {
        setSelectedId(null);
        setDraft(createEmptySmartbrainSshSource());
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
    setDraft(createEmptySmartbrainSshSource());
    setShowPassword(false);
    setShowPassphrase(false);
    setNotice(null);
  }, []);

  const handleSave = useCallback(async () => {
    const validation = validateSmartbrainSshSource(draft);
    if (validation) {
      setNotice({
        kind: "error",
        text: intl.formatMessage({ id: validationMessageId(validation) }),
      });
      return;
    }

    const nextDraft = normalizeSmartbrainSshSource({
      ...draft,
      updatedAt: Math.floor(Date.now() / 1000),
    });
    const nextSources = selectedId
      ? sources.map((item) => (item.id === selectedId ? nextDraft : item))
      : [nextDraft, ...sources];

    setSaving(true);
    try {
      await saveSmartbrainSshSources(nextSources);
      setSources(nextSources);
      setSelectedId(nextDraft.id);
      setDraft(nextDraft);
      setNotice({
        kind: "success",
        text: intl.formatMessage({ id: "settings.smartbrain.ssh.saveSuccess" }),
      });
    } catch (error) {
      setNotice({
        kind: "error",
        text: intl.formatMessage(
          { id: "settings.smartbrain.ssh.saveFailed" },
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
    if (!confirm(intl.formatMessage({ id: "settings.smartbrain.ssh.deleteConfirm" }))) {
      return;
    }
    const nextSources = sources.filter((item) => item.id !== selectedId);
    setSaving(true);
    try {
      await saveSmartbrainSshSources(nextSources);
      setSources(nextSources);
      if (nextSources.length > 0) {
        setSelectedId(nextSources[0].id);
      } else {
        setSelectedId(null);
        setDraft(createEmptySmartbrainSshSource());
        setShowPassword(false);
        setShowPassphrase(false);
      }
      setNotice({
        kind: "success",
        text: intl.formatMessage({ id: "settings.smartbrain.ssh.deleteSuccess" }),
      });
    } catch (error) {
      setNotice({
        kind: "error",
        text: intl.formatMessage(
          { id: "settings.smartbrain.ssh.saveFailed" },
          { error: String(error) },
        ),
      });
    } finally {
      setSaving(false);
    }
  }, [intl, selectedId, sources]);

  const handleTestConnection = useCallback(async () => {
    const validation = validateSmartbrainSshSource(draft);
    if (validation) {
      setNotice({
        kind: "error",
        text: intl.formatMessage({ id: validationMessageId(validation) }),
      });
      return;
    }

    setTestingConnection(true);
    try {
      const result = await testSmartbrainSshConnection({
        host: draft.host,
        port: draft.port,
        username: draft.username,
        authMethod: draft.authMethod,
        password: draft.password,
        privateKey: draft.privateKey,
        privateKeyPath: draft.privateKeyPath,
        passphrase: draft.passphrase,
        timeoutSec: 10,
      });
      setNotice({
        kind: result.ok ? "success" : "error",
        text: result.ok
          ? intl.formatMessage(
              { id: "settings.smartbrain.ssh.testSuccess" },
              { message: result.message },
            )
          : intl.formatMessage(
              { id: "settings.smartbrain.ssh.testFailed" },
              { error: result.message },
            ),
      });
    } catch (error) {
      setNotice({
        kind: "error",
        text: intl.formatMessage(
          { id: "settings.smartbrain.ssh.testFailed" },
          { error: typeof error === "string" ? error : (error as Error).message },
        ),
      });
    } finally {
      setTestingConnection(false);
    }
  }, [draft, intl]);

  const execHint = draft.allowExec
    ? intl.formatMessage({ id: "settings.smartbrain.ssh.execAllowed" })
    : intl.formatMessage({ id: "settings.smartbrain.ssh.execReadonly" });

  return (
    <div className="space-y-5">
      <section className="settings-card space-y-3">
        <div className="flex items-start justify-between gap-3">
          <div>
            <h4 className="text-[13px] font-semibold text-[var(--text-strong)]">
              {intl.formatMessage({ id: "settings.smartbrain.ssh" })}
            </h4>
            <p className="mt-1 text-xs text-[var(--text-faint)]">
              {intl.formatMessage({ id: "settings.smartbrain.ssh.description" })}
            </p>
          </div>
          <button
            type="button"
            onClick={handleCreateNew}
            className="app-button-secondary flex shrink-0 items-center gap-1.5 text-xs"
          >
            <IconPlus size={13} stroke={1.8} />
            {intl.formatMessage({ id: "settings.smartbrain.ssh.new" })}
          </button>
        </div>

        <div className="rounded-[var(--radius-md)] border border-amber-500/30 bg-amber-500/8 px-3 py-2 text-xs text-amber-300">
          {intl.formatMessage({ id: "settings.smartbrain.ssh.warning" })}
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
                {intl.formatMessage({ id: "settings.smartbrain.ssh.empty" })}
              </div>
            ) : (
              sources.map((source) => (
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
                    {source.enabled ? (
                      <IconServer size={14} stroke={1.8} className="text-[var(--accent-strong)]" />
                    ) : (
                      <IconServerOff size={14} stroke={1.8} className="text-[var(--text-faint)]" />
                    )}
                    <span className="min-w-0 flex-1 truncate text-xs font-medium text-[var(--text-strong)]">
                      {source.name || intl.formatMessage({ id: "settings.smartbrain.ssh.unnamed" })}
                    </span>
                  </div>
                  <div className="mt-1 truncate text-[10px] text-[var(--text-faint)]">
                    {formatSshTarget(source)}
                  </div>
                  <div className="mt-1 truncate text-[10px] text-[var(--text-faint)]">
                    {source.enabled
                      ? source.allowExec
                        ? intl.formatMessage({ id: "settings.smartbrain.ssh.badge.exec" })
                        : intl.formatMessage({ id: "settings.smartbrain.ssh.badge.readonly" })
                      : intl.formatMessage({ id: "settings.smartbrain.ssh.badge.disabled" })}
                  </div>
                </button>
              ))
            )}
          </div>

          <div className="space-y-3 rounded-[var(--radius-md)] border border-[var(--border-subtle)] bg-[var(--surface-soft)]/55 p-3">
            <div className="grid gap-3 md:grid-cols-2">
              <div className="space-y-1">
                <label className="text-[11px] text-[var(--text-faint)]">
                  {intl.formatMessage({ id: "settings.smartbrain.ssh.name" })}
                </label>
                <input
                  value={draft.name}
                  onChange={(event) => setDraft((prev) => ({ ...prev, name: event.target.value }))}
                  placeholder={intl.formatMessage({ id: "settings.smartbrain.ssh.namePlaceholder" })}
                  className="app-input w-full"
                />
              </div>
              <div className="space-y-1">
                <label className="text-[11px] text-[var(--text-faint)]">
                  {intl.formatMessage({ id: "settings.smartbrain.ssh.authMethod" })}
                </label>
                <select
                  value={draft.authMethod}
                  onChange={(event) =>
                    setDraft((prev) => ({
                      ...prev,
                      authMethod: event.target.value as SmartbrainSshAuthMethod,
                    }))
                  }
                  className="app-select w-full"
                >
                  <option value="password">
                    {intl.formatMessage({ id: "settings.smartbrain.ssh.auth.password" })}
                  </option>
                  <option value="privateKey">
                    {intl.formatMessage({ id: "settings.smartbrain.ssh.auth.privateKey" })}
                  </option>
                </select>
              </div>
            </div>

            <div className="flex flex-wrap items-center gap-4">
              <label className="flex items-center gap-2 text-xs text-[var(--text-muted)]">
                <input
                  type="checkbox"
                  checked={draft.enabled}
                  onChange={(event) => setDraft((prev) => ({ ...prev, enabled: event.target.checked }))}
                  className="accent-[var(--accent)]"
                />
                {intl.formatMessage({ id: "settings.smartbrain.ssh.enabled" })}
              </label>
              <label className="flex items-center gap-2 text-xs text-[var(--text-muted)]">
                <input
                  type="checkbox"
                  checked={draft.allowExec}
                  onChange={(event) => setDraft((prev) => ({ ...prev, allowExec: event.target.checked }))}
                  className="accent-[var(--accent)]"
                />
                {intl.formatMessage({ id: "settings.smartbrain.ssh.allowExec" })}
              </label>
            </div>

            <div className="grid gap-3 md:grid-cols-2">
              <div className="space-y-1">
                <label className="text-[11px] text-[var(--text-faint)]">
                  {intl.formatMessage({ id: "settings.smartbrain.ssh.host" })}
                </label>
                <input
                  value={draft.host}
                  onChange={(event) => setDraft((prev) => ({ ...prev, host: event.target.value }))}
                  placeholder={intl.formatMessage({ id: "settings.smartbrain.ssh.hostPlaceholder" })}
                  className="app-input w-full"
                />
              </div>
              <div className="space-y-1">
                <label className="text-[11px] text-[var(--text-faint)]">
                  {intl.formatMessage({ id: "settings.smartbrain.ssh.port" })}
                </label>
                <input
                  type="number"
                  min={1}
                  max={65535}
                  value={draft.port ?? ""}
                  onChange={(event) =>
                    setDraft((prev) => ({
                      ...prev,
                      port: event.target.value ? Number(event.target.value) : null,
                    }))
                  }
                  placeholder="22"
                  className="app-input w-full"
                />
              </div>
              <div className="space-y-1 md:col-span-2">
                <label className="text-[11px] text-[var(--text-faint)]">
                  {intl.formatMessage({ id: "settings.smartbrain.ssh.username" })}
                </label>
                <input
                  value={draft.username}
                  onChange={(event) => setDraft((prev) => ({ ...prev, username: event.target.value }))}
                  placeholder={intl.formatMessage({ id: "settings.smartbrain.ssh.usernamePlaceholder" })}
                  className="app-input w-full"
                />
              </div>
            </div>

            {draft.authMethod === "password" ? (
              <div className="space-y-1">
                <label className="text-[11px] text-[var(--text-faint)]">
                  {intl.formatMessage({ id: "settings.smartbrain.ssh.password" })}
                </label>
                <div className="flex items-center gap-2">
                  <input
                    type={showPassword ? "text" : "password"}
                    value={draft.password}
                    onChange={(event) => setDraft((prev) => ({ ...prev, password: event.target.value }))}
                    placeholder={intl.formatMessage({ id: "settings.smartbrain.ssh.passwordPlaceholder" })}
                    className="app-input min-w-0 flex-1"
                    autoComplete="new-password"
                  />
                  <button
                    type="button"
                    onClick={() => setShowPassword((prev) => !prev)}
                    className="app-button-secondary flex h-9 w-9 items-center justify-center px-0"
                    title={intl.formatMessage({
                      id: showPassword
                        ? "settings.smartbrain.ssh.hideSecret"
                        : "settings.smartbrain.ssh.showSecret",
                    })}
                  >
                    {showPassword ? <IconEyeOff size={14} stroke={1.8} /> : <IconEye size={14} stroke={1.8} />}
                  </button>
                </div>
              </div>
            ) : (
              <div className="space-y-3">
                <div className="space-y-1">
                  <label className="text-[11px] text-[var(--text-faint)]">
                    {intl.formatMessage({ id: "settings.smartbrain.ssh.privateKeyPath" })}
                  </label>
                  <input
                    value={draft.privateKeyPath}
                    onChange={(event) =>
                      setDraft((prev) => ({ ...prev, privateKeyPath: event.target.value }))
                    }
                    placeholder={intl.formatMessage({
                      id: "settings.smartbrain.ssh.privateKeyPathPlaceholder",
                    })}
                    className="app-input w-full"
                  />
                </div>
                <div className="space-y-1">
                  <label className="text-[11px] text-[var(--text-faint)]">
                    {intl.formatMessage({ id: "settings.smartbrain.ssh.privateKey" })}
                  </label>
                  <textarea
                    value={draft.privateKey}
                    onChange={(event) => setDraft((prev) => ({ ...prev, privateKey: event.target.value }))}
                    rows={5}
                    placeholder={intl.formatMessage({
                      id: "settings.smartbrain.ssh.privateKeyPlaceholder",
                    })}
                    className="w-full rounded-[var(--radius-md)] border border-[var(--border-subtle)] bg-[var(--surface-main)] px-3 py-2 font-mono text-xs text-[var(--text-base)] outline-none transition-colors focus:border-[var(--accent-border)]"
                    spellCheck={false}
                  />
                </div>
                <div className="space-y-1">
                  <label className="text-[11px] text-[var(--text-faint)]">
                    {intl.formatMessage({ id: "settings.smartbrain.ssh.passphrase" })}
                  </label>
                  <div className="flex items-center gap-2">
                    <input
                      type={showPassphrase ? "text" : "password"}
                      value={draft.passphrase}
                      onChange={(event) =>
                        setDraft((prev) => ({ ...prev, passphrase: event.target.value }))
                      }
                      placeholder={intl.formatMessage({
                        id: "settings.smartbrain.ssh.passphrasePlaceholder",
                      })}
                      className="app-input min-w-0 flex-1"
                      autoComplete="new-password"
                    />
                    <button
                      type="button"
                      onClick={() => setShowPassphrase((prev) => !prev)}
                      className="app-button-secondary flex h-9 w-9 items-center justify-center px-0"
                      title={intl.formatMessage({
                        id: showPassphrase
                          ? "settings.smartbrain.ssh.hideSecret"
                          : "settings.smartbrain.ssh.showSecret",
                      })}
                    >
                      {showPassphrase ? (
                        <IconEyeOff size={14} stroke={1.8} />
                      ) : (
                        <IconEye size={14} stroke={1.8} />
                      )}
                    </button>
                  </div>
                </div>
              </div>
            )}

            <div
              className={`rounded-[var(--radius-md)] border px-3 py-2 text-xs ${
                draft.enabled
                  ? "border-green-500/25 bg-green-500/8 text-green-400"
                  : "border-amber-500/25 bg-amber-500/8 text-amber-300"
              }`}
            >
              {draft.enabled ? (
                <span className="inline-flex items-center gap-1.5">
                  <IconCheck size={13} stroke={1.8} />
                  {execHint}
                </span>
              ) : (
                <span className="inline-flex items-center gap-1.5">
                  <IconAlertTriangle size={13} stroke={1.8} />
                  {intl.formatMessage({ id: "settings.smartbrain.ssh.disabledHint" })}
                </span>
              )}
            </div>

            <div className="flex flex-wrap items-center gap-2">
              <button
                type="button"
                onClick={() => void handleSave()}
                disabled={saving || testingConnection}
                className="rounded-lg bg-[var(--accent-strong)] px-4 py-1.5 text-xs font-medium text-white transition-colors hover:opacity-90 disabled:opacity-50"
              >
                {saving
                  ? intl.formatMessage({ id: "settings.smartbrain.ssh.saving" })
                  : intl.formatMessage({ id: "settings.smartbrain.ssh.save" })}
              </button>
              <button
                type="button"
                onClick={() => void handleTestConnection()}
                disabled={saving || testingConnection}
                className="app-button-secondary flex items-center gap-1.5 text-xs disabled:opacity-50"
              >
                <IconServer size={13} stroke={1.8} />
                {testingConnection
                  ? intl.formatMessage({ id: "settings.smartbrain.ssh.testing" })
                  : intl.formatMessage({ id: "settings.smartbrain.ssh.testConnection" })}
              </button>
              <button type="button" onClick={handleCreateNew} className="app-button-secondary text-xs">
                {intl.formatMessage({ id: "settings.smartbrain.ssh.resetDraft" })}
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
