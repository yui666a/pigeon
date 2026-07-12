import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, beforeEach, vi, afterEach } from "vitest";
import { MailActions } from "../components/mail-view/MailActions";
import { useComposeStore } from "../stores/composeStore";
import { useAccountStore } from "../stores/accountStore";
import { useMailStore } from "../stores/mailStore";
import type { Mail } from "../types/mail";

function makeMail(): Mail {
  return {
    id: "m1",
    account_id: "acc1",
    folder: "INBOX",
    message_id: "<orig@ex.com>",
    in_reply_to: null,
    references: null,
    from_addr: "tanaka@example.com",
    to_addr: "me@example.com",
    cc_addr: null,
    subject: "打ち合わせの件",
    body_text: "こんにちは。",
    body_html: null,
    date: "2026-07-10T10:00:00Z",
    has_attachments: false,
    raw_size: null,
    uid: 1,
    flags: null,
    is_read: false,
    is_flagged: false,
    fetched_at: "2026-07-10T10:00:00Z",
  };
}

describe("MailActions", () => {
  beforeEach(() => {
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
    useAccountStore.setState({ accounts: [], selectedAccountId: null });
  });

  it("renders reply, reply-all and forward buttons", () => {
    render(<MailActions mail={makeMail()} />);
    expect(screen.getByRole("button", { name: "返信" })).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "全員に返信" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "転送" })).toBeInTheDocument();
  });

  it("opens compose in reply mode with the mail", () => {
    render(<MailActions mail={makeMail()} />);
    fireEvent.click(screen.getByRole("button", { name: "返信" }));
    const s = useComposeStore.getState();
    expect(s.isOpen).toBe(true);
    expect(s.mode).toBe("reply");
    expect(s.to).toBe("tanaka@example.com");
    expect(s.replyToMailId).toBe("m1");
  });

  it("opens compose in replyAll mode", () => {
    render(<MailActions mail={makeMail()} />);
    fireEvent.click(screen.getByRole("button", { name: "全員に返信" }));
    expect(useComposeStore.getState().mode).toBe("replyAll");
  });

  it("opens compose in forward mode without replyToMailId", () => {
    render(<MailActions mail={makeMail()} />);
    fireEvent.click(screen.getByRole("button", { name: "転送" }));
    const s = useComposeStore.getState();
    expect(s.mode).toBe("forward");
    expect(s.replyToMailId).toBeNull();
    expect(s.subject).toBe("Fwd: 打ち合わせの件");
  });

  describe("archive and delete", () => {
    const archiveMail = vi.fn();
    const deleteMail = vi.fn();
    const unarchiveMail = vi.fn();

    beforeEach(() => {
      archiveMail.mockReset();
      deleteMail.mockReset();
      unarchiveMail.mockReset();
      useMailStore.setState({ archiveMail, deleteMail, unarchiveMail });
    });

    afterEach(() => {
      vi.restoreAllMocks();
    });

    it("renders archive and delete buttons", () => {
      render(<MailActions mail={makeMail()} />);
      expect(
        screen.getByRole("button", { name: "アーカイブ" }),
      ).toBeInTheDocument();
      expect(screen.getByRole("button", { name: "削除" })).toBeInTheDocument();
    });

    it("archives the mail without confirmation", () => {
      render(<MailActions mail={makeMail()} />);
      fireEvent.click(screen.getByRole("button", { name: "アーカイブ" }));
      expect(archiveMail).toHaveBeenCalledWith(
        expect.objectContaining({ id: "m1" }),
      );
    });

    it("deletes the mail after the user confirms", () => {
      vi.spyOn(window, "confirm").mockReturnValue(true);
      render(<MailActions mail={makeMail()} />);
      fireEvent.click(screen.getByRole("button", { name: "削除" }));
      expect(window.confirm).toHaveBeenCalled();
      expect(deleteMail).toHaveBeenCalledWith(
        expect.objectContaining({ id: "m1" }),
      );
    });

    it("does not delete when the user cancels the confirmation", () => {
      vi.spyOn(window, "confirm").mockReturnValue(false);
      render(<MailActions mail={makeMail()} />);
      fireEvent.click(screen.getByRole("button", { name: "削除" }));
      expect(deleteMail).not.toHaveBeenCalled();
    });

    it("does not show the unarchive button for a non-archived mail", () => {
      render(<MailActions mail={makeMail()} />);
      expect(
        screen.queryByRole("button", { name: "アーカイブ解除" }),
      ).not.toBeInTheDocument();
    });
  });

  describe("star/flag toggle", () => {
    const toggleFlagged = vi.fn();

    beforeEach(() => {
      toggleFlagged.mockReset();
      useMailStore.setState({ toggleFlagged });
    });

    it("shows an unflagged star (☆) for a mail without is_flagged", () => {
      render(<MailActions mail={makeMail()} />);
      expect(screen.getByRole("button", { name: "☆" })).toBeInTheDocument();
    });

    it("shows a flagged star (★) for a flagged mail", () => {
      render(<MailActions mail={{ ...makeMail(), is_flagged: true }} />);
      expect(screen.getByRole("button", { name: "★" })).toBeInTheDocument();
    });

    it("toggles the flag on click", () => {
      const mail = makeMail();
      render(<MailActions mail={mail} />);
      fireEvent.click(screen.getByRole("button", { name: "☆" }));
      expect(toggleFlagged).toHaveBeenCalledWith(
        expect.objectContaining({ id: "m1" }),
      );
    });
  });

  describe("unarchive (folder='Archive')", () => {
    const unarchiveMail = vi.fn();
    const archiveMail = vi.fn();

    function makeArchivedMail(): Mail {
      return { ...makeMail(), folder: "Archive" };
    }

    beforeEach(() => {
      unarchiveMail.mockReset();
      archiveMail.mockReset();
      useMailStore.setState({ unarchiveMail, archiveMail });
    });

    it("shows unarchive instead of archive for an archived mail", () => {
      render(<MailActions mail={makeArchivedMail()} />);
      expect(
        screen.getByRole("button", { name: "アーカイブ解除" }),
      ).toBeInTheDocument();
      expect(
        screen.queryByRole("button", { name: "アーカイブ" }),
      ).not.toBeInTheDocument();
    });

    it("unarchives the mail on click", () => {
      render(<MailActions mail={makeArchivedMail()} />);
      fireEvent.click(screen.getByRole("button", { name: "アーカイブ解除" }));
      expect(unarchiveMail).toHaveBeenCalledWith(
        expect.objectContaining({ id: "m1", folder: "Archive" }),
      );
      expect(archiveMail).not.toHaveBeenCalled();
    });
  });
});
