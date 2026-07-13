import { describe, it, expect, vi, beforeEach } from "vitest";

const invokeMock = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

// classify-progress イベントの購読ハンドラを捕まえて、テストから発火できるようにする
type ProgressHandler = (event: {
  payload: {
    account_id: string;
    current: number;
    total: number;
    assigned_mail_id?: string | null;
  };
}) => void;
let progressHandler: ProgressHandler | null = null;
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn((_event: string, handler: ProgressHandler) => {
    progressHandler = handler;
    return Promise.resolve(() => {});
  }),
}));

import { useClassifyStore } from "../../stores/classifyStore";
import { useProjectStore } from "../../stores/projectStore";
import { useMailStore } from "../../stores/mailStore";

const resetStore = () =>
  useClassifyStore.setState({
    classifying: false,
    progress: null,
    pendingProposal: null,
    _accountId: null,
  });

beforeEach(() => {
  invokeMock.mockReset();
  progressHandler = null;
  resetStore();
  useProjectStore.setState({ projects: [] });
  // spyOn（removeUnclassifiedMail 等）の履歴・実装をテスト間で持ち越さない
  vi.restoreAllMocks();
});

// classify_batch の戻り値（Rust の ClassifyBatchOutcome。status で判別する）
const completed = (done: number, total: number) => ({
  status: "completed",
  done,
  total,
});
const paused = (mailId: string, done: number, total: number) => ({
  status: "paused",
  proposal: {
    mail_id: mailId,
    action: "create",
    project_name: "New",
    description: "d",
    confidence: 0.8,
    reason: "r",
  },
  done,
  total,
});

const batchCalls = () =>
  invokeMock.mock.calls.filter((c) => c[0] === "classify_batch");

