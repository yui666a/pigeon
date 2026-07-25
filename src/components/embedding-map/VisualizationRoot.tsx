import { useEffect, useState } from "react";
import { emit } from "@tauri-apps/api/event";
import { embeddingMapApi } from "../../api/embeddingMapApi";
import { mailApi } from "../../api/mailApi";
import { errorMessage } from "../../api/errors";
import { EmbeddingMapCanvas } from "./EmbeddingMapCanvas";
import { PreviewPane } from "./PreviewPane";
import { ProjectPanel } from "./ProjectPanel";
import { usePointDrag } from "./usePointDrag";
import { assignAndNotify } from "./assignMail";
import { applyAssignment } from "./mapAssignment";
import type { MapPoint, MapProject, MailPreview } from "../../types/embeddingMap";

/**
 * 埋め込みマップウィンドウのルート。散布図での発見（Phase A）に加え、
 * 点を案件パネルへ D&D して片付ける（Phase B, 設計書 §4.4・§4.5）。
 */
export function VisualizationRoot() {
  const [points, setPoints] = useState<MapPoint[]>([]);
  const [projects, setProjects] = useState<MapProject[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [assignError, setAssignError] = useState<string | null>(null);
  const [preview, setPreview] = useState<MailPreview | null>(null);
  const [previewLoading, setPreviewLoading] = useState(false);

  useEffect(() => {
    embeddingMapApi.points().then(setPoints).catch((e) => setError(errorMessage(e)));
    embeddingMapApi.projects().then(setProjects).catch((e) => setError(errorMessage(e)));
  }, []);

  const handlePointClick = (point: MapPoint) => {
    setPreviewLoading(true);
    embeddingMapApi
      .preview(point.mail_id)
      .then(setPreview)
      .catch((e) => setError(errorMessage(e)))
      .finally(() => setPreviewLoading(false));
  };

  const { drag, startPress } = usePointDrag(handlePointClick);

  const handleDrop = async (project: MapProject) => {
    if (!drag) return;
    const mailId = drag.point.mail_id;
    setAssignError(null);
    const outcome = await assignAndNotify(mailId, project, {
      bulkMove: mailApi.bulkMoveMails,
      emit,
    });
    if (outcome === "assigned") {
      setPoints((prev) => applyAssignment(prev, mailId, project));
    } else {
      setAssignError("割り当てに失敗しました");
    }
  };

  if (error) return <div className="p-4 text-red-600">エラー: {error}</div>;

  return (
    <div className="flex h-screen">
      <div className="flex-1 flex items-center justify-center overflow-hidden">
        <EmbeddingMapCanvas points={points} onPointMouseDown={startPress} />
      </div>
      <div className="w-80 border-l overflow-y-auto">
        <ProjectPanel projects={projects} dropActive={!!drag} onDrop={handleDrop} />
        {assignError && (
          <div className="px-3 py-2 text-xs text-red-600">{assignError}</div>
        )}
        <PreviewPane preview={preview} loading={previewLoading} />
      </div>
      {drag && (
        <div
          className="pointer-events-none fixed z-50 rounded bg-gray-800 px-2 py-1 text-xs text-white opacity-80"
          style={{ left: drag.x + 12, top: drag.y + 12 }}
        >
          {drag.point.subject}
        </div>
      )}
    </div>
  );
}
