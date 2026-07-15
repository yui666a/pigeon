import { render, screen, waitFor, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { MailBody } from "../components/mail-view/MailBody";
import type { Mail } from "../types/mail";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  save: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-opener", () => ({
  openUrl: vi.fn(() => Promise.resolve()),
}));

const mockInvoke = vi.mocked(invoke);
const mockOpenUrl = vi.mocked(openUrl);

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

  // --- サニタイズの厳格化（フィッシングUI・UIリドレッシング対策） ---

  describe("HTMLサニタイズ", () => {
    function renderHtml(html: string) {
      return render(<MailBody mail={makeMail({ body_html: html })} />).container;
    }

    it("script要素を除去する", () => {
      const c = renderHtml('<p>hi</p><script>window.x=1</script>');
      expect(c.querySelector("script")).toBeNull();
    });

    it("form/input/button/textarea/selectを除去する（フィッシングUI対策）", () => {
      const c = renderHtml(
        '<form action="https://evil.example/steal"><input name="pw"><button>送信</button><textarea></textarea><select></select></form>',
      );
      for (const tag of ["form", "input", "button", "textarea", "select"]) {
        expect(c.querySelector(`.prose ${tag}`), tag).toBeNull();
      }
    });

    it("style要素とstyle属性を除去する（UIリドレッシング対策）", () => {
      const c = renderHtml(
        '<style>body{display:none}</style><div style="position:fixed;inset:0">overlay</div>',
      );
      expect(c.querySelector("style")).toBeNull();
      expect(c.querySelector("div[style]")).toBeNull();
      // テキスト自体は残る（除去するのは属性のみ）
      expect(c.textContent).toContain("overlay");
    });

    it("iframe/object/embedを除去する", () => {
      const c = renderHtml(
        '<iframe src="https://evil.example"></iframe><object data="x"></object><embed src="x">',
      );
      expect(c.querySelector("iframe, object, embed")).toBeNull();
    });

    it("リンクに rel=noopener noreferrer を付与し target を除去する", () => {
      const c = renderHtml('<a href="https://example.com" target="_blank">link</a>');
      const a = c.querySelector("a");
      expect(a?.getAttribute("rel")).toBe("noopener noreferrer");
      expect(a?.hasAttribute("target")).toBe(false);
    });
  });

  // --- リンククリック制御（Webview遷移の禁止・外部ブラウザ強制） ---

  describe("リンククリック", () => {
    function clickLink(html: string) {
      const { container } = render(<MailBody mail={makeMail({ body_html: html })} />);
      const anchor = container.querySelector("a");
      expect(anchor).toBeTruthy();
      fireEvent.click(anchor as HTMLAnchorElement);
    }

    beforeEach(() => {
      mockOpenUrl.mockClear();
    });

    it("httpsリンクは外部ブラウザで開きWebviewは遷移しない", () => {
      clickLink('<a href="https://example.com/page">link</a>');
      expect(mockOpenUrl).toHaveBeenCalledWith("https://example.com/page");
    });

    it("mailtoリンクは既定メールクライアントに渡す", () => {
      clickLink('<a href="mailto:someone@example.com">mail</a>');
      expect(mockOpenUrl).toHaveBeenCalledWith("mailto:someone@example.com");
    });

    it("カスタムスキームのリンクは開かない（deep-link悪用対策）", () => {
      clickLink('<a href="com.haiso666.pigeon://oauth/callback?x=1">deep</a>');
      expect(mockOpenUrl).not.toHaveBeenCalled();
    });

    it("相対URLのリンクは開かない", () => {
      clickLink('<a href="/local/path">rel</a>');
      expect(mockOpenUrl).not.toHaveBeenCalled();
    });
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
