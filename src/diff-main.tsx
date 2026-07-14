import React, { useEffect } from "react";
import ReactDOM from "react-dom/client";
import { IntlProvider } from "react-intl";
import { RunSummaryDiffWindow } from "./components/diff/RunSummaryDiffWindow";
import { useSettingsStore, initSettingsFromDb } from "./stores/settingsStore";
import enUS from "./i18n/en-US/common.json";
import zhCN from "./i18n/zh-CN/common.json";
import "./index.css";

const messages: Record<string, Record<string, string>> = {
  "zh-CN": zhCN,
  "en-US": enUS,
};

function DiffApp() {
  const locale = useSettingsStore((s) => s.locale);

  useEffect(() => {
    void initSettingsFromDb();
  }, []);

  return (
    <IntlProvider
      locale={locale}
      messages={messages[locale] ?? zhCN}
      defaultLocale="zh-CN"
    >
      <RunSummaryDiffWindow />
    </IntlProvider>
  );
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <DiffApp />
  </React.StrictMode>,
);
