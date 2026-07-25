import type { MapPoint, MapProject } from "../../types/embeddingMap";

/**
 * 割り当て成功後の点の見た目更新。該当する点の案件ラベルと色を差し替えた
 * 新しい配列を返す（楽観的更新はしない — command 成功後にのみ呼ぶこと）。
 */
export function applyAssignment(
  points: MapPoint[],
  mailId: string,
  project: MapProject,
): MapPoint[] {
  return points.map((p) =>
    p.mail_id === mailId
      ? {
          ...p,
          project_id: project.id,
          project_name: project.name,
          project_color: project.color,
        }
      : p,
  );
}
