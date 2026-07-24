import React from "react";
import ReactDOM from "react-dom/client";
import "./App.css"; // main.tsx はCSSを直接importせず App.tsx が読む ./App.css がグローバルCSS（Tailwind込み）のため合わせる
import { VisualizationRoot } from "./components/embedding-map/VisualizationRoot";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <VisualizationRoot />
  </React.StrictMode>,
);
