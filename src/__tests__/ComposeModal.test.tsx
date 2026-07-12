import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { ComposeModal } from "../components/compose/ComposeModal";
import { useComposeStore } from "../stores/composeStore";
import { useAccountStore } from "../stores/accountStore";
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

describe("ComposeModal", () => {
  beforeEach(() => {
    vi.clearAllMocks();
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
    });
    useAccountStore.setState({
      accounts: [makeAccount()],
      selectedAccountId: "acc1",
    });
  });

  it("renders nothing when closed", () => {
    const { container } = render(<ComposeModal />);
    expect(container.firstChild).toBeNull();
  });

  it("renders all fields when open", () => {
    useComposeStore.setState({ isOpen: true });
    render(<ComposeModal />);
    expect(screen.getByLabelText("To")).toBeInTheDocument();
    expect(screen.getByLabelText("Cc")).toBeInTheDocument();
    expect(screen.getByLabelText("Bcc")).toBeInTheDocument();
    expect(screen.getByLabelText("件名")).toBeInTheDocument();
    expect(screen.getByLabelText("本文")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "送信" })).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "キャンセル" }),
    ).toBeInTheDocument();
  });

  it("shows prefilled values from the store", () => {
    useComposeStore.setState({
      isOpen: true,
      to: "tanaka@example.com",
      subject: "Re: 打ち合わせの件",
      body: "> こんにちは。",
    });
    render(<ComposeModal />);
    expect(screen.getByLabelText("To")).toHaveValue("tanaka@example.com");
    expect(screen.getByLabelText("件名")).toHaveValue("Re: 打ち合わせの件");
    expect(screen.getByLabelText("本文")).toHaveValue("> こんにちは。");
  });

  it("updates store fields on input", () => {
    useComposeStore.setState({ isOpen: true });
    render(<ComposeModal />);
    fireEvent.change(screen.getByLabelText("To"), {
      target: { value: "a@ex.com" },
    });
    fireEvent.change(screen.getByLabelText("件名"), {
      target: { value: "件名A" },
    });
    fireEvent.change(screen.getByLabelText("本文"), {
      target: { value: "本文A" },
    });
    const s = useComposeStore.getState();
    expect(s.to).toBe("a@ex.com");
    expect(s.subject).toBe("件名A");
    expect(s.body).toBe("本文A");
  });

  it("invokes send_mail when 送信 is clicked", async () => {
    mockInvoke.mockResolvedValue(undefined);
    useComposeStore.setState({
      isOpen: true,
      to: "a@ex.com",
      subject: "S",
      body: "B",
    });
    render(<ComposeModal />);

    fireEvent.click(screen.getByRole("button", { name: "送信" }));

    await vi.waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith(
        "send_mail",
        expect.objectContaining({
          req: expect.objectContaining({
            account_id: "acc1",
            to: ["a@ex.com"],
            subject: "S",
            body_text: "B",
          }),
        }),
      );
    });
    expect(useComposeStore.getState().isOpen).toBe(false);
  });

  it("disables 送信 and shows spinner while sending", () => {
    useComposeStore.setState({ isOpen: true, sending: true });
    render(<ComposeModal />);
    expect(screen.getByRole("button", { name: "送信中" })).toBeDisabled();
    expect(screen.getByRole("status")).toBeInTheDocument();
  });

  it("disables 送信 when no account is selected", () => {
    useAccountStore.setState({ selectedAccountId: null });
    useComposeStore.setState({ isOpen: true, to: "a@ex.com" });
    render(<ComposeModal />);
    expect(screen.getByRole("button", { name: "送信" })).toBeDisabled();
  });

  it("closes on Escape", () => {
    useComposeStore.setState({ isOpen: true });
    render(<ComposeModal />);
    fireEvent.keyDown(document, { key: "Escape" });
    expect(useComposeStore.getState().isOpen).toBe(false);
  });

  it("does not close on Escape while sending", () => {
    useComposeStore.setState({ isOpen: true, sending: true });
    render(<ComposeModal />);
    fireEvent.keyDown(document, { key: "Escape" });
    expect(useComposeStore.getState().isOpen).toBe(true);
  });

  it("closes when キャンセル is clicked", async () => {
    mockInvoke.mockResolvedValue({
      id: "draft-1",
      account_id: "acc1",
      to_addr: "a@ex.com",
      cc_addr: "",
      bcc_addr: "",
      subject: "",
      body_text: "",
      in_reply_to: null,
      created_at: "2026-07-13T00:00:00Z",
      updated_at: "2026-07-13T00:00:00Z",
    });
    useComposeStore.setState({ isOpen: true, to: "a@ex.com" });
    render(<ComposeModal />);
    fireEvent.click(screen.getByRole("button", { name: "キャンセル" }));
    await vi.waitFor(() => {
      expect(useComposeStore.getState().isOpen).toBe(false);
    });
    expect(useComposeStore.getState().to).toBe("");
  });
});
