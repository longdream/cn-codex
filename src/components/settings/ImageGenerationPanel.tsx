import { useCallback, useEffect, useMemo, useState } from "react";
import { useIntl } from "react-intl";
import { standaloneConfigRead, standaloneConfigWrite } from "../../api";
import {
  normalizeImageGenerationSettings,
  useAppStore,
  type ImageGenerationSettings,
} from "../../stores/appStore";

function readImageGenerationFromConfig(
  config: Record<string, unknown> | undefined,
): ImageGenerationSettings | null {
  if (!config) {
    return null;
  }
  const raw = config.image_generation;
  if (!raw || typeof raw !== "object") {
    return null;
  }
  const record = raw as Record<string, unknown>;
  return normalizeImageGenerationSettings({
    model: typeof record.model === "string" ? record.model : undefined,
    baseUrl: typeof record.base_url === "string" ? record.base_url : undefined,
    apiKey: typeof record.api_key === "string" ? record.api_key : undefined,
  });
}

export function ImageGenerationPanel() {
  const intl = useIntl();
  const imageSettings = useAppStore((state) => state.imageGenerationSettings);
  const setImageGenerationSettings = useAppStore((state) => state.setImageGenerationSettings);
  const [draft, setDraft] = useState<ImageGenerationSettings>(imageSettings);
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [feedback, setFeedback] = useState<{
    kind: "success" | "error";
    text: string;
  } | null>(null);

  useEffect(() => {
    setDraft(imageSettings);
  }, [imageSettings]);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    standaloneConfigRead()
      .then((result) => {
        if (cancelled) {
          return;
        }
        const fromConfig = readImageGenerationFromConfig(result?.config);
        setImageGenerationSettings(fromConfig ?? normalizeImageGenerationSettings());
      })
      .catch(() => {
        // noop: keep store snapshot as fallback
      })
      .finally(() => {
        if (!cancelled) {
          setLoading(false);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [setImageGenerationSettings]);

  const canSave = useMemo(() => {
    if (saving || loading) {
      return false;
    }
    const normalized = normalizeImageGenerationSettings(draft);
    return (
      normalized.model !== imageSettings.model ||
      normalized.baseUrl !== imageSettings.baseUrl ||
      normalized.apiKey !== imageSettings.apiKey
    );
  }, [draft, imageSettings, loading, saving]);

  const handleSave = useCallback(async () => {
    const normalized = normalizeImageGenerationSettings(draft);
    setSaving(true);
    setFeedback(null);
    try {
      await standaloneConfigWrite([
        {
          keyPath: "image_generation.model",
          value: normalized.model,
          mergeStrategy: "replace",
        },
        {
          keyPath: "image_generation.base_url",
          value: normalized.baseUrl,
          mergeStrategy: "replace",
        },
        {
          keyPath: "image_generation.api_key",
          value: normalized.apiKey || null,
          mergeStrategy: "replace",
        },
      ]);
      setImageGenerationSettings(normalized);
      setFeedback({
        kind: "success",
        text: intl.formatMessage({ id: "settings.image.saveSuccess" }),
      });
    } catch (error) {
      console.error("Save image generation config failed:", error);
      setFeedback({
        kind: "error",
        text: intl.formatMessage({ id: "settings.image.saveError" }),
      });
    } finally {
      setSaving(false);
    }
  }, [draft, intl, setImageGenerationSettings]);

  return (
    <div className="space-y-5">
      <section className="settings-card space-y-3">
        <h4 className="text-[13px] font-semibold text-[var(--text-strong)]">
          {intl.formatMessage({ id: "settings.image" })}
        </h4>
        <p className="text-xs text-[var(--text-faint)]">
          {intl.formatMessage({ id: "settings.image.description" })}
        </p>
        <p className="text-[11px] text-[var(--text-muted)]">
          {intl.formatMessage({ id: "settings.image.priorityHint" })}
        </p>

        <div className="grid gap-3 md:grid-cols-2">
          <div className="space-y-1">
            <label className="text-[11px] text-[var(--text-faint)]">
              {intl.formatMessage({ id: "settings.image.model" })}
            </label>
            <input
              value={draft.model}
              onChange={(event) =>
                setDraft((prev) => ({ ...prev, model: event.target.value }))
              }
              placeholder={intl.formatMessage({ id: "settings.image.modelPlaceholder" })}
              className="app-input w-full"
            />
          </div>
          <div className="space-y-1">
            <label className="text-[11px] text-[var(--text-faint)]">
              {intl.formatMessage({ id: "settings.image.baseUrl" })}
            </label>
            <input
              value={draft.baseUrl}
              onChange={(event) =>
                setDraft((prev) => ({ ...prev, baseUrl: event.target.value }))
              }
              placeholder={intl.formatMessage({ id: "settings.image.baseUrlPlaceholder" })}
              className="app-input w-full"
            />
          </div>
        </div>

        <div className="space-y-1">
          <label className="text-[11px] text-[var(--text-faint)]">
            {intl.formatMessage({ id: "settings.image.apiKey" })}
          </label>
          <input
            value={draft.apiKey}
            onChange={(event) =>
              setDraft((prev) => ({ ...prev, apiKey: event.target.value }))
            }
            placeholder={intl.formatMessage({ id: "settings.image.apiKeyPlaceholder" })}
            className="app-input w-full"
          />
        </div>

        <div className="flex items-center gap-3">
          <button
            type="button"
            onClick={() => void handleSave()}
            disabled={!canSave}
            className="rounded-lg bg-[var(--accent-strong)] px-4 py-1.5 text-xs font-medium text-white transition-colors hover:opacity-90 disabled:opacity-50"
          >
            {saving
              ? intl.formatMessage({ id: "common.saving" })
              : intl.formatMessage({ id: "common.save" })}
          </button>
          {feedback && (
            <span
              className={`text-xs ${
                feedback.kind === "success"
                  ? "text-green-500"
                  : "text-[var(--danger)]"
              }`}
            >
              {feedback.text}
            </span>
          )}
        </div>
      </section>
    </div>
  );
}
