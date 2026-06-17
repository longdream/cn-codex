import { useMemo, useState } from "react";
import { useIntl } from "react-intl";

export interface SlashCommand {
  name: string;
  description: string;
  action: () => void;
  trigger?: string;
  searchText?: string;
  closeOnSelect?: boolean;
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
    const normalizedQuery = query.trim().toLowerCase();
    if (!normalizedQuery) {
      return commands;
    }

    return commands.filter(
      (command) => {
        const trigger = (command.trigger ?? command.name).toLowerCase();
        const searchText = command.searchText?.toLowerCase() ?? "";
        return (
          trigger.includes(normalizedQuery) ||
          command.description.toLowerCase().includes(normalizedQuery) ||
          searchText.includes(normalizedQuery)
        );
      },
    );
  }, [commands, query]);

  if (filtered.length === 0) {
    return null;
  }

  return (
    <div className="absolute bottom-full left-0 right-0 mb-2 px-4 sm:px-8">
      <div className="mx-auto max-w-[1180px] overflow-hidden rounded-[var(--radius-lg)] border border-[var(--chat-line)] bg-[var(--chat-card-solid)] shadow-[var(--shadow-strong)]">
        <div className="flex items-center justify-between border-b border-[var(--chat-line)] px-3 py-2">
          <span className="text-[11px] font-medium uppercase tracking-wide text-[var(--chat-faint)]">
            {intl.formatMessage({ id: "chat.slashHint" })}
          </span>
          <span className="text-[11px] text-[var(--chat-faint)]">{filtered.length}</span>
        </div>

        <div className="p-1">
          {filtered.map((command, index) => (
            <button
              key={command.name}
              onClick={() => onSelect(command)}
              onMouseEnter={() => setSelectedIndex(index)}
              className={`w-full rounded-[var(--radius-md)] px-3 py-2 text-left text-sm transition-colors ${
                index === selectedIndex
                  ? "bg-[var(--accent-soft)] text-[var(--chat-prose)]"
                  : "text-[var(--chat-muted)] hover:bg-[var(--chat-chip)]"
              }`}
            >
              <div className="flex items-center gap-3">
                <span className="font-mono text-xs text-[var(--chat-faint)]">
                  /{command.trigger ?? command.name}
                </span>
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
      name: "skill",
      description: "Browse and pick a skill",
      action: () => {},
      closeOnSelect: false,
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
    {
      name: "modifyrobot",
      description: "Modify selected robot",
      action: () => {},
    },
  ];
}
