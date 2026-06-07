import { useMemo, useState } from "react";
import { useIntl } from "react-intl";

interface SlashCommand {
  name: string;
  description: string;
  action: () => void;
}

interface SlashCommandPanelProps {
  query: string;
  commands: SlashCommand[];
  onSelect: (command: SlashCommand) => void;
  onClose: () => void;
}

export function SlashCommandPanel({
  query,
  commands,
  onSelect,
  onClose: _onClose,
}: SlashCommandPanelProps) {
  const intl = useIntl();
  const [selectedIndex, setSelectedIndex] = useState(0);

  const filtered = useMemo(() => {
    const normalizedQuery = query.toLowerCase().replace("/", "");
    if (!normalizedQuery) {
      return commands;
    }

    return commands.filter(
      (command) =>
        command.name.toLowerCase().includes(normalizedQuery) ||
        command.description.toLowerCase().includes(normalizedQuery),
    );
  }, [commands, query]);

  if (filtered.length === 0) {
    return null;
  }

  return (
    <div className="absolute bottom-full left-0 right-0 mb-2 px-5">
      <div className="mx-auto max-w-3xl overflow-hidden rounded-[var(--radius-lg)] border border-[var(--border-subtle)] bg-[var(--surface-raised)] shadow-[var(--shadow-strong)]">
        <div className="flex items-center justify-between border-b border-[var(--border-subtle)] px-3 py-2">
          <span className="text-[11px] font-medium uppercase tracking-wide text-[var(--text-faint)]">
            {intl.formatMessage({ id: "chat.slashHint" })}
          </span>
          <span className="text-[11px] text-[var(--text-faint)]">{filtered.length}</span>
        </div>

        <div className="p-1">
          {filtered.map((command, index) => (
            <button
              key={command.name}
              onClick={() => onSelect(command)}
              onMouseEnter={() => setSelectedIndex(index)}
              className={`w-full rounded-[var(--radius-md)] px-3 py-2 text-left text-sm transition-colors ${
                index === selectedIndex
                  ? "bg-[var(--accent-soft)] text-[var(--text-strong)]"
                  : "text-[var(--text-muted)] hover:bg-[var(--surface-soft)]"
              }`}
            >
              <div className="flex items-center gap-3">
                <span className="font-mono text-xs text-[var(--text-faint)]">/{command.name}</span>
                <span>{command.description}</span>
              </div>
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}

export function getDefaultSlashCommands(): SlashCommand[] {
  return [
    {
      name: "plan",
      description: "Create an implementation plan",
      action: () => {},
    },
    {
      name: "goal",
      description: "Set a project goal",
      action: () => {},
    },
    {
      name: "model",
      description: "Switch AI model",
      action: () => {},
    },
    {
      name: "clear",
      description: "Clear conversation",
      action: () => {},
    },
    {
      name: "compact",
      description: "Compact conversation context",
      action: () => {},
    },
    {
      name: "help",
      description: "Show available commands",
      action: () => {},
    },
  ];
}
