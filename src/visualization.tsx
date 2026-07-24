import React, { useEffect, useState } from "react";
import ReactDOM from "react-dom/client";
import "./App.css"; // main.tsx はCSSを直接importせず App.tsx が読む ./App.css がグローバルCSS（Tailwind込み）のため合わせる
import { embeddingMapApi } from "./api/embeddingMapApi";
import { EmbeddingMapCanvas } from "./components/embedding-map/EmbeddingMapCanvas";
import { PreviewPane } from "./components/embedding-map/PreviewPane";
import type { MapPoint, MailPreview } from "./types/embeddingMap";
import { errorMessage } from "./api/errors";

function VisualizationRoot() {
  const [points, setPoints] = useState<MapPoint[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [preview, setPreview] = useState<MailPreview | null>(null);
  const [previewLoading, setPreviewLoading] = useState(false);

  useEffect(() => {
    embeddingMapApi.points().then(setPoints).catch((e) => setError(errorMessage(e)));
  }, []);

  const handlePointClick = (mailId: string) => {
    setPreviewLoading(true);
    embeddingMapApi
      .preview(mailId)
      .then(setPreview)
      .catch((e) => setError(errorMessage(e)))
      .finally(() => setPreviewLoading(false));
  };

  if (error) return <div className="p-4 text-red-600">エラー: {error}</div>;

  return (
    <div className="flex h-screen">
      <div className="flex-1 flex items-center justify-center overflow-hidden">
        <EmbeddingMapCanvas points={points} onPointClick={handlePointClick} />
      </div>
      <div className="w-80 border-l overflow-y-auto">
        <PreviewPane preview={preview} loading={previewLoading} />
      </div>
    </div>
  );
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <VisualizationRoot />
  </React.StrictMode>,
);
