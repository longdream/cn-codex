import { IconBook2, IconBrain, IconDatabase, IconSettings2, IconSparkles } from "@tabler/icons-react";
import { useState } from "react";
import { useIntl } from "react-intl";
import { ExperiencePanel } from "./ExperiencePanel";
import { KnowledgePanel } from "./KnowledgePanel";
import { SmartbrainDatabasePanel } from "./SmartbrainDatabasePanel";
import { SmartbrainDatabaseSettingsPanel } from "./SmartbrainDatabaseSettingsPanel";

type SmartbrainTab = "knowledge" | "experience" | "database" | "databaseSettings";

export function SmartbrainPanel() {
  const intl = useIntl();
  const [tab, setTab] = useState<SmartbrainTab>("knowledge");

  const tabs: Array<{ id: SmartbrainTab; icon: JSX.Element; label: string }> = [
    {
      id: "knowledge",
      icon: <IconBook2 size={14} stroke={1.8} />,
      label: intl.formatMessage({ id: "settings.smartbrain.knowledge" }),
    },
    {
      id: "experience",
      icon: <IconSparkles size={14} stroke={1.8} />,
      label: intl.formatMessage({ id: "settings.smartbrain.experience" }),
    },
    {
      id: "database",
      icon: <IconDatabase size={14} stroke={1.8} />,
      label: intl.formatMessage({ id: "settings.smartbrain.database" }),
    },
    {
      id: "databaseSettings",
      icon: <IconSettings2 size={14} stroke={1.8} />,
      label: intl.formatMessage({ id: "settings.smartbrain.databaseSettings" }),
    },
  ];

  return (
    <div className="space-y-5">
      <section className="settings-card space-y-3">
        <div className="flex items-start justify-between gap-3">
          <div className="flex items-start gap-3">
            <div className="flex h-10 w-10 items-center justify-center rounded-[var(--radius-md)] bg-[var(--accent-soft)] text-[var(--accent-strong)]">
              <IconBrain size={18} stroke={1.9} />
            </div>
            <div>
              <h4 className="text-[13px] font-semibold text-[var(--text-strong)]">
                {intl.formatMessage({ id: "settings.smartbrain" })}
              </h4>
              <p className="mt-1 text-xs text-[var(--text-faint)]">
                {intl.formatMessage({ id: "settings.smartbrain.description" })}
              </p>
            </div>
          </div>
          <span className="rounded-full bg-[var(--surface-soft)] px-2 py-1 text-[10px] text-[var(--text-faint)]">
            {intl.formatMessage({ id: "settings.smartbrain.dialogScoped" })}
          </span>
        </div>

        <div className="flex flex-wrap gap-2">
          {tabs.map((item) => (
            <button
              key={item.id}
              type="button"
              onClick={() => setTab(item.id)}
              className={`inline-flex items-center gap-1.5 rounded-[var(--radius-md)] border px-3 py-1.5 text-xs transition-colors ${
                tab === item.id
                  ? "border-[var(--accent-border)] bg-[var(--accent-soft)] text-[var(--accent-strong)]"
                  : "border-[var(--border-subtle)] bg-[var(--surface-soft)] text-[var(--text-muted)] hover:bg-[var(--surface-elevated)] hover:text-[var(--text-strong)]"
              }`}
            >
              {item.icon}
              {item.label}
            </button>
          ))}
        </div>
      </section>

      {tab === "knowledge" && <KnowledgePanel />}
      {tab === "experience" && <ExperiencePanel />}
      {tab === "database" && <SmartbrainDatabasePanel />}
      {tab === "databaseSettings" && <SmartbrainDatabaseSettingsPanel />}
    </div>
  );
}
