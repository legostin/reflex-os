import React from "react";
import ReactDOM from "react-dom/client";
import { getCurrentWindow } from "@tauri-apps/api/window";
import App from "./App";
import { I18nProvider } from "./i18n";

const label = (() => {
  try {
    const l = getCurrentWindow().label;
    if (l === "main" || l === "quick") return l;
  } catch {}
  return "main";
})();
document.documentElement.dataset.window = label;
document.body.dataset.window = label;

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <I18nProvider>
      <App />
    </I18nProvider>
  </React.StrictMode>,
);
