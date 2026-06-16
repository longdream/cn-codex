import { useCallback, useEffect, useRef } from "react";
import { createPortal } from "react-dom";

export interface ContextMenuItem {
  id: string;
  label: string;
  icon?: React.ReactNode;
  onClick: () => void;
  danger?: boolean;
  disabled?: boolean;
}

export interface ContextMenuDivider {
  id: string;
  divider: true;
}

export type ContextMenuEntry = ContextMenuItem | ContextMenuDivider;

export interface ContextMenuPosition {
  x: number;
  y: number;
}

interface ContextMenuProps {
  items: ContextMenuEntry[];
  position: ContextMenuPosition;
  onClose: () => void;
}

function isDivider(entry: ContextMenuEntry): entry is ContextMenuDivider {
  return "divider" in entry && entry.divider === true;
}

export function ContextMenu({ items, position, onClose }: ContextMenuProps) {
  const menuRef = useRef<HTMLDivElement>(null);

  const handleClickOutside = useCallback(
    (event: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(event.target as Node)) {
        onClose();
      }
    },
    [onClose],
  );

  const handleKeyDown = useCallback(
    (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        onClose();
      }
    },
    [onClose],
  );

  useEffect(() => {
    document.addEventListener("mousedown", handleClickOutside);
    document.addEventListener("keydown", handleKeyDown);
    return () => {
      document.removeEventListener("mousedown", handleClickOutside);
      document.removeEventListener("keydown", handleKeyDown);
    };
  }, [handleClickOutside, handleKeyDown]);

  useEffect(() => {
    const menu = menuRef.current;
    if (!menu) return;
    const rect = menu.getBoundingClientRect();
    const viewportWidth = window.innerWidth;
    const viewportHeight = window.innerHeight;

    let adjustedX = position.x;
    let adjustedY = position.y;

    if (position.x + rect.width > viewportWidth) {
      adjustedX = viewportWidth - rect.width - 8;
    }
    if (position.y + rect.height > viewportHeight) {
      adjustedY = viewportHeight - rect.height - 8;
    }

    menu.style.left = `${Math.max(4, adjustedX)}px`;
    menu.style.top = `${Math.max(4, adjustedY)}px`;
  }, [position]);

  return createPortal(
    <div
      ref={menuRef}
      className="fixed z-[9999] min-w-[160px] rounded-[var(--radius-md)] border border-[var(--chat-line)] bg-[var(--chat-card-solid)] py-1 shadow-lg"
      style={{ left: position.x, top: position.y }}
    >
      {items.map((entry) => {
        if (isDivider(entry)) {
          return (
            <div
              key={entry.id}
              className="my-1 border-t border-[var(--chat-line)]"
            />
          );
        }

        return (
          <button
            key={entry.id}
            type="button"
            disabled={entry.disabled}
            onClick={() => {
              entry.onClick();
              onClose();
            }}
            className={`flex w-full items-center gap-2.5 px-3 py-2 text-left text-sm transition-colors disabled:opacity-40 ${
              entry.danger
                ? "text-[var(--danger)] hover:bg-[var(--danger-soft)]"
                : "text-[var(--text-base)] hover:bg-[var(--surface-elevated)]"
            }`}
          >
            {entry.icon && (
              <span className="flex h-4 w-4 flex-shrink-0 items-center justify-center opacity-70">
                {entry.icon}
              </span>
            )}
            <span>{entry.label}</span>
          </button>
        );
      })}
    </div>,
    document.body,
  );
}
