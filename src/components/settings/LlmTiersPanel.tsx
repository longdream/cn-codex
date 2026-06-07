import { useCallback, useEffect, useState } from "react";
import { useIntl } from "react-intl";
import { llmGetTiers, llmGetUsage, llmSetTier } from "../../api";
import type { LlmTierConfig, LlmTiersConfig, TierLevel, TierUsageStats } from "../../types";

export function LlmTiersPanel() {
  const intl = useIntl();
  const [tiers, setTiers] = useState<LlmTiersConfig | null>(null);
  const [usage, setUsage] = useState<TierUsageStats | null>(null);
  const [editing, setEditing] = useState<TierLevel | null>(null);

  useEffect(() => {
    llmGetTiers().then(setTiers).catch(console.error);
    llmGetUsage().then(setUsage).catch(console.error);
  }, []);

  const handleSave = useCallback(async (level: TierLevel, config: LlmTierConfig) => {
    try {
      await llmSetTier(level, config);
      const updated = await llmGetTiers();
      setTiers(updated);
      setEditing(null);
    } catch (err) {
      console.error("Save tier failed:", err);
    }
  }, []);

  if (!tiers) {
    return (
      <div className="text-sm text-[var(--text-muted)]">
        {intl.formatMessage({ id: "common.loading" })}
      </div>
    );
  }

  const tierEntries: [TierLevel, string][] = [
    ["low", intl.formatMessage({ id: "settings.llmTier.low" })],
    ["medium", intl.formatMessage({ id: "settings.llmTier.medium" })],
    ["high", intl.formatMessage({ id: "settings.llmTier.high" })],
  ];

  return (
    <div className="space-y-5">
      <section className="settings-card space-y-4">
        <div className="space-y-1">
          <h3 className="text-sm font-semibold text-[var(--text-strong)]">
            {intl.formatMessage({ id: "settings.llmTier" })}
          </h3>
          <p className="text-sm text-[var(--text-muted)]">
            {intl.formatMessage({ id: "settings.llmTier.description" })}
          </p>
        </div>

        <div className="space-y-3">
          {tierEntries.map(([level, label]) => {
            const config = tiers.tiers[level];
            if (!config) {
              return null;
            }

            return (
              <div
                key={level}
                className="rounded-2xl border border-[var(--border-subtle)] bg-[var(--surface-contrast)]/82 px-4 py-4"
              >
                <div className="flex items-start justify-between gap-4">
                  <div className="space-y-2">
                    <div className="flex items-center gap-3">
                      <span className="text-sm font-semibold text-[var(--text-strong)]">{label}</span>
                      <span className="rounded-full bg-[var(--surface-soft)] px-2 py-1 text-[11px] text-[var(--text-muted)]">
                        {level}
                      </span>
                    </div>
                    <p className="font-mono text-xs text-[var(--text-muted)]">
                      {[config.model, config.reasoningEffort, config.serviceTier]
                        .filter(Boolean)
                        .join(" / ")}
                    </p>
                  </div>
                  <button
                    onClick={() => setEditing(editing === level ? null : level)}
                    className="secondary-button rounded-full px-3 py-1.5 text-xs"
                  >
                    {editing === level ? intl.formatMessage({ id: "common.close" }) : intl.formatMessage({ id: "common.edit" })}
                  </button>
                </div>

                {editing === level && (
                  <TierEditor
                    config={config}
                    onSave={(nextConfig) => handleSave(level, nextConfig)}
                    onCancel={() => setEditing(null)}
                  />
                )}
              </div>
            );
          })}
        </div>
      </section>

      {usage && (
        <section className="settings-card space-y-4">
          <div className="space-y-1">
            <h3 className="text-sm font-semibold text-[var(--text-strong)]">
              {intl.formatMessage({ id: "settings.tokenUsage" })}
            </h3>
            <p className="text-sm text-[var(--text-muted)]">
              {intl.formatMessage({ id: "settings.tokenUsage.description" })}
            </p>
          </div>

          <div className="grid gap-3 md:grid-cols-4">
            <UsageMetric
              label={intl.formatMessage({ id: "settings.tokenUsage.total" })}
              value={usage.totalTokens}
              emphasize
            />
            <UsageMetric
              label={intl.formatMessage({ id: "settings.tokenUsage.low" })}
              value={usage.lowTokens}
            />
            <UsageMetric
              label={intl.formatMessage({ id: "settings.tokenUsage.medium" })}
              value={usage.mediumTokens}
            />
            <UsageMetric
              label={intl.formatMessage({ id: "settings.tokenUsage.high" })}
              value={usage.highTokens}
            />
          </div>
        </section>
      )}
    </div>
  );
}

