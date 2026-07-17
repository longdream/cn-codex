import { IconChevronDown, IconChevronRight } from "@tabler/icons-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useIntl } from "react-intl";
import { skillCategoriesRead, skillList, skillRead } from "../../api";
import type { SkillCategoryConfig, SkillSummary } from "../../types/skill";
import { SettingsPagination, usePagedItems } from "./SettingsPagination";
import { LanSkillShareSection } from "./LanSkillShareSection";

const SKILLS_PER_GROUP_PAGE = 8;

export function SkillsPanel() {
  const intl = useIntl();
  const [skills, setSkills] = useState<SkillSummary[]>([]);
  const [categories, setCategories] = useState<SkillCategoryConfig[]>([]);
  const [loading, setLoading] = useState(true);
  const [selectedSkill, setSelectedSkill] = useState<string | null>(null);
  const [skillContent, setSkillContent] = useState<string>("");
  const [expandedCategories, setExpandedCategories] = useState<Set<string>>(new Set());
  const [skillPageByCategory, setSkillPageByCategory] = useState<Record<string, number>>({});

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const [list, configuredCategories] = await Promise.all([
        skillList(),
        skillCategoriesRead(),
      ]);
      setSkills(list);
      setCategories(configuredCategories);
    } catch {
      setSkills([]);
      setCategories([]);
    }
    setLoading(false);
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const handleViewSkill = async (id: string) => {
    if (selectedSkill === id) {
      setSelectedSkill(null);
      setSkillContent("");
      return;
    }
    try {
      const detail = await skillRead(id);
      setSelectedSkill(id);
      setSkillContent(detail.content);
    } catch (err) {
      console.error("Failed to read skill:", err);
    }
  };

  const toggleCategory = (labelId: string) => {
    setExpandedCategories((prev) => {
      const next = new Set(prev);
      if (next.has(labelId)) {
        next.delete(labelId);
      } else {
        next.add(labelId);
      }
      return next;
    });
  };

  const skillMap = useMemo(() => {
    const map = new Map<string, SkillSummary>();
    for (const s of skills) {
      map.set(s.id, s);
    }
    return map;
  }, [skills]);

  const categorizedGroups = useMemo(() => {
    const orderedCategories = categories.filter((category) => category.labelId && category.ids.length > 0);
    const groups: Array<{
      labelId: string;
      skills: SkillSummary[];
    }> = [];
    const assignedIds = new Set<string>();

    for (const category of orderedCategories) {
      const matched: SkillSummary[] = [];
      for (const id of category.ids) {
        const skill = skillMap.get(id);
        if (!skill || assignedIds.has(skill.id)) {
          continue;
        }
        matched.push(skill);
        assignedIds.add(skill.id);
      }
      if (matched.length > 0) {
        groups.push({ labelId: category.labelId, skills: matched });
      }
    }

    const uncategorized = skills.filter((skill) => !assignedIds.has(skill.id));
    if (uncategorized.length > 0) {
      groups.push({
        labelId: "settings.skills.category.other",
        skills: uncategorized,
      });
    }

    return groups;
  }, [categories, skills, skillMap]);
  const {
    page: groupsPage,
    setPage: setGroupsPage,
    pageSize: groupsPageSize,
    totalItems: totalGroups,
    totalPages: totalGroupPages,
    pagedItems: pagedCategorizedGroups,
  } = usePagedItems(categorizedGroups);

  useEffect(() => {
    setSkillPageByCategory((prev) => {
      const next: Record<string, number> = {};
      for (const group of categorizedGroups) {
        const maxPage = Math.max(0, Math.ceil(group.skills.length / SKILLS_PER_GROUP_PAGE) - 1);
        const current = prev[group.labelId] ?? 0;
        next[group.labelId] = Math.min(current, maxPage);
      }
      return next;
    });
  }, [categorizedGroups]);

  const handleGroupSkillPageChange = useCallback((labelId: string, nextPage: number) => {
    setSkillPageByCategory((prev) => ({ ...prev, [labelId]: nextPage }));
  }, []);

  if (loading) {
    return (
      <div className="text-[13px] text-[var(--text-muted)]">
        {intl.formatMessage({ id: "common.loading" })}
      </div>
    );
  }

  return (
    <div className="space-y-5">
      <section className="settings-card space-y-4">
        <div className="flex items-end justify-between gap-4">
          <div className="space-y-1">
            <h3 className="text-[13px] font-semibold text-[var(--text-strong)]">
              {intl.formatMessage({ id: "settings.integration.skills" })}
            </h3>
            <p className="text-[13px] text-[var(--text-muted)]">
              {intl.formatMessage({ id: "settings.integration.skillsHint" })}
            </p>
          </div>
          <span className="rounded-full border border-[var(--border-subtle)] bg-[var(--surface-contrast)] px-3 py-1 text-xs text-[var(--text-muted)]">
            {skills.length}
          </span>
        </div>

        {skills.length === 0 ? (
          <div className="rounded-2xl border border-dashed border-[var(--border-strong)] bg-[var(--surface-contrast)]/56 px-4 py-5 text-center">
            <p className="text-[13px] text-[var(--text-muted)]">
              {intl.formatMessage({ id: "settings.integration.noSkills" })}
            </p>
          </div>
        ) : (
          <div className="space-y-2">
            {pagedCategorizedGroups.map((group) => {
              const isOpen = expandedCategories.has(group.labelId);
              const groupPage = skillPageByCategory[group.labelId] ?? 0;
              const groupTotalPages = Math.max(1, Math.ceil(group.skills.length / SKILLS_PER_GROUP_PAGE));
              const pagedSkills = group.skills.slice(
                groupPage * SKILLS_PER_GROUP_PAGE,
                (groupPage + 1) * SKILLS_PER_GROUP_PAGE,
              );
              return (
                <div
                  key={group.labelId}
                  className="overflow-hidden rounded-2xl border border-[var(--border-subtle)] bg-[var(--surface-contrast)]/82"
                >
                  <button
                    onClick={() => toggleCategory(group.labelId)}
                    className="flex w-full items-center justify-between gap-3 px-4 py-3 text-left transition-colors hover:bg-[var(--surface-elevated)]"
                  >
                    <div className="flex items-center gap-2">
                      {isOpen ? (
                        <IconChevronDown size={14} stroke={1.8} className="text-[var(--text-faint)]" />
                      ) : (
                        <IconChevronRight size={14} stroke={1.8} className="text-[var(--text-faint)]" />
                      )}
                      <span className="text-[13px] font-semibold text-[var(--text-strong)]">
                        {intl.formatMessage({ id: group.labelId })}
                      </span>
                    </div>
                    <span className="rounded-full bg-[var(--surface-soft)] px-2 py-0.5 text-[11px] text-[var(--text-muted)]">
                      {group.skills.length}
                    </span>
                  </button>

                  {isOpen && (
                    <div className="border-t border-[var(--border-subtle)]">
                      {pagedSkills.map((skill) => (
                        <div key={skill.id}>
                          <div
                            onClick={() => handleViewSkill(skill.id)}
                            className="w-full cursor-pointer select-text px-4 py-3 pl-10 text-left transition-colors hover:bg-[var(--surface-elevated)]"
                          >
                            <div className="flex items-center justify-between gap-4">
                              <div className="space-y-0.5">
                                <p className="text-[13px] font-medium text-[var(--text-strong)]">{skill.name}</p>
                                {skill.description && (
                                  <p className="text-xs text-[var(--text-muted)]">{skill.description}</p>
                                )}
                              </div>
                              <IconChevronDown
                                size={14}
                                stroke={1.8}
                                className={`shrink-0 text-[var(--text-faint)] transition-transform ${
                                  selectedSkill === skill.id ? "rotate-180" : ""
                                }`}
                              />
                            </div>
                          </div>
                          {selectedSkill === skill.id && skillContent && (
                            <div className="border-t border-[var(--border-subtle)] px-4 py-4 pl-10">
                              <p className="mb-3 text-xs uppercase tracking-[0.16em] text-[var(--text-faint)]">
                                {intl.formatMessage({ id: "settings.integration.skillPreview" })}
                              </p>
                              <pre className="thin-scrollbar max-h-72 select-text overflow-y-auto whitespace-pre-wrap rounded-2xl bg-[var(--surface-main)]/72 p-4 text-xs text-[var(--text-base)]">
                                {skillContent}
                              </pre>
                            </div>
                          )}
                        </div>
                      ))}
                      <div className="px-4 py-2">
                        <SettingsPagination
                          page={groupPage}
                          onPageChange={(nextPage) => handleGroupSkillPageChange(group.labelId, nextPage)}
                          pageSize={SKILLS_PER_GROUP_PAGE}
                          totalItems={group.skills.length}
                          totalPages={groupTotalPages}
                          className="flex items-center justify-between"
                        />
                      </div>
                    </div>
                  )}
                </div>
              );
            })}
            <SettingsPagination
              page={groupsPage}
              onPageChange={setGroupsPage}
              pageSize={groupsPageSize}
              totalItems={totalGroups}
              totalPages={totalGroupPages}
            />
          </div>
        )}
      </section>
      <LanSkillShareSection skills={skills} onInstalled={() => void load()} />
    </div>
  );
}
