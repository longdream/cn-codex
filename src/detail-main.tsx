import React from "react";
import ReactDOM from "react-dom/client";
import { IntlProvider } from "react-intl";
import { DocumentDetailWindow } from "./components/detail/DocumentDetailWindow";
import { useSettingsStore, initSettingsFromDb } from "./stores/settingsStore";
import enUS from "./i18n/en-US/common.json";
import zhCN from "./i18n/zh-CN/common.json";
import "./index.css";

const messages: Record<string, Record<string, string>> = {
  "zh-CN": zhCN,
  "en-US": enUS,
};

function DetailApp() {
  const locale = useSettingsStore((s) => s.locale);
  return (
    <IntlProvider
      locale={locale}
      messages={messages[locale] ?? zhCN}
      defaultLocale="zh-CN"
    >
      <DocumentDetailWindow />
    </IntlProvider>
  );
}

initSettingsFromDb().then(() => {
  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    <React.StrictMode>
      <DetailApp />
    </React.StrictMode>,
  );
});