function UsageMetric({
  label,
  value,
  emphasize = false,
}: {
  label: string;
  value: number;
  emphasize?: boolean;
}) {
  return (
    <div
      className={`rounded-2xl border px-4 py-4 ${
        emphasize
          ? "border-[var(--accent-border)] bg-[var(--accent-soft)]"
          : "border-[var(--border-subtle)] bg-[var(--surface-contrast)]/82"
      }`}
    >
      <p className="text-xs uppercase tracking-[0.14em] text-[var(--text-faint)]">{label}</p>
      <p className="mt-3 font-mono text-lg font-semibold text-[var(--text-strong)]">
        {value.toLocaleString()}
      </p>
    </div>
  );
}

function TierEditor({
  config,
  onSave,
  onCancel,
}: {
  config: LlmTierConfig;
  onSave: (config: LlmTierConfig) => void;
  onCancel: () => void;
}) {
  const intl = useIntl();
  const [model, setModel] = useState(config.model);
  const [effort, setEffort] = useState(config.reasoningEffort);
  const [serviceTier, setServiceTier] = useState(config.serviceTier);
  const [description, setDescription] = useState(config.description ?? "");

  return (
    <div className="mt-4 space-y-3 border-t border-[var(--border-subtle)] pt-4">
      <input
        value={model}
        onChange={(event) => setModel(event.target.value)}
        placeholder={intl.formatMessage({ id: "settings.llmTier.editor.modelPlaceholder" })}
        className="app-input"
      />

      <div className="grid gap-3 md:grid-cols-2">
        <select
          value={effort}
          onChange={(event) => setEffort(event.target.value as LlmTierConfig["reasoningEffort"])}
          className="app-select"
        >
          <option value="low">
            {intl.formatMessage({ id: "settings.llmTier.editor.reasoning.low" })}
          </option>
          <option value="medium">
            {intl.formatMessage({ id: "settings.llmTier.editor.reasoning.medium" })}
          </option>
          <option value="high">
            {intl.formatMessage({ id: "settings.llmTier.editor.reasoning.high" })}
          </option>
        </select>

        <select
          value={serviceTier}
          onChange={(event) => setServiceTier(event.target.value as LlmTierConfig["serviceTier"])}
          className="app-select"
        >
          <option value="default">
            {intl.formatMessage({ id: "settings.llmTier.editor.serviceTier.default" })}
          </option>
          <option value="priority">
            {intl.formatMessage({ id: "settings.llmTier.editor.serviceTier.priority" })}
          </option>
        </select>
      </div>

      <input
        value={description}
        onChange={(event) => setDescription(event.target.value)}
        placeholder={intl.formatMessage({
          id: "settings.llmTier.editor.descriptionPlaceholder",
        })}
        className="app-input"
      />

      <div className="flex items-center justify-end gap-3">
        <button onClick={onCancel} className="secondary-button rounded-full px-4 py-2 text-sm">
          {intl.formatMessage({ id: "common.cancel" })}
        </button>
        <button
          onClick={() =>
            onSave({
              model,
              reasoningEffort: effort,
              serviceTier,
              description,
            })
          }
          className="primary-button"
        >
          {intl.formatMessage({ id: "common.save" })}
        </button>
      </div>
    </div>
  );
}
