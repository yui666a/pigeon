import { render, screen, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { MailBody } from "../components/mail-view/MailBody";
import type { Mail } from "../types/mail";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  save: vi.fn(),
}));

const mockInvoke = vi.mocked(invoke);

function makeMail(overrides: Partial<Mail> = {}): Mail {
  return {
    id: "m1", account_id: "acc1", folder: "INBOX",
    message_id: "<msg1@example.com>", in_reply_to: null, references: null,
    from_addr: "alice@example.com", to_addr: "bob@example.com",
    cc_addr: null, subject: "件名",
    body_text: "本文テキスト", body_html: null,
    date: "2026-07-12T10:00:00+09:00", has_attachments: false,
    raw_size: null, uid: 1, flags: null, is_read: false, is_flagged: false,
    fetched_at: "2026-07-12T00:00:00",
    ...overrides,
  };
}

describe("MailBody", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });

  it("renders body text", () => {
    render(<MailBody mail={makeMail()} />);
    expect(screen.getByText("本文テキスト")).toBeInTheDocument();
  });

  it("has_attachments のとき添付セクションを表示する", () => {
    render(<MailBody mail={makeMail({ has_attachments: true })} />);
    expect(
      screen.getByRole("button", { name: /添付ファイルを表示/ }),
    ).toBeInTheDocument();
  });

  it("添付がないメールでは添付セクションを表示しない", () => {
    render(<MailBody mail={makeMail({ has_attachments: false })} />);
    expect(
      screen.queryByRole("button", { name: /添付ファイルを表示/ }),
    ).not.toBeInTheDocument();
  });

  it("cid参照を含む本文はget_inline_imagesの結果でdata URIに置換される", async () => {
    mockInvoke.mockResolvedValueOnce([
      { content_id: "logo123@example.com", data_uri: "data:image/png;base64,AAAA" },
    ]);
    const { container } = render(
      <MailBody
        mail={makeMail({
          has_attachments: true,
          body_html: '<img src="cid:logo123@example.com" alt="logo">',
        })}
      />,
    );

    await waitFor(() => {
      expect(container.querySelector("img")?.getAttribute("src")).toBe(
        "data:image/png;base64,AAAA",
      );
    });
    expect(mockInvoke).toHaveBeenCalledWith("get_inline_images", { mailId: "m1" });
  });

  it("cid参照がない本文ではget_inline_imagesを呼ばない", () => {
    render(
      <MailBody
        mail={makeMail({ has_attachments: true, body_html: "<p>本文</p>" })}
      />,
    );
    expect(mockInvoke).not.toHaveBeenCalledWith("get_inline_images", expect.anything());
  });

  it("get_inline_imagesが失敗しても本文表示は壊れない", async () => {
    mockInvoke.mockRejectedValueOnce(new Error("network error"));
    render(
      <MailBody
        mail={makeMail({
          has_attachments: true,
          body_html: '<img src="cid:missing@example.com">',
        })}
      />,
    );
    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith("get_inline_images", { mailId: "m1" });
    });
    // cid未解決のままでもクラッシュしない
    expect(document.querySelector("img")).toBeTruthy();
  });
});
