import { create } from "zustand";
import { listen } from "@tauri-apps/api/event";
import { classifyApi } from "../api/classifyApi";
import { errorMessage } from "../api/errors";
import { useErrorStore } from "./errorStore";
import { useMailStore } from "./mailStore";
import { useProjectStore } from "./projectStore";
import type {
  ClassifyProgressEvent,
  ClassifyResponse,
} from "../types/classifier";

/**
 * バッチ分類の制御ストア。
 *
 * ループの本体はバックエンド（classify_batch）にあり、ここは
 * 「1 invoke → create 提案で停止したら承認/却下 → 再 invoke で再開」の
 * 薄い制御と進捗イベントの反映だけを持つ
 * （設計: docs/superpowers/specs/2026-07-13-classify-batch-backend-design.md）。
 */
interface ClassifyState {
  classifying: boolean;
  progress: { current: number; total: number } | null;
  pendingProposal: ClassifyResponse | null;
  // 内部: 実行中バッチのアカウント（再開・キャンセルの宛先）
  _accountId: string | null;

  classifyMail: (mailId: string) => Promise<void>;
  classifyAll: (accountId: string) => Promise<void>;
  cancelClassification: () => Promise<void>;
  approveNewProject: (
    mailId: string,
    projectName: string,
    description?: string,
  ) => Promise<void>;
  rejectClassification: (mailId: string) => Promise<void>;
  /** classify-progress イベントの購読を張る（ClassifyButton がマウント時に呼ぶ） */
  initProgressListener: () => Promise<() => void>;
}

export const useClassifyStore = create<ClassifyState>((set, get) => {
  // classify_batch を1回 invoke し、戻り値（次の停止点 or 完了）を状態に反映する
  const runBatch = async (accountId: string): Promise<void> => {
    set({ classifying: true, pendingProposal: null, _accountId: accountId });
    try {
      const outcome = await classifyApi.classifyBatch(accountId);
      switch (outcome.status) {
        case "paused":
          // create 提案で停止。承認/却下を待つ（approve/reject が再開する）
          set({
            pendingProposal: outcome.proposal,
            progress: { current: outcome.done, total: outcome.total },
          });
          return;
        case "already_running":
          // 進行中のバッチに任せる（進捗はイベントで届く）
          return;
        default:
          // completed / cancelled
          set({
            classifying: false,
            progress: null,
            pendingProposal: null,
            _accountId: null,
          });
      }
    } catch (e) {
      useErrorStore.getState().addError(errorMessage(e));
      set({
        classifying: false,
        progress: null,
        pendingProposal: null,
        _accountId: null,
      });
    }
  };

  return {
    classifying: false,
    progress: null,
    pendingProposal: null,
    _accountId: null,

    classifyMail: async (mailId) => {
      try {
        await classifyApi.classifyMail(mailId);
      } catch (e) {
        useErrorStore.getState().addError(errorMessage(e));
      }
    },

    classifyAll: async (accountId) => {
      await runBatch(accountId);
    },

    cancelClassification: async () => {
      const accountId = get()._accountId;
      if (accountId) {
        try {
          await classifyApi.cancelClassification(accountId);
        } catch (e) {
          useErrorStore.getState().addError(errorMessage(e));
        }
      }
      // 実行中の invoke が返るのを待たず即座に表示を畳む
      //（バックエンドは次のメール処理前に中断してバッチを破棄する）
      set({
        classifying: false,
        progress: null,
        pendingProposal: null,
        _accountId: null,
      });
    },

    approveNewProject: async (mailId, projectName, description) => {
      try {
        const project = await classifyApi.approveNewProject(
          mailId,
          projectName,
          description,
        );
        useProjectStore.getState().addProject(project);
      } catch (e) {
        // 承認に失敗しただけなので提案は残す（再試行できる）
        useErrorStore.getState().addError(errorMessage(e));
        return;
      }
      set({ pendingProposal: null });
      const accountId = get()._accountId;
      if (accountId) await runBatch(accountId); // 新案件込みで続きから再開
    },

    rejectClassification: async (mailId) => {
      try {
        await classifyApi.rejectClassification(mailId);
      } catch (e) {
        useErrorStore.getState().addError(errorMessage(e));
      }
      set({ pendingProposal: null });
      const accountId = get()._accountId;
      if (accountId) await runBatch(accountId); // 却下メールはスキップして再開
    },

    initProgressListener: async () => {
      const unlisten = await listen<ClassifyProgressEvent>(
        "classify-progress",
        (event) => {
          // 実行中バッチのアカウントの進捗のみ反映する
          const { _accountId, classifying } = get();
          if (classifying && _accountId === event.payload.account_id) {
            set({
              progress: {
                current: event.payload.current,
                total: event.payload.total,
              },
            });
            // 案件へ確定割り当てされたメールは未分類一覧から即座に消す
            // （バッチ完了を待たずに「移動した」ことが分かるように）
            if (event.payload.assigned_mail_id) {
              useMailStore
                .getState()
                .removeUnclassifiedMail(event.payload.assigned_mail_id);
            }
          }
        },
      );
      return unlisten;
    },
  };
});
