import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { api } from "./api";
import { QuickPanel } from "./components/QuickPanel";
import { initI18n } from "./i18n";
import { isQuickPanel } from "./runtime";
import { applyTheme, loadTheme } from "./format";
import "./theme.css";
import "./App.css";

async function start() {
  const root = document.getElementById("root");
  if (!root) {
    throw new Error("root element missing");
  }
  // The language is decided on the Rust side, so wait for it before the first render.
  const language = await api.languageState().catch(() => ({ effective: "en" }));
  await initI18n(language.effective);
  applyTheme(loadTheme());
  let page = isQuickPanel() ? <QuickPanel /> : <App />;
  // Only the screenshot build (vite --mode mock) can show the drawing of the tray menu
  if (import.meta.env.MODE === "mock" && new URLSearchParams(window.location.search).get("screen") === "tray") {
    const { TrayPreview } = await import("./preview/TrayPreview");
    page = <TrayPreview />;
  }
  ReactDOM.createRoot(root).render(<React.StrictMode>{page}</React.StrictMode>);
}

void start();
