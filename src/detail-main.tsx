import React, { useEffect } from "react";
import ReactDOM from "react-dom/client";
import { IntlProvider } from "react-intl";
import { DocumentDetailWindow } from "./components/detail/DocumentDetailWindow";
import { useSettingsStore, initSettingsFromDb } from "./stores/settingsStore";
import { applyDocumentTheme } from "./utils/applyTheme";
import enUS from "./i18n/en-US/common.json";
import zhCN from "./i18n/zh-CN/common.json";
import "./index.css";

const messages: Record<string, Record<string, string>> = {
  "zh-CN": zhCN,
  "en-US": enUS,
};

function DetailApp() {
  const locale = useSettingsStore((s) => s.locale);
  const theme = useSettingsStore((s) => s.theme);

  // 不阻塞首帧挂载：先立刻渲染详情页加载态，再后台补齐 locale 等设置。
  useEffect(() => {
    void initSettingsFromDb();
  }, []);

  useEffect(() => {
    const media = window.matchMedia("(prefers-color-scheme: light)");
    const applyTheme = () => {
      applyDocumentTheme(theme, document, media);
    };

    applyTheme();

    if (theme !== "system") {
      return;
    }

    media.addEventListener("change", applyTheme);
    return () => media.removeEventListener("change", applyTheme);
  }, [theme]);

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

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <DetailApp />
  </React.StrictMode>,
);
