import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { renderHook, act } from "@testing-library/react";
import { useBulkActions } from "../hooks/useBulkActions";
import { useMailStore } from "../stores/mailStore";
import { useSelectionStore } from "../stores/selectionStore";
import type { Mail, Thread } from "../types/mail";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(() => Promise.resolve(null)),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}));

const mail = (id: string): Mail => ({
  id,
  account_id: "acc1",
  folder: "INBOX",
  message_id: `<${id}@example.com>`,
  in_reply_to: null,
  references: null,
  from_addr: "a@example.com",
  to_addr: "b@example.com",
  cc_addr: null,
  subject: "件名",
  body_text: null,
  body_html: null,
  date: "2026-07-13T00:00:00Z",
  has_attachments: false,
  raw_size: null,
  uid: 1,
  flags: null,
  is_read: false,
  is_flagged: false,
  fetched_at: "2026-07-13T00:00:00Z",
});

const threads: Thread[] = [
  {
    thread_id: "t1",
    subject: "件名",
    last_date: "2026-07-13T00:00:00Z",
    mail_count: 2,
    from_addrs: ["a@example.com"],
    mails: [mail("m1"), mail("m2")],
  },
];

const bulkDeleteMails = vi.fn(() => Promise.resolve({ succeeded: ["m1", "m2"], failed: [] }));
const bulkArchiveMails = vi.fn(() => Promise.resolve({ succeeded: ["m1", "m2"], failed: [] }));
const bulkMoveMails = vi.fn(() => Promise.resolve({ succeeded: ["m1", "m2"], failed: [] }));

describe("useBulkActions", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useMailStore.setState({ bulkDeleteMails, bulkArchiveMails, bulkMoveMails });
    useSelectionStore.setState({ selectedThreadIds: new Set(["t1"]) });
    vi.spyOn(window, "confirm").mockReturnValue(true);
  });

  afterEach(() => {
    vi.restoreAllMocks();
    useSelectionStore.setState({ selectedThreadIds: new Set() });
  });

  it("削除は確認ダイアログの後に実行し、選択解除とリロードを行う", async () => {
    const reload = vi.fn();
    const { result } = renderHook(() =>
      useBulkActions({ accountId: "acc1", threads, reload }),
    );

    await act(() => result.current.handleBulkDelete());

    expect(window.confirm).toHaveBeenCalledWith(
      "選択した 1 スレッドを削除しますか？サーバーにゴミ箱があればゴミ箱へ移動し、無い場合は完全に削除されます。",
    );
    expect(bulkDeleteMails).toHaveBeenCalledWith("acc1", ["m1", "m2"]);
    expect(useSelectionStore.getState().selectedThreadIds.size).toBe(0);
    expect(reload).toHaveBeenCalledTimes(1);
  });

  it("確認ダイアログでキャンセルすると削除しない", async () => {
    vi.spyOn(window, "confirm").mockReturnValue(false);
    const reload = vi.fn();
    const { result } = renderHook(() =>
      useBulkActions({ accountId: "acc1", threads, reload }),
    );

    await act(() => result.current.handleBulkDelete());

    expect(bulkDeleteMails).not.toHaveBeenCalled();
    expect(reload).not.toHaveBeenCalled();
    expect(useSelectionStore.getState().selectedThreadIds.size).toBe(1);
  });

  it("アーカイブは確認なしで実行し、選択解除とリロードを行う", async () => {
    const reload = vi.fn();
    const { result } = renderHook(() =>
      useBulkActions({ accountId: "acc1", threads, reload }),
    );

    await act(() => result.current.handleBulkArchive());

    expect(window.confirm).not.toHaveBeenCalled();
    expect(bulkArchiveMails).toHaveBeenCalledWith("acc1", ["m1", "m2"]);
    expect(useSelectionStore.getState().selectedThreadIds.size).toBe(0);
    expect(reload).toHaveBeenCalledTimes(1);
  });

  it("案件への移動は accountId なしでも実行できる", async () => {
    const reload = vi.fn();
    const { result } = renderHook(() =>
      useBulkActions({ accountId: null, threads, reload }),
    );

    await act(() => result.current.handleBulkMove("p1"));

    expect(bulkMoveMails).toHaveBeenCalledWith(["m1", "m2"], "p1");
    expect(useSelectionStore.getState().selectedThreadIds.size).toBe(0);
    expect(reload).toHaveBeenCalledTimes(1);
  });

  it("accountId が無い場合は削除・アーカイブを実行しない", async () => {
    const reload = vi.fn();
    const { result } = renderHook(() =>
      useBulkActions({ accountId: null, threads, reload }),
    );

    await act(() => result.current.handleBulkDelete());
    await act(() => result.current.handleBulkArchive());

    expect(bulkDeleteMails).not.toHaveBeenCalled();
    expect(bulkArchiveMails).not.toHaveBeenCalled();
  });

  it("選択メールが 0 件なら何もしない", async () => {
    useSelectionStore.setState({ selectedThreadIds: new Set() });
    const reload = vi.fn();
    const { result } = renderHook(() =>
      useBulkActions({ accountId: "acc1", threads, reload }),
    );

    await act(() => result.current.handleBulkDelete());
    await act(() => result.current.handleBulkArchive());
    await act(() => result.current.handleBulkMove("p1"));

    expect(bulkDeleteMails).not.toHaveBeenCalled();
    expect(bulkArchiveMails).not.toHaveBeenCalled();
    expect(bulkMoveMails).not.toHaveBeenCalled();
    expect(reload).not.toHaveBeenCalled();
  });
});
