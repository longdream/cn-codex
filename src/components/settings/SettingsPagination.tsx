import { IconChevronLeft, IconChevronRight } from "@tabler/icons-react";
import { useEffect, useMemo, useState } from "react";
import { useIntl } from "react-intl";

export const DEFAULT_SETTINGS_PAGE_SIZE = 10;

export function usePagedItems<T>(
  items: T[],
  pageSize = DEFAULT_SETTINGS_PAGE_SIZE,
): {
  page: number;
  setPage: (value: number | ((prev: number) => number)) => void;
  pageSize: number;
  totalItems: number;
  totalPages: number;
  pagedItems: T[];
} {
  const [page, setPage] = useState(0);
  const totalItems = items.length;
  const totalPages = Math.max(1, Math.ceil(totalItems / pageSize));

  useEffect(() => {
    setPage((prev) => Math.min(prev, totalPages - 1));
  }, [totalPages]);

  const pagedItems = useMemo(() => {
    const start = page * pageSize;
    return items.slice(start, start + pageSize);
  }, [items, page, pageSize]);

  return {
    page,
    setPage,
    pageSize,
    totalItems,
    totalPages,
    pagedItems,
  };
}

interface SettingsPaginationProps {
  page: number;
  totalPages: number;
  totalItems: number;
  pageSize: number;
  onPageChange: (page: number) => void;
  className?: string;
}

export function SettingsPagination({
  page,
  totalPages,
  totalItems,
  pageSize,
  onPageChange,
  className,
}: SettingsPaginationProps) {
  const intl = useIntl();

  if (totalItems <= pageSize) {
    return null;
  }

  return (
    <div className={className ?? "flex items-center justify-between pt-2"}>
      <button
        type="button"
        onClick={() => onPageChange(Math.max(0, page - 1))}
        disabled={page === 0}
        className="icon-button disabled:opacity-40"
        aria-label={intl.formatMessage({ id: "settings.pagination.prev" })}
      >
        <IconChevronLeft size={14} stroke={1.8} />
      </button>
      <span className="text-xs text-[var(--text-faint)]">
        {intl.formatMessage(
          { id: "settings.pagination.label" },
          { page: page + 1, totalPages },
        )}
      </span>
      <button
        type="button"
        onClick={() => onPageChange(Math.min(totalPages - 1, page + 1))}
        disabled={page >= totalPages - 1}
        className="icon-button disabled:opacity-40"
        aria-label={intl.formatMessage({ id: "settings.pagination.next" })}
      >
        <IconChevronRight size={14} stroke={1.8} />
      </button>
    </div>
  );
}