describe("classifyStore batch flow", () => {
  it("classifyAll は classify_batch を1回だけ invoke し、completed で状態をリセットする", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "classify_batch") return Promise.resolve(completed(2, 2));
      return Promise.resolve();
    });

    await useClassifyStore.getState().classifyAll("acc1");

    expect(batchCalls()).toHaveLength(1);
    expect(batchCalls()[0][1]).toEqual({ accountId: "acc1" });
    expect(useClassifyStore.getState().classifying).toBe(false);
    expect(useClassifyStore.getState().pendingProposal).toBeNull();
    expect(useClassifyStore.getState().progress).toBeNull();
  });

  it("paused で pendingProposal に提案をセットし、実行中のまま停止する", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "classify_batch") return Promise.resolve(paused("m1", 1, 3));
      return Promise.resolve();
    });

    await useClassifyStore.getState().classifyAll("acc1");

    expect(useClassifyStore.getState().pendingProposal?.mail_id).toBe("m1");
    expect(useClassifyStore.getState().classifying).toBe(true);
    expect(useClassifyStore.getState().progress).toEqual({
      current: 1,
      total: 3,
    });
  });

  it("approveNewProject でプロジェクトを一覧追加し、classify_batch を再invokeして再開する", async () => {
    let batchStep = 0;
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "classify_batch") {
        batchStep++;
        return Promise.resolve(
          batchStep === 1 ? paused("m1", 1, 2) : completed(2, 2),
        );
      }
      if (cmd === "approve_new_project")
        return Promise.resolve({
          id: "np",
          name: "New",
          account_id: "acc1",
          description: null,
          color: null,
        });
      return Promise.resolve();
    });

    await useClassifyStore.getState().classifyAll("acc1");
    expect(useClassifyStore.getState().pendingProposal?.mail_id).toBe("m1");

    await useClassifyStore.getState().approveNewProject("m1", "New");

    expect(useProjectStore.getState().projects.map((p) => p.id)).toContain(
      "np",
    );
    expect(batchCalls()).toHaveLength(2);
    expect(useClassifyStore.getState().pendingProposal).toBeNull();
    expect(useClassifyStore.getState().classifying).toBe(false);
  });

  it("rejectClassification で reject 後に classify_batch を再invokeして再開する", async () => {
    let batchStep = 0;
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "classify_batch") {
        batchStep++;
        return Promise.resolve(
          batchStep === 1 ? paused("m1", 1, 2) : completed(2, 2),
        );
      }
      return Promise.resolve();
    });

    await useClassifyStore.getState().classifyAll("acc1");
    await useClassifyStore.getState().rejectClassification("m1");

    expect(
      invokeMock.mock.calls.some((c) => c[0] === "reject_classification"),
    ).toBe(true);
    expect(batchCalls()).toHaveLength(2);
    expect(useClassifyStore.getState().pendingProposal).toBeNull();
    expect(useClassifyStore.getState().classifying).toBe(false);
  });

  it("cancelClassification は cancel_classification を invoke し、以降は再開しない", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "classify_batch") return Promise.resolve(paused("m1", 1, 3));
      return Promise.resolve();
    });

    await useClassifyStore.getState().classifyAll("acc1");
    await useClassifyStore.getState().cancelClassification();

    expect(
      invokeMock.mock.calls.filter((c) => c[0] === "cancel_classification"),
    ).toHaveLength(1);
    expect(useClassifyStore.getState().classifying).toBe(false);
    expect(useClassifyStore.getState().pendingProposal).toBeNull();

    // キャンセル済みなので reject しても classify_batch を再invokeしない
    const before = batchCalls().length;
    await useClassifyStore.getState().rejectClassification("m1");
    expect(batchCalls()).toHaveLength(before);
  });

  it("already_running は何もしない（進行中のバッチに任せる）", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "classify_batch")
        return Promise.resolve({ status: "already_running" });
      return Promise.resolve();
    });

    await useClassifyStore.getState().classifyAll("acc1");

    // 実行中のバッチが進行しているので classifying は立てたままにする
    expect(useClassifyStore.getState().classifying).toBe(true);
  });

  it("classify_batch のエラーで状態をリセットする", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "classify_batch")
        return Promise.reject(new Error("Ollama down"));
      return Promise.resolve();
    });

    await useClassifyStore.getState().classifyAll("acc1");

    expect(useClassifyStore.getState().classifying).toBe(false);
    expect(useClassifyStore.getState().progress).toBeNull();
  });

  it("classify-progress イベントで対象アカウントの進捗のみ反映する", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "classify_batch") return Promise.resolve(paused("m1", 1, 5));
      return Promise.resolve();
    });
    await useClassifyStore.getState().initProgressListener();
    await useClassifyStore.getState().classifyAll("acc1");

    progressHandler?.({
      payload: { account_id: "acc1", current: 2, total: 5 },
    });
    expect(useClassifyStore.getState().progress).toEqual({
      current: 2,
      total: 5,
    });

    // 別アカウントのイベントは無視する
    progressHandler?.({
      payload: { account_id: "other", current: 9, total: 9 },
    });
    expect(useClassifyStore.getState().progress).toEqual({
      current: 2,
      total: 5,
    });
  });

  // 未分類一覧に2件セットして分類バッチを開始する共通セットアップ。
  // 進捗イベント発火後に unclassifiedMails に残る ID を返す。
  const setupWithUnclassified = async () => {
    useMailStore.setState({
      unclassifiedMails: [
        { id: "mail-42" } as never,
        { id: "mail-99" } as never,
      ],
      unclassifiedThreads: [],
    });
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "classify_batch") return Promise.resolve(paused("m1", 1, 5));
      return Promise.resolve();
    });
    await useClassifyStore.getState().initProgressListener();
    await useClassifyStore.getState().classifyAll("acc1");
  };

  const remainingUnclassifiedIds = () =>
    useMailStore.getState().unclassifiedMails.map((m) => m.id);

  it("assigned_mail_id を含む進捗で該当メールを未分類一覧から消す", async () => {
    await setupWithUnclassified();

    progressHandler?.({
      payload: {
        account_id: "acc1",
        current: 2,
        total: 5,
        assigned_mail_id: "mail-42",
      },
    });

    expect(remainingUnclassifiedIds()).toEqual(["mail-99"]);
  });

  it("assigned_mail_id が null の進捗ではメールを消さない", async () => {
    await setupWithUnclassified();

    progressHandler?.({
      payload: {
        account_id: "acc1",
        current: 2,
        total: 5,
        assigned_mail_id: null,
      },
    });

    expect(remainingUnclassifiedIds()).toEqual(["mail-42", "mail-99"]);
  });

  it("別アカウントの assigned_mail_id では消さない", async () => {
    await setupWithUnclassified();

    progressHandler?.({
      payload: {
        account_id: "other",
        current: 2,
        total: 5,
        assigned_mail_id: "mail-42",
      },
    });

    expect(remainingUnclassifiedIds()).toEqual(["mail-42", "mail-99"]);
  });
});
