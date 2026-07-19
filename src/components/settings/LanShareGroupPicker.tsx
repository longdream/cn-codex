import type { CollabGroup } from "../../api/lanCollab";

interface LanShareGroupPickerProps {
  groups: CollabGroup[];
  value: string;
  onChange: (groupId: string) => void;
  disabled?: boolean;
  emptyLabel: string;
  placeholder: string;
}

/** 共享时选择目标协作组（必选，默认私有）。 */
export function LanShareGroupPicker({
  groups,
  value,
  onChange,
  disabled,
  emptyLabel,
  placeholder,
}: LanShareGroupPickerProps) {
  if (groups.length === 0) {
    return (
      <p className="text-[11px] text-[var(--warning)]">
        {emptyLabel}
      </p>
    );
  }

  return (
    <select
      value={value}
      onChange={(e) => onChange(e.target.value)}
      disabled={disabled}
      className="w-full rounded-[var(--radius-sm)] border border-[var(--border-subtle)] bg-[var(--surface-panel)] px-2 py-1.5 text-[12px] outline-none focus:border-[var(--accent)] disabled:opacity-50"
    >
      <option value="">{placeholder}</option>
      {groups.map((group) => (
        <option key={group.groupId} value={group.groupId}>
          {group.name}
          {group.isOwner ? " · Owner" : ""}
          {group.inviteCode ? ` · ${group.inviteCode}` : ""}
        </option>
      ))}
    </select>
  );
}

export function groupLabel(
  groups: CollabGroup[],
  groupId?: string | null,
  fallback = "未绑定协作组",
): string {
  if (!groupId) return fallback;
  const found = groups.find((g) => g.groupId === groupId);
  return found ? found.name : groupId;
}
