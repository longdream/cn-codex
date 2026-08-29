import { useCallback, useEffect, useState } from "react";
import { useIntl } from "react-intl";
import { IconRefresh } from "@tabler/icons-react";
import { useVoiceStore } from "../../stores/voiceStore";
import { voiceDownloadModel, type VoiceModelStatus } from "../../api/voice";

/**
 * 设置 → 语音（个性设置 + 语音模型）。
 * 个性设置仅作用于闲聊分支，与主链路无关（需求 6）。
 */
export function VoiceSettingsPanel() {
  const intl = useIntl();
  const persona = useVoiceStore((s) => s.persona);
  const updatePersona = useVoiceStore((s) => s.updatePersona);
  const chatHistory = useVoiceStore((s) => s.chatHistory);
  const clearChatHistory = useVoiceStore((s) => s.clearChatHistory);
  const refreshStatuses = useVoiceStore((s) => s.refreshStatuses);
  const [modelStatuses, setModelStatuses] = useState<VoiceModelStatus[]>([]);

  const loadStatuses = useCallback(async () => {
    const { voiceGetModelStatuses } = await import("../../api/voice");
    try {
      setModelStatuses(await voiceGetModelStatuses());
    } catch (err) {
      console.error("[voice] load statuses failed:", err);
    }
  }, []);

  useEffect(() => {
    void loadStatuses();
    const timer = setInterval(() => void loadStatuses(), 3000);
    return () => clearInterval(timer);
  }, [loadStatuses]);

  const handleRedownload = async (id: string) => {
    try {
      await voiceDownloadModel(id);
    } catch (err) {
      console.error("[voice] redownload failed:", err);
    }
    void refreshStatuses();
    void loadStatuses();
  };

  const modelIds: Array<{ id: string; labelId: string }> = [
    { id: "asr", labelId: "voice.settings.model.asr" },
    { id: "tts", labelId: "voice.settings.model.tts" },
    { id: "vad", labelId: "voice.settings.model.vad" },
  ];

  return (
    <div className="space-y-5">
      {/* 个性设置 */}
      <section className="settings-card space-y-4">
        <div>
          <h4 className="text-[13px] font-semibold text-[var(--text-strong)]">
            {intl.formatMessage({ id: "voice.settings.persona" })}
          </h4>
          <p className="mt-1 text-[12px] text-[var(--text-muted)]">
            {intl.formatMessage({ id: "voice.settings.persona.description" })}
          </p>
        </div>

        <div className="grid grid-cols-2 gap-4">
          <label className="space-y-1.5">
            <span className="text-[12px] font-medium text-[var(--text-secondary)]">
              {intl.formatMessage({ id: "voice.settings.personaPreset" })}
            </span>
            <select
              className="app-select w-full"
              value={persona.personaPreset}
              onChange={(e) => updatePersona({ personaPreset: e.target.value })}
            >
              <option value="friendly">{intl.formatMessage({ id: "voice.settings.persona.friendly" })}</option>
              <option value="professional">{intl.formatMessage({ id: "voice.settings.persona.professional" })}</option>
              <option value="humorous">{intl.formatMessage({ id: "voice.settings.persona.humorous" })}</option>
              <option value="concise">{intl.formatMessage({ id: "voice.settings.persona.concise" })}</option>
              <option value="custom">{intl.formatMessage({ id: "voice.settings.persona.custom" })}</option>
            </select>
          </label>
          <label className="space-y-1.5">
            <span className="text-[12px] font-medium text-[var(--text-secondary)]">
              {intl.formatMessage({ id: "voice.settings.languageStyle" })}
            </span>
            <select
              className="app-select w-full"
              value={persona.languageStyle}
              onChange={(e) => updatePersona({ languageStyle: e.target.value })}
            >
              <option value="casual">{intl.formatMessage({ id: "voice.settings.style.casual" })}</option>
              <option value="formal">{intl.formatMessage({ id: "voice.settings.style.formal" })}</option>
              <option value="playful">{intl.formatMessage({ id: "voice.settings.style.playful" })}</option>
              <option value="brief">{intl.formatMessage({ id: "voice.settings.style.brief" })}</option>
            </select>
          </label>
        </div>

        <label className="block space-y-1.5">
          <span className="text-[12px] font-medium text-[var(--text-secondary)]">
            {intl.formatMessage({ id: "voice.settings.customPrompt" })}
          </span>
          <textarea
            className="app-input w-full resize-y"
            rows={2}
            value={persona.customPrompt}
            onChange={(e) => updatePersona({ customPrompt: e.target.value })}
            placeholder={intl.formatMessage({ id: "voice.settings.customPrompt" })}
          />
        </label>

        <div className="grid grid-cols-3 gap-4">
          <label className="space-y-1.5">
            <span className="text-[12px] font-medium text-[var(--text-secondary)]">
              {intl.formatMessage({ id: "voice.settings.chatContextTurns" })}
            </span>
            <input
              type="number"
              min={1}
              max={20}
              className="app-input w-full"
              value={persona.chatContextTurns}
              onChange={(e) =>
                updatePersona({ chatContextTurns: Math.min(20, Math.max(1, Number(e.target.value) || 10)) })
              }
            />
          </label>
          <label className="space-y-1.5">
            <span className="text-[12px] font-medium text-[var(--text-secondary)]">
              {intl.formatMessage({ id: "voice.settings.ttsSid" })}
            </span>
            <input
              type="number"
              min={0}
              className="app-input w-full"
              value={persona.ttsSid}
              onChange={(e) => updatePersona({ ttsSid: Math.max(0, Number(e.target.value) || 0) })}
            />
          </label>
          <label className="space-y-1.5">
            <span className="text-[12px] font-medium text-[var(--text-secondary)]">
              {intl.formatMessage({ id: "voice.settings.ttsSpeed" })} ({persona.ttsSpeed.toFixed(1)}x)
            </span>
            <input
              type="range"
              min={0.8}
              max={1.5}
              step={0.1}
              className="w-full"
              value={persona.ttsSpeed}
              onChange={(e) => updatePersona({ ttsSpeed: Number(e.target.value) })}
            />
          </label>
        </div>

        <div className="flex items-center justify-between">
          <label className="flex items-center gap-2 text-[12px] text-[var(--text-secondary)]">
            <input
              type="checkbox"
              checked={persona.vadEnabled}
              onChange={(e) => updatePersona({ vadEnabled: e.target.checked })}
            />
            {intl.formatMessage({ id: "voice.settings.vad" })}
          </label>
          <button
            className="app-button-ghost text-[12px]"
            onClick={clearChatHistory}
            disabled={chatHistory.length === 0}
          >
            {intl.formatMessage({ id: "voice.settings.clearHistory" })}
          </button>
        </div>
      </section>

      {/* 语音模型 */}
      <section className="settings-card space-y-3">
        <div>
          <h4 className="text-[13px] font-semibold text-[var(--text-strong)]">
            {intl.formatMessage({ id: "voice.settings.models" })}
          </h4>
          <p className="mt-1 text-[12px] text-[var(--text-muted)]">
            {intl.formatMessage({ id: "voice.settings.models.description" })}
          </p>
        </div>
        <div className="space-y-2">
          {modelIds.map(({ id, labelId }) => {
            const model = modelStatuses.find((m) => m.id === id);
            const status = model?.status ?? "not_installed";
            return (
              <div
                key={id}
                className="flex items-center justify-between rounded-md border border-[var(--border-subtle)] px-3 py-2"
              >
                <div className="min-w-0">
                  <p className="text-[12px] font-medium text-[var(--text-strong)]">
                    {intl.formatMessage({ id: labelId })}
                  </p>
                  <p className="truncate text-[11px] text-[var(--text-faint)]">
                    {model?.label ?? id}
                    {model?.totalSize ? ` · ${(model.totalSize / 1024 / 1024).toFixed(0)} MB` : ""}
                  </p>
                </div>
                <div className="flex items-center gap-2">
                  <span
                    className={`voice-model-status is-${status}`}
                    data-testid={`voice-model-status-${id}`}
                  >
                    {intl.formatMessage({ id: `voice.settings.modelStatus.${status}` })}
                  </span>
                  <button
                    className="icon-button"
                    title={intl.formatMessage({ id: "voice.settings.model.redownload" })}
                    onClick={() => void handleRedownload(id)}
                  >
                    <IconRefresh size={13} stroke={1.8} />
                  </button>
                </div>
              </div>
            );
          })}
        </div>
      </section>
    </div>
  );
}
