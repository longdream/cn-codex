import { useMemo, useState } from "react";
import { IconCpu } from "@tabler/icons-react";

import type { ProviderConfig } from "../../types/provider";
import { filterProviderModels } from "../../utils/chatModelSelection";
import { selectProviderDefaultModel } from "../../utils/skillLabRuntime";

interface ProviderModelPickerProps {
  providers: ProviderConfig[];
  providerId: string;
  modelId: string;
  disabled?: boolean;
  providerLabel: string;
  modelLabel: string;
  modelSearchPlaceholder: string;
  emptyModelsLabel: string;
  onChange: (selection: { providerId: string; modelId: string }) => void;
}

export function ProviderModelPicker({
  providers,
  providerId,
  modelId,
  disabled = false,
  providerLabel,
  modelLabel,
  modelSearchPlaceholder,
  emptyModelsLabel,
  onChange,
}: ProviderModelPickerProps) {
  const [query, setQuery] = useState("");
  const provider = providers.find((item) => item.id === providerId) ?? null;
  const models = useMemo(
    () => filterProviderModels(provider?.models ?? [], query),
    [provider, query],
  );

  return (
    <div className="grid gap-2 sm:grid-cols-2">
      <label className="space-y-1 text-xs text-[var(--text-muted)]">
        <span>{providerLabel}</span>
        <select
          value={providerId}
          disabled={disabled}
          onChange={(event) => onChange(
            selectProviderDefaultModel(providers, event.target.value),
          )}
          className="h-9 w-full rounded-md border border-[var(--border-subtle)] bg-[var(--surface-elevated)] px-2 text-[13px] text-[var(--text-base)] outline-none focus:border-[var(--accent)] disabled:opacity-50"
        >
          {providers.map((item) => (
            <option key={item.id} value={item.id}>{item.name}</option>
          ))}
        </select>
      </label>

      <div className="space-y-1 text-xs text-[var(--text-muted)]">
        <span>{modelLabel}</span>
        <input
          value={query}
          disabled={disabled}
          onChange={(event) => setQuery(event.target.value)}
          placeholder={modelSearchPlaceholder}
          className="h-9 w-full rounded-md border border-[var(--border-subtle)] bg-[var(--surface-elevated)] px-2 text-[13px] text-[var(--text-base)] outline-none placeholder:text-[var(--text-faint)] focus:border-[var(--accent)] disabled:opacity-50"
        />
        <div className="max-h-32 overflow-y-auto rounded-md border border-[var(--border-subtle)] bg-[var(--surface-elevated)] py-1">
          {models.length > 0 ? models.map((model) => (
            <button
              key={model.id}
              type="button"
              disabled={disabled}
              onClick={() => onChange({ providerId, modelId: model.id })}
              className={`flex w-full items-center gap-2 px-2 py-1.5 text-left text-xs hover:bg-[var(--surface-hover)] disabled:opacity-50 ${
                model.id === modelId
                  ? "bg-[var(--accent-soft)] text-[var(--accent-strong)]"
                  : "text-[var(--text-base)]"
              }`}
            >
              <IconCpu size={12} stroke={1.8} />
              <span className="min-w-0 flex-1 truncate">{model.label}</span>
              <span className="truncate text-[10px] text-[var(--text-faint)]">{model.id}</span>
            </button>
          )) : (
            <div className="px-2 py-3 text-center text-xs text-[var(--text-faint)]">
              {emptyModelsLabel}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
