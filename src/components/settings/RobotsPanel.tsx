import { IconRobot, IconTrash, IconChevronDown, IconChevronRight } from "@tabler/icons-react";
import { useCallback, useEffect, useState } from "react";
import { useIntl } from "react-intl";
import { robotList, robotRead, robotDelete } from "../../api/robot";
import type { RobotSummary, RobotDetail } from "../../types/robot";
import { useAppStore } from "../../stores/appStore";
import { SettingsPagination, usePagedItems } from "./SettingsPagination";

const ROBOT_DETAIL_LIST_PAGE_SIZE = 8;

export function RobotsPanel() {
  const intl = useIntl();
  const selectedRobotId = useAppStore((state) => state.selectedRobotId);
  const setSelectedRobotId = useAppStore((state) => state.setSelectedRobotId);
  const [robots, setRobots] = useState<RobotSummary[]>([]);
  const [loading, setLoading] = useState(true);
  const [expandedId, setExpandedId] = useState<string | null>(null);
  const [detail, setDetail] = useState<RobotDetail | null>(null);
  const [detailLoading, setDetailLoading] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);
  const [detailSkillsPage, setDetailSkillsPage] = useState(0);
  const [detailPluginSkillsPage, setDetailPluginSkillsPage] = useState(0);
  const [detailWorkflowNodesPage, setDetailWorkflowNodesPage] = useState(0);
  const [detailWorkflowPage, setDetailWorkflowPage] = useState(0);
  const {
    page,
    setPage,
    pageSize,
    totalItems,
    totalPages,
    pagedItems: pagedRobots,
  } = usePagedItems(robots);
  const detailSkillsPageSafe = detail
    ? Math.min(detailSkillsPage, Math.max(0, Math.ceil(detail.skills.length / ROBOT_DETAIL_LIST_PAGE_SIZE) - 1))
    : 0;
  const detailPluginSkillsPageSafe = detail
    ? Math.min(
      detailPluginSkillsPage,
      Math.max(0, Math.ceil(detail.pluginSkills.length / ROBOT_DETAIL_LIST_PAGE_SIZE) - 1),
    )
    : 0;
  const detailWorkflowNodesPageSafe = detail
    ? Math.min(
      detailWorkflowNodesPage,
      Math.max(0, Math.ceil(detail.workflowNodes.length / ROBOT_DETAIL_LIST_PAGE_SIZE) - 1),
    )
    : 0;
  const detailWorkflowPageSafe = detail
    ? Math.min(detailWorkflowPage, Math.max(0, Math.ceil(detail.workflow.length / ROBOT_DETAIL_LIST_PAGE_SIZE) - 1))
    : 0;
  const pagedDetailSkills = detail
    ? detail.skills.slice(
      detailSkillsPageSafe * ROBOT_DETAIL_LIST_PAGE_SIZE,
      (detailSkillsPageSafe + 1) * ROBOT_DETAIL_LIST_PAGE_SIZE,
    )
    : [];
  const pagedDetailPluginSkills = detail
    ? detail.pluginSkills.slice(
      detailPluginSkillsPageSafe * ROBOT_DETAIL_LIST_PAGE_SIZE,
      (detailPluginSkillsPageSafe + 1) * ROBOT_DETAIL_LIST_PAGE_SIZE,
    )
    : [];
  const pagedDetailWorkflowNodes = detail
    ? detail.workflowNodes.slice(
      detailWorkflowNodesPageSafe * ROBOT_DETAIL_LIST_PAGE_SIZE,
      (detailWorkflowNodesPageSafe + 1) * ROBOT_DETAIL_LIST_PAGE_SIZE,
    )
    : [];
  const pagedDetailWorkflow = detail
    ? detail.workflow.slice(
      detailWorkflowPageSafe * ROBOT_DETAIL_LIST_PAGE_SIZE,
      (detailWorkflowPageSafe + 1) * ROBOT_DETAIL_LIST_PAGE_SIZE,
    )
    : [];
  const workflowNodeOffset = detailWorkflowNodesPageSafe * ROBOT_DETAIL_LIST_PAGE_SIZE;

  const load = useCallback(async () => {
    try {
      const list = await robotList();
      setRobots(list);
    } catch {
      setRobots([]);
    }
    setLoading(false);
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const handleToggleExpand = async (id: string) => {
    if (expandedId === id) {
      setExpandedId(null);
      setDetail(null);
      return;
    }
    setDetailSkillsPage(0);
    setDetailPluginSkillsPage(0);
    setDetailWorkflowNodesPage(0);
    setDetailWorkflowPage(0);
    setExpandedId(id);
    setDetailLoading(true);
    try {
      const d = await robotRead(id);
      setDetail(d);
    } catch {
      setDetail(null);
    }
    setDetailLoading(false);
  };

  const handleDelete = async (robot: RobotSummary) => {
    const confirmed = window.confirm(
      intl.formatMessage(
        { id: "settings.robots.deleteConfirm" },
        { name: robot.name },
      ),
    );
    if (!confirmed) return;

    setActionError(null);
    try {
      await robotDelete(robot.id);
      // 删除当前选中机器人时，立即清空选择，避免聊天输入区保留失效 robotId。
      if (selectedRobotId === robot.id) {
        setSelectedRobotId(null);
      }
      if (expandedId === robot.id) {
        setExpandedId(null);
        setDetail(null);
      }
      await load();
      // 通知聊天输入区刷新机器人列表，确保删除后下拉菜单实时同步。
      window.dispatchEvent(new CustomEvent("robot-list-changed"));
    } catch (err) {
      setActionError(err instanceof Error ? err.message : String(err));
    }
  };

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
              {intl.formatMessage({ id: "settings.robots.title" })}
            </h3>
            <p className="text-[13px] text-[var(--text-muted)]">
              {intl.formatMessage({ id: "settings.robots.hint" })}
            </p>
          </div>
          <span className="rounded-full border border-[var(--border-subtle)] bg-[var(--surface-contrast)] px-3 py-1 text-xs text-[var(--text-muted)]">
            {robots.length}
          </span>
        </div>

        {actionError && (
          <p className="break-words rounded-2xl border border-red-500/30 bg-red-500/10 px-3 py-2 text-xs text-red-300">
            {actionError}
          </p>
        )}

        {robots.length === 0 ? (
          <div className="rounded-2xl border border-dashed border-[var(--border-strong)] bg-[var(--surface-contrast)]/56 px-4 py-5 text-center">
            <IconRobot size={24} stroke={1.4} className="mx-auto mb-2 text-[var(--text-faint)]" />
            <p className="text-[13px] text-[var(--text-muted)]">
              {intl.formatMessage({ id: "settings.robots.empty" })}
            </p>
          </div>
        ) : (
          <div className="space-y-3">
            {pagedRobots.map((robot) => {
              const isExpanded = expandedId === robot.id;
              return (
                <div
                  key={robot.id}
                  className="rounded-2xl border border-[var(--border-subtle)] bg-[var(--surface-contrast)]/82 px-4 py-4"
                >
                  <div className="flex flex-wrap items-center justify-between gap-3">
                    <button
                      className="flex min-w-0 items-center gap-2 text-left"
                      onClick={() => handleToggleExpand(robot.id)}
                    >
                      {isExpanded ? (
                        <IconChevronDown size={14} stroke={1.8} className="shrink-0 text-[var(--text-muted)]" />
                      ) : (
                        <IconChevronRight size={14} stroke={1.8} className="shrink-0 text-[var(--text-muted)]" />
                      )}
                      <div className="min-w-0 space-y-0.5">
                        <p className="text-[13px] font-semibold text-[var(--text-strong)]">
                          {robot.name}
                        </p>
                        <p className="truncate text-xs text-[var(--text-muted)]">
                          {robot.description}
                        </p>
                      </div>
                    </button>
                    <div className="flex items-center gap-2">
                      <span className="rounded-full bg-[var(--surface-soft)] px-2 py-1 text-[11px] text-[var(--text-muted)]">
                        {intl.formatMessage(
                          { id: "settings.robots.skillsBadge" },
                          { count: robot.skillsCount + robot.pluginSkillsCount },
                        )}
                      </span>
                      <span className="rounded-full bg-[var(--surface-soft)] px-2 py-1 text-[11px] text-[var(--text-muted)]">
                        {intl.formatMessage(
                          { id: "settings.robots.workflowBadge" },
                          { count: robot.workflowSteps },
                        )}
                      </span>
                      <button
                        onClick={() => handleDelete(robot)}
                        className="flex h-7 w-7 items-center justify-center rounded-[var(--radius-sm)] border border-red-500/25 bg-red-500/10 text-red-300 transition-colors hover:bg-red-500/20"
                        title={intl.formatMessage({ id: "settings.robots.deleteTitle" })}
                      >
                        <IconTrash size={13} stroke={1.8} />
                      </button>
                    </div>
                  </div>

                  {isExpanded && (
                    <div className="mt-4 space-y-3 border-t border-[var(--border-subtle)] pt-3">
                      {detailLoading ? (
                        <p className="text-xs text-[var(--text-muted)]">
                          {intl.formatMessage({ id: "common.loading" })}
                        </p>
                      ) : detail ? (
                        <>
                          {detail.skills.length > 0 && (
                            <div>
                              <p className="mb-1.5 text-[11px] font-semibold uppercase tracking-wider text-[var(--text-faint)]">
                                {intl.formatMessage({ id: "settings.robots.localSkills" })}
                              </p>
                              <div className="flex flex-wrap gap-1.5">
                                {pagedDetailSkills.map((s) => (
                                  <span key={s} className="rounded-full bg-[var(--accent-soft)] px-2 py-0.5 text-[11px] text-[var(--accent-strong)]">
                                    {s}
                                  </span>
                                ))}
                              </div>
                              <SettingsPagination
                                page={detailSkillsPageSafe}
                                onPageChange={setDetailSkillsPage}
                                pageSize={ROBOT_DETAIL_LIST_PAGE_SIZE}
                                totalItems={detail.skills.length}
                                totalPages={Math.max(1, Math.ceil(detail.skills.length / ROBOT_DETAIL_LIST_PAGE_SIZE))}
                                className="flex items-center justify-between pt-2"
                              />
                            </div>
                          )}

                          {detail.pluginSkills.length > 0 && (
                            <div>
                              <p className="mb-1.5 text-[11px] font-semibold uppercase tracking-wider text-[var(--text-faint)]">
                                {intl.formatMessage({ id: "settings.robots.pluginSkills" })}
                              </p>
                              <div className="flex flex-wrap gap-1.5">
                                {pagedDetailPluginSkills.map((ps) => (
                                  <span key={`${ps.pluginId}/${ps.skillId}`} className="rounded-full bg-[var(--surface-soft)] px-2 py-0.5 text-[11px] text-[var(--text-muted)]">
                                    {ps.pluginId}/{ps.skillId}
                                  </span>
                                ))}
                              </div>
                              <SettingsPagination
                                page={detailPluginSkillsPageSafe}
                                onPageChange={setDetailPluginSkillsPage}
                                pageSize={ROBOT_DETAIL_LIST_PAGE_SIZE}
                                totalItems={detail.pluginSkills.length}
                                totalPages={Math.max(1, Math.ceil(detail.pluginSkills.length / ROBOT_DETAIL_LIST_PAGE_SIZE))}
                                className="flex items-center justify-between pt-2"
                              />
                            </div>
                          )}

                          {detail.workflowNodes.length > 0 ? (
                            <div>
                              <p className="mb-1.5 text-[11px] font-semibold uppercase tracking-wider text-[var(--text-faint)]">
                                {intl.formatMessage({ id: "settings.robots.workflow" })}
                              </p>
                              <div className="space-y-2">
                                {pagedDetailWorkflowNodes.map((node, i) => (
                                  <div
                                    key={`${workflowNodeOffset + i}-${node.objective}`}
                                    className="rounded-xl border border-[var(--border-subtle)] bg-[var(--surface-main)]/60 px-3 py-2"
                                  >
                                    <p className="text-xs font-medium text-[var(--text-strong)]">
                                      {workflowNodeOffset + i + 1}. {node.objective}
                                    </p>
                                    {(node.skills.length > 0 || node.pluginSkills.length > 0) && (
                                      <div className="mt-1.5 space-y-1">
                                        {node.skills.length > 0 && (
                                          <div className="flex flex-wrap gap-1.5">
                                            {node.skills.map((skillId) => (
                                              <span
                                                key={`${i}-local-${skillId}`}
                                                className="rounded-full bg-[var(--accent-soft)] px-2 py-0.5 text-[11px] text-[var(--accent-strong)]"
                                              >
                                                {skillId}
                                              </span>
                                            ))}
                                          </div>
                                        )}
                                        {node.pluginSkills.length > 0 && (
                                          <div className="flex flex-wrap gap-1.5">
                                            {node.pluginSkills.map((pluginSkill) => (
                                              <span
                                                key={`${i}-plugin-${pluginSkill.pluginId}-${pluginSkill.skillId}`}
                                                className="rounded-full bg-[var(--surface-soft)] px-2 py-0.5 text-[11px] text-[var(--text-muted)]"
                                              >
                                                {pluginSkill.pluginId}/{pluginSkill.skillId}
                                              </span>
                                            ))}
                                          </div>
                                        )}
                                      </div>
                                    )}
                                  </div>
                                ))}
                              </div>
                              <SettingsPagination
                                page={detailWorkflowNodesPageSafe}
                                onPageChange={setDetailWorkflowNodesPage}
                                pageSize={ROBOT_DETAIL_LIST_PAGE_SIZE}
                                totalItems={detail.workflowNodes.length}
                                totalPages={Math.max(1, Math.ceil(detail.workflowNodes.length / ROBOT_DETAIL_LIST_PAGE_SIZE))}
                                className="flex items-center justify-between pt-2"
                              />
                            </div>
                          ) : detail.workflow.length > 0 && (
                            <div>
                              <p className="mb-1.5 text-[11px] font-semibold uppercase tracking-wider text-[var(--text-faint)]">
                                {intl.formatMessage({ id: "settings.robots.workflow" })}
                              </p>
                              <ol className="list-inside list-decimal space-y-1 text-xs text-[var(--text-muted)]">
                                {pagedDetailWorkflow.map((step, i) => (
                                  <li key={i}>{step}</li>
                                ))}
                              </ol>
                              <SettingsPagination
                                page={detailWorkflowPageSafe}
                                onPageChange={setDetailWorkflowPage}
                                pageSize={ROBOT_DETAIL_LIST_PAGE_SIZE}
                                totalItems={detail.workflow.length}
                                totalPages={Math.max(1, Math.ceil(detail.workflow.length / ROBOT_DETAIL_LIST_PAGE_SIZE))}
                                className="flex items-center justify-between pt-2"
                              />
                            </div>
                          )}

                          {detail.systemPrompt && (
                            <div>
                              <p className="mb-1.5 text-[11px] font-semibold uppercase tracking-wider text-[var(--text-faint)]">
                                System Prompt
                              </p>
                              <pre className="thin-scrollbar max-h-40 overflow-auto rounded-xl bg-[var(--surface-main)] p-3 text-xs text-[var(--text-muted)]">
                                {detail.systemPrompt}
                              </pre>
                            </div>
                          )}
                        </>
                      ) : null}
                    </div>
                  )}
                </div>
              );
            })}
            <SettingsPagination
              page={page}
              onPageChange={setPage}
              pageSize={pageSize}
              totalItems={totalItems}
              totalPages={totalPages}
            />
          </div>
        )}
      </section>
    </div>
  );
}
