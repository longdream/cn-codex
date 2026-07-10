import { IconShieldCheck } from "@tabler/icons-react";
import { useCallback, useEffect, useState } from "react";
import { useIntl } from "react-intl";
import {
  defaultSmartbrainDbSettings,
  loadSmartbrainDbSettings,
  saveSmartbrainDbSettings,
  type SmartbrainDbSettings,
} from "./smartbrainDatabaseState";

export function SmartbrainDatabaseSettingsPanel() {
  const intl = useIntl();
  const [settings, setSettings] = useState<SmartbrainDbSettings>(defaultSmartbrainDbSettings());
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);

  useEffect(() => {
    loadSmartbrainDbSettings()
      .then((result) => setSettings(result))
      .finally(() => setLoading(false));
  }, []);

  const handleSave = useCallback(async () => {
    setSaving(true);
    setSaved(false);
    try {
      await saveSmartbrainDbSettings({
        ...settings,
        defaultRowLimit: Math.max(1, settings.defaultRowLimit || 200),
        defaultTimeoutSec: Math.max(1, settings.defaultTimeoutSec || 15),
      });
      setSaved(true);
      window.setTimeout(() => setSaved(false), 2000);
    } finally {
      setSaving(false);
    }
  }, [settings]);

  return (
    <div className="space-y-5">
      <section className="settings-card space-y-3">
        <div className="flex items-start gap-3">
          <div className="flex h-9 w-9 items-center justify-center rounded-[var(--radius-md)] bg-[var(--accent-soft)] text-[var(--accent-strong)]">
            <IconShieldCheck size={18} stroke={1.9} />
          </div>
          <div>
            <h4 className="text-[13px] font-semibold text-[var(--text-strong)]">
              {intl.formatMessage({ id: "settings.smartbrain.databaseSettings" })}
            </h4>
            <p className="mt-1 text-xs text-[var(--text-faint)]">
              {intl.formatMessage({ id: "settings.smartbrain.databaseSettings.description" })}
            </p>
          </div>
        </div>

        <div className="grid gap-3 md:grid-cols-2">
          <div className="space-y-1">
            <label className="text-[11px] text-[var(--text-faint)]">
              {intl.formatMessage({ id: "settings.smartbrain.databaseSettings.rowLimit" })}
            </label>
            <input
              type="number"
              min={1}
              value={settings.defaultRowLimit}
              onChange={(event) =>
                setSettings((prev) => ({ ...prev, defaultRowLimit: Number(event.target.value) }))
              }
              disabled={loading}
              className="app-input w-full"
            />
          </div>
          <div className="space-y-1">
            <label className="text-[11px] text-[var(--text-faint)]">
              {intl.formatMessage({ id: "settings.smartbrain.databaseSettings.timeout" })}
            </label>
            <input
              type="number"
              min={1}
              value={settings.defaultTimeoutSec}
              onChange={(event) =>
                setSettings((prev) => ({ ...prev, defaultTimeoutSec: Number(event.target.value) }))
              }
              disabled={loading}
              className="app-input w-full"
            />
          </div>
        </div>

        <div className="grid gap-2">
          <label className="flex items-center gap-2 text-xs text-[var(--text-muted)]">
            <input
              type="checkbox"
              checked={settings.requireReadonlyReminder}
              onChange={(event) =>
                setSettings((prev) => ({ ...prev, requireReadonlyReminder: event.target.checked }))
              }
              className="accent-[var(--accent)]"
            />
            {intl.formatMessage({ id: "settings.smartbrain.databaseSettings.requireReadonlyReminder" })}
          </label>
          <label className="flex items-center gap-2 text-xs text-[var(--text-muted)]">
            <input
              type="checkbox"
              checked={settings.skipWhenNoPermission}
              onChange={(event) =>
                setSettings((prev) => ({ ...prev, skipWhenNoPermission: event.target.checked }))
              }
              className="accent-[var(--accent)]"
            />
            {intl.formatMessage({ id: "settings.smartbrain.databaseSettings.skipNoPermission" })}
          </label>
          <label className="flex items-center gap-2 text-xs text-[var(--text-muted)]">
            <input
              type="checkbox"
              checked={settings.denyDdl}
              onChange={(event) =>
                setSettings((prev) => ({ ...prev, denyDdl: event.target.checked }))
              }
              className="accent-[var(--accent)]"
            />
            {intl.formatMessage({ id: "settings.smartbrain.databaseSettings.denyDdl" })}
          </label>
          <label className="flex items-center gap-2 text-xs text-[var(--text-muted)]">
            <input
              type="checkbox"
              checked={settings.denyDrop}
              onChange={(event) =>
                setSettings((prev) => ({ ...prev, denyDrop: event.target.checked }))
              }
              className="accent-[var(--accent)]"
            />
            {intl.formatMessage({ id: "settings.smartbrain.databaseSettings.denyDrop" })}
          </label>
          <label className="flex items-center gap-2 text-xs text-[var(--text-muted)]">
            <input
              type="checkbox"
              checked={settings.denyDeleteWithoutWritePermission}
              onChange={(event) =>
                setSettings((prev) => ({
                  ...prev,
                  denyDeleteWithoutWritePermission: event.target.checked,
                }))
              }
              className="accent-[var(--accent)]"
            />
            {intl.formatMessage({ id: "settings.smartbrain.databaseSettings.denyDeleteWithoutWritePermission" })}
          </label>
        </div>

        <div className="space-y-1">
          <label className="text-[11px] text-[var(--text-faint)]">
            {intl.formatMessage({ id: "settings.smartbrain.databaseSettings.rules" })}
          </label>
          <textarea
            value={settings.rulesMarkdown}
            onChange={(event) =>
              setSettings((prev) => ({ ...prev, rulesMarkdown: event.target.value }))
            }
            rows={12}
            disabled={loading}
            className="w-full rounded-xl border border-[var(--border-subtle)] bg-[var(--surface-main)] p-3 font-mono text-xs text-[var(--text-base)] placeholder:text-[var(--text-faint)] focus:border-[var(--accent-strong)] focus:outline-none resize-y"
          />
          <p className="text-[11px] text-[var(--text-faint)]">
            {intl.formatMessage({ id: "settings.smartbrain.databaseSettings.rulesHint" })}
          </p>
        </div>

        <div className="flex items-center gap-3">
          <button
            type="button"
            onClick={() => void handleSave()}
            disabled={saving || loading}
            className="rounded-lg bg-[var(--accent-strong)] px-4 py-1.5 text-xs font-medium text-white transition-colors hover:opacity-90 disabled:opacity-50"
          >
            {saving
              ? intl.formatMessage({ id: "settings.rules.saving" })
              : intl.formatMessage({ id: "settings.rules.save" })}
          </button>
          {saved && (
            <span className="text-xs text-green-500">
              {intl.formatMessage({ id: "settings.rules.saved" })}
            </span>
          )}
        </div>
      </section>
    </div>
  );
}
