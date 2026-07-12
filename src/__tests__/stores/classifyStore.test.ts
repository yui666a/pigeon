import { describe, it, expect, vi, beforeEach } from "vitest";

const invokeMock = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));
// listen はもう使わないが、他モジュールが読む可能性に備えてスタブ
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));

import { useClassifyStore } from "../../stores/classifyStore";
import { useProjectStore } from "../../stores/projectStore";

const resetStore = () =>
  useClassifyStore.setState({
    classifying: false,
    progress: null,
    pendingProposal: null,
    error: null,
  });

beforeEach(() => {
  invokeMock.mockReset();
  resetStore();
  useProjectStore.setState({ projects: [] });
});

// classify_mail は Rust の ClassifyResponse を返す。mail_id と ClassifyResult が
// 両方とも #[serde(flatten)] されているため、実際のJSONは
// { mail_id, action, confidence, reason, ... } の完全にフラットな形になる。
// テストのモックも実際のワイヤーフォーマットに合わせる。
const resp = (mailId: string, action: string, extra: object = {}) => ({
  mail_id: mailId,
  action,
  confidence: 0.9,
  reason: "r",
  ...extra,
});

describe("classifyStore sequential flow", () => {
  it("assign結果は自動で次のメールへ進む", async () => {
    invokeMock.mockImplementation((cmd: string, args: { mailId?: string }) => {
      if (cmd === "get_unclassified_mails")
        return Promise.resolve([{ id: "m1" }, { id: "m2" }]);
      if (cmd === "classify_mail")
        return Promise.resolve(resp(args.mailId as string, "assign", { project_id: "p1" }));
      return Promise.resolve();
    });
    await useClassifyStore.getState().classifyAll("acc1");
    // 2件とも classify_mail が呼ばれ、停止していない
    const calls = invokeMock.mock.calls.filter((c) => c[0] === "classify_mail");
    expect(calls).toHaveLength(2);
    expect(useClassifyStore.getState().pendingProposal).toBeNull();
    expect(useClassifyStore.getState().classifying).toBe(false);
  });

  it("create結果で停止し pendingProposal に1件セットする", async () => {
    invokeMock.mockImplementation((cmd: string, args: { mailId?: string }) => {
      if (cmd === "get_unclassified_mails")
        return Promise.resolve([{ id: "m1" }, { id: "m2" }]);
      if (cmd === "classify_mail")
        return Promise.resolve(
          resp(args.mailId as string, "create", { project_name: "New", description: "d" }),
        );
      return Promise.resolve();
    });
    await useClassifyStore.getState().classifyAll("acc1");
    // m1 で create → 停止。classify_mail は1回だけ。
    const calls = invokeMock.mock.calls.filter((c) => c[0] === "classify_mail");
    expect(calls).toHaveLength(1);
    expect(useClassifyStore.getState().pendingProposal?.mail_id).toBe("m1");
    expect(useClassifyStore.getState().classifying).toBe(true);
  });

  it("approveNewProject でプロジェクトを一覧追加し次へ進む", async () => {
    let step = 0;
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "get_unclassified_mails")
        return Promise.resolve([{ id: "m1" }, { id: "m2" }]);
      if (cmd === "classify_mail") {
        step++;
        // m1 は create、m2 は assign
        return Promise.resolve(
          step === 1
            ? resp("m1", "create", { project_name: "New" })
            : resp("m2", "assign", { project_id: "np" }),
        );
      }
      if (cmd === "approve_new_project")
        return Promise.resolve({ id: "np", name: "New", account_id: "acc1", description: null, color: null });
      return Promise.resolve();
    });
    await useClassifyStore.getState().classifyAll("acc1");
    expect(useClassifyStore.getState().pendingProposal?.mail_id).toBe("m1");

    await useClassifyStore.getState().approveNewProject("m1", "New");
    // 新プロジェクトが一覧に入った
    expect(useProjectStore.getState().projects.map((p) => p.id)).toContain("np");
    // pending クリア、m2 まで進んで完了
    expect(useClassifyStore.getState().pendingProposal).toBeNull();
    expect(useClassifyStore.getState().classifying).toBe(false);
  });

  it("rejectClassification で次へ進む（未分類のまま）", async () => {
    let step = 0;
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "get_unclassified_mails")
        return Promise.resolve([{ id: "m1" }, { id: "m2" }]);
      if (cmd === "classify_mail") {
        step++;
        return Promise.resolve(
          step === 1 ? resp("m1", "create", { project_name: "New" }) : resp("m2", "unclassified"),
        );
      }
      return Promise.resolve();
    });
    await useClassifyStore.getState().classifyAll("acc1");
    await useClassifyStore.getState().rejectClassification("m1");
    expect(invokeMock.mock.calls.some((c) => c[0] === "reject_classification")).toBe(true);
    expect(useClassifyStore.getState().pendingProposal).toBeNull();
    expect(useClassifyStore.getState().classifying).toBe(false);
  });

  it("cancelClassification 後は次を分類しない", async () => {
    invokeMock.mockImplementation((cmd: string, args: { mailId?: string }) => {
      if (cmd === "get_unclassified_mails")
        return Promise.resolve([{ id: "m1" }, { id: "m2" }, { id: "m3" }]);
      if (cmd === "classify_mail") {
        // m1 を create にして停止させ、その隙にキャンセル
        return Promise.resolve(resp(args.mailId as string, "create", { project_name: "N" }));
      }
      return Promise.resolve();
    });
    await useClassifyStore.getState().classifyAll("acc1");
    await useClassifyStore.getState().cancelClassification();
    const before = invokeMock.mock.calls.filter((c) => c[0] === "classify_mail").length;
    // reject して次へ進もうとしてもキャンセル済みなので進まない
    await useClassifyStore.getState().rejectClassification("m1");
    const after = invokeMock.mock.calls.filter((c) => c[0] === "classify_mail").length;
    expect(after).toBe(before);
    expect(useClassifyStore.getState().classifying).toBe(false);
  });
});
