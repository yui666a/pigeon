import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { useErrorStore } from "./errorStore";
import { useProjectStore } from "./projectStore";
import type { ClassifyResponse } from "../types/classifier";
import type { Project } from "../types/project";

interface UnclassifiedMailRef {
  id: string;
}

// classify_mail の戻り `result`（Rust ClassifyResult、#[serde(tag="action")]）のフラット形。
// action ごとに project_id（assign）/ project_name・description（create）が付く。
interface ClassifyResultRaw {
  action: "assign" | "create" | "unclassified";
  project_id?: string;
  project_name?: string;
  description?: string;
  confidence: number;
  reason: string;
}

interface ClassifyState {
  classifying: boolean;
  progress: { current: number; total: number } | null;
  pendingProposal: ClassifyResponse | null;
  error: string | null;
  // 内部: 逐次ループの状態
  _queue: UnclassifiedMailRef[];
  _index: number;
  _cancelled: boolean;

  classifyMail: (mailId: string) => Promise<void>;
  classifyAll: (accountId: string) => Promise<void>;
  cancelClassification: () => Promise<void>;
  approveNewProject: (
    mailId: string,
    projectName: string,
    description?: string,
  ) => Promise<void>;
  rejectClassification: (mailId: string) => Promise<void>;
}

export const useClassifyStore = create<ClassifyState>((set, get) => {
  // 次の1件を分類し、create でなければ自動で次へ進む
  const classifyNext = async (): Promise<void> => {
    const { _queue, _index, _cancelled } = get();
    if (_cancelled || _index >= _queue.length) {
      set({ classifying: false, progress: null, pendingProposal: null });
      return;
    }
    const mail = _queue[_index];
    let res: ClassifyResponse;
    try {
      // classify_mail は Rust の ClassifyResponse = { mail_id, result: ClassifyResult }
      // を返す（result の中に action/confidence/reason/project_id/project_name/description）。
      // フロントの ClassifyResponse はフラットなので、ここで平坦化する。
      const r = await invoke<{ mail_id: string; result: ClassifyResultRaw }>(
        "classify_mail",
        { mailId: mail.id },
      );
      res = { mail_id: r.mail_id, ...r.result };
    } catch (e) {
      useErrorStore.getState().addError(String(e));
      set({ classifying: false, progress: null });
      return;
    }
    set({
      _index: _index + 1,
      progress: { current: _index + 1, total: _queue.length },
    });
    if (res.action === "create") {
      set({ pendingProposal: res });
      return; // 停止：承認/却下を待つ
    }
    await classifyNext();
  };

  return {
    classifying: false,
    progress: null,
    pendingProposal: null,
    error: null,
    _queue: [],
    _index: 0,
    _cancelled: false,

    classifyMail: async (mailId) => {
      try {
        await invoke("classify_mail", { mailId });
      } catch (e) {
        set({ error: String(e) });
        useErrorStore.getState().addError(String(e));
      }
    },

    classifyAll: async (accountId) => {
      try {
        const mails = await invoke<UnclassifiedMailRef[]>(
          "get_unclassified_mails",
          { accountId },
        );
        set({
          classifying: true,
          _queue: mails,
          _index: 0,
          _cancelled: false,
          pendingProposal: null,
          progress: { current: 0, total: mails.length },
          error: null,
        });
        await classifyNext();
      } catch (e) {
        set({ error: String(e), classifying: false, progress: null });
        useErrorStore.getState().addError(String(e));
      }
    },

    cancelClassification: async () => {
      set({ _cancelled: true, classifying: false, progress: null, pendingProposal: null });
    },

    approveNewProject: async (mailId, projectName, description) => {
      try {
        const project = await invoke<Project>("approve_new_project", {
          mailId,
          projectName,
          description: description ?? null,
        });
        useProjectStore.getState().addProject(project);
        set({ pendingProposal: null });
        await classifyNext();
      } catch (e) {
        set({ error: String(e) });
        useErrorStore.getState().addError(String(e));
      }
    },

    rejectClassification: async (mailId) => {
      try {
        await invoke("reject_classification", { mailId });
      } catch (e) {
        useErrorStore.getState().addError(String(e));
      }
      set({ pendingProposal: null });
      await classifyNext();
    },
  };
});
