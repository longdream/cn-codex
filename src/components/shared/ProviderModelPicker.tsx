import { useEffect, useMemo, useRef, useState } from "react";
import { IconChevronDown, IconCpu } from "@tabler/icons-react";

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
  const [open, setOpen] = useState(false);
  const pickerRef = useRef<HTMLDivElement>(null);
  const provider = providers.find((item) => item.id === providerId) ?? null;
  const selectedModel = provider?.models.find((model) => model.id === modelId) ?? null;
  const models = useMemo(
    () => filterProviderModels(provider?.models ?? [], query),
    [provider, query],
  );

  useEffect(() => {
    if (!open) return;

    const closeOnOutsideClick = (event: MouseEvent) => {
      if (!pickerRef.current?.contains(event.target as Node)) {
        setOpen(false);
      }
    };
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setOpen(false);
      }
    };
    document.addEventListener("mousedown", closeOnOutsideClick);
    document.addEventListener("keydown", closeOnEscape);
    return () => {
      document.removeEventListener("mousedown", closeOnOutsideClick);
      document.removeEventListener("keydown", closeOnEscape);
    };
  }, [open]);

  useEffect(() => {
    if (disabled) {
      setOpen(false);
    }
  }, [disabled]);

  useEffect(() => {
    setQuery("");
    setOpen(false);
  }, [providerId]);

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

      <div className="relative space-y-1 text-xs text-[var(--text-muted)]" ref={pickerRef}>
        <span>{modelLabel}</span>
        <button
          type="button"
          disabled={disabled}
          onClick={() => setOpen((value) => !value)}
          aria-expanded={open}
          aria-haspopup="listbox"
          className="flex h-9 w-full items-center gap-2 rounded-md border border-[var(--border-subtle)] bg-[var(--surface-elevated)] px-2 text-left text-[13px] text-[var(--text-base)] outline-none transition-colors hover:border-[var(--accent-border)] focus:border-[var(--accent)] disabled:cursor-not-allowed disabled:opacity-50"
        >
          <IconCpu size={14} stroke={1.8} className="flex-shrink-0 text-[var(--text-faint)]" />
          <span className="min-w-0 flex-1 truncate">
            {selectedModel?.label ?? selectedModel?.id ?? emptyModelsLabel}
          </span>
          <IconChevronDown
            size={14}
            stroke={1.8}
            className={`flex-shrink-0 text-[var(--text-faint)] transition-transform ${open ? "rotate-180" : ""}`}
          />
        </button>
        {open && (
          <div className="absolute z-50 mt-1 w-full rounded-md border border-[var(--border-subtle)] bg-[var(--surface-panel)] p-1 shadow-[var(--shadow-strong)]">
            <input
              autoFocus
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder={modelSearchPlaceholder}
              className="mb-1 h-8 w-full rounded-[var(--radius-sm)] border border-[var(--border-subtle)] bg-[var(--surface-elevated)] px-2 text-[12px] text-[var(--text-base)] outline-none placeholder:text-[var(--text-faint)] focus:border-[var(--accent)]"
            />
            <div className="max-h-44 overflow-y-auto" role="listbox">
              {models.length > 0 ? models.map((model) => (
                <button
                  key={model.id}
                  type="button"
                  onClick={() => {
                    onChange({ providerId, modelId: model.id });
                    setQuery("");
                    setOpen(false);
                  }}
                  className={`flex w-full items-center gap-2 rounded-[var(--radius-sm)] px-2 py-1.5 text-left text-xs hover:bg-[var(--surface-hover)] ${
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
        )}
      </div>
    </div>
  );
}
