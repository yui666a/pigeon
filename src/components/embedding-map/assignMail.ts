import type { BulkResult } from "../../types/mail";
import type { MapProject } from "../../types/embeddingMap";
import type { MailAssignedEvent } from "../../types/events";

export type AssignOutcome = "assigned" | "failed";

interface AssignDeps {
  /** 実体は mailApi.bulkMoveMails（bulk_move_mails の直 invoke） */
  bulkMove: (mailIds: string[], projectId: string) => Promise<BulkResult>;
  /** 実体は @tauri-apps/api/event の emit */
  emit: (event: string, payload: MailAssignedEvent) => Promise<void>;
}

/**
 * 1通をドロップ先の案件へ割り当て、成功時のみ mail-assigned を emit する。
 * 別ウィンドウは zustand を共有しないため store（useMailStore.bulkMoveMails）
 * を経由せず command を直接叩く（設計書 2026-07-22-embedding-map-window-design.md §4.4）。
 */
export async function assignAndNotify(
  mailId: string,
  project: MapProject,
  deps: AssignDeps,
): Promise<AssignOutcome> {
  try {
    const result = await deps.bulkMove([mailId], project.id);
    if (!result.succeeded.includes(mailId)) return "failed";
    await deps.emit("mail-assigned", { mail_id: mailId, project_id: project.id });
    return "assigned";
  } catch {
    return "failed";
  }
}
