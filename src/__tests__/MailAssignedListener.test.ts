import { describe, it, expect, vi } from "vitest";
import { useMailStore } from "../stores/mailStore";
import { useProjectStore } from "../stores/projectStore";
import type { Mail } from "../types/mail";

function makeMail(id: string): Mail {
  return {
    id, account_id: "acc1", folder: "INBOX",
    message_id: `<${id}@example.com>`, in_reply_to: null, references: null,
    from_addr: "alice@example.com", to_addr: "bob@example.com",
    cc_addr: null, subject: `件名${id}`,
    body_text: "本文", body_html: null,
    date: "2026-07-24T10:00:00+09:00", has_attachments: false,
    raw_size: null, uid: 1, flags: null, is_read: false, is_flagged: false,
    fetched_at: "2026-07-24T00:00:00",
  };
}

describe("handleMailAssigned", () => {
  it("未分類リストから該当メールを除去する", () => {
    useMailStore.setState({
      unclassifiedMails: [makeMail("m1"), makeMail("m2")],
      unclassifiedThreads: [],
    });
    useProjectStore.setState({ selectedProjectId: null });

    useMailStore.getState().handleMailAssigned({ mail_id: "m1", project_id: "p1" });

    expect(useMailStore.getState().unclassifiedMails.map((m) => m.id)).toEqual(["m2"]);
  });

  it("割り当て先の案件を表示中ならスレッド一覧を取り直す", () => {
    const fetchThreadsByProject = vi.fn().mockResolvedValue(undefined);
    useMailStore.setState({
      unclassifiedMails: [],
      unclassifiedThreads: [],
      fetchThreadsByProject,
    });
    useProjectStore.setState({ selectedProjectId: "p1" });

    useMailStore.getState().handleMailAssigned({ mail_id: "m1", project_id: "p1" });

    expect(fetchThreadsByProject).toHaveBeenCalledWith("p1");
  });

  it("別の案件を表示中ならスレッド一覧は取り直さない", () => {
    const fetchThreadsByProject = vi.fn().mockResolvedValue(undefined);
    useMailStore.setState({
      unclassifiedMails: [],
      unclassifiedThreads: [],
      fetchThreadsByProject,
    });
    useProjectStore.setState({ selectedProjectId: "other" });

    useMailStore.getState().handleMailAssigned({ mail_id: "m1", project_id: "p1" });

    expect(fetchThreadsByProject).not.toHaveBeenCalled();
  });
});
