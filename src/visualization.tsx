import React from "react";
import ReactDOM from "react-dom/client";
import "./App.css"; // main.tsx はCSSを直接importせず App.tsx が読む ./App.css がグローバルCSS（Tailwind込み）のため合わせる

function VisualizationRoot() {
  return <div style={{ padding: 16 }}>埋め込みマップ（準備中）</div>;
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <VisualizationRoot />
  </React.StrictMode>,
);
