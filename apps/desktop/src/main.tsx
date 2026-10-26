import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { api } from "./api";
import { initI18n } from "./i18n";
import "./App.css";

async function start() {
  const root = document.getElementById("root");
  if (!root) {
    throw new Error("root element missing");
  }
  // The language is decided on the Rust side, so wait for it before the first render.
  const language = await api.languageState().catch(() => ({ effective: "en" as const }));
  await initI18n(language.effective);
  ReactDOM.createRoot(root).render(
    <React.StrictMode>
      <App />
    </React.StrictMode>,
  );
}

void start();
