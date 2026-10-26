import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./i18n";
import "./App.css";

const root = document.getElementById("root");
if (!root) {
  throw new Error("root element missing");
}
ReactDOM.createRoot(root).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
