import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { DraftList } from "../components/thread-list/DraftList";
import { useDraftStore } from "../stores/draftStore";
import { useComposeStore } from "../stores/composeStore";
import { useAccountStore } from "../stores/accountStore";
import type { Draft } from "../types/mail";
import type { Account } from "../types/account";

const mockInvoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => mockInvoke(...args),
}));

function makeAccount(): Account {
  return {
    id: "acc1",
    name: "Hiroshi",
    email: "me@example.com",
    imap_host: "imap.example.com",
    imap_port: 993,
    smtp_host: "smtp.example.com",
    smtp_port: 587,
    auth_type: "plain",
    provider: "other",
    needs_reauth: false,
    created_at: "2026-07-12T00:00:00Z",
  };
}

function makeDraft(overrides: Partial<Draft> = {}): Draft {
  return {
    id: "d1",
    account_id: "acc1",
    to_addr: "tanaka@example.com",
    cc_addr: "",
    bcc_addr: "",
    subject: "見積もりの件",
    body_text: "本文です",
    in_reply_to: null,
    created_at: "2026-07-12T10:00:00Z",
    updated_at: "2026-07-12T11:00:00Z",
    ...overrides,
  };
}

describe("DraftList", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    // 個別テストで上書きしない限り get_drafts は空配列を返す
    // （未モックの vi.fn() は undefined を返し、drafts を undefined で
    //   上書きしてクラッシュさせるため、既定値を明示する）
    mockInvoke.mockResolvedValue([]);
    useDraftStore.setState({ drafts: [], loading: false });
    useAccountStore.setState({
      accounts: [makeAccount()],
      selectedAccountId: "acc1",
    });
    useComposeStore.setState({
      isOpen: false,
      mode: "new",
      to: "",
      cc: "",
      bcc: "",
      subject: "",
      body: "",
      sending: false,
      replyToMailId: null,
      draftId: null,
    });
  });

  it("fetches drafts for the selected account on mount", async () => {
    mockInvoke.mockResolvedValue([makeDraft()]);
    render(<DraftList />);
    // store への反映まで待つ（invoke呼び出しだけを待つとアンマウント後に
    // setState が走り、次のテストで未定義参照エラーになる）
    await waitFor(() => {
      expect(useDraftStore.getState().drafts).toHaveLength(1);
    });
    expect(mockInvoke).toHaveBeenCalledWith("get_drafts", {
      accountId: "acc1",
    });
  });

  it("renders subject and recipient preview for each draft", () => {
    useDraftStore.setState({ drafts: [makeDraft()] });
    render(<DraftList />);
    expect(screen.getByText("見積もりの件")).toBeInTheDocument();
    expect(screen.getByText(/tanaka@example.com/)).toBeInTheDocument();
  });

  it("shows a placeholder for a draft with no subject", () => {
    useDraftStore.setState({ drafts: [makeDraft({ subject: "" })] });
    render(<DraftList />);
    expect(screen.getByText("(件名なし)")).toBeInTheDocument();
  });

  it("opens ComposeModal restored from the draft when clicked", () => {
    useDraftStore.setState({ drafts: [makeDraft()] });
    render(<DraftList />);
    fireEvent.click(screen.getByText("見積もりの件"));

    const s = useComposeStore.getState();
    expect(s.isOpen).toBe(true);
    expect(s.to).toBe("tanaka@example.com");
    expect(s.subject).toBe("見積もりの件");
    expect(s.draftId).toBe("d1");
  });

  it("deletes the draft when the delete button is clicked", async () => {
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === "get_drafts") return Promise.resolve([makeDraft()]);
      if (cmd === "delete_draft") return Promise.resolve(undefined);
      return Promise.reject(new Error(`unexpected: ${cmd}`));
    });
    useDraftStore.setState({ drafts: [makeDraft()] });
    render(<DraftList />);

    fireEvent.click(screen.getByRole("button", { name: "削除" }));

    // 一覧から消えるまで待つ（store更新の反映を確認する）
    await waitFor(() => {
      expect(screen.queryByText("見積もりの件")).not.toBeInTheDocument();
    });
    expect(mockInvoke).toHaveBeenCalledWith("delete_draft", { id: "d1" });
  });

  it("shows an empty state when there are no drafts", () => {
    useDraftStore.setState({ drafts: [] });
    render(<DraftList />);
    expect(screen.getByText("下書きはありません")).toBeInTheDocument();
  });
});
