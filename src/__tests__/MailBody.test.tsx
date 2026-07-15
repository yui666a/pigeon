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

/** HTML本文を描画して隔離用iframeを取り出す */
function renderFrame(html: string): HTMLIFrameElement {
  const { container } = render(<MailBody mail={makeMail({ body_html: html })} />);
  const frame = container.querySelector("iframe");
  expect(frame).toBeTruthy();
  return frame as HTMLIFrameElement;
}

describe("MailBody", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });

  it("renders body text", () => {
    render(<MailBody mail={makeMail()} />);
    expect(screen.getByText("本文テキスト")).toBeInTheDocument();
  });

  it("テキスト本文のみのメールではiframeを使わない", () => {
    const { container } = render(<MailBody mail={makeMail()} />);
    expect(container.querySelector("iframe")).toBeNull();
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

  // --- sandbox iframe 隔離（DOMPurifyバイパス時にTauri IPCへ到達させない第3層） ---

  describe("sandbox iframe 隔離", () => {
    it("HTML本文はsrcdoc付きsandbox iframeで描画する", () => {
      const frame = renderFrame("<p>こんにちは</p>");
      expect(frame.getAttribute("srcdoc")).toContain("<p>こんにちは</p>");
    });

    it("sandboxはallow-same-originのみ（allow-scriptsを付けない）", () => {
      const frame = renderFrame("<p>x</p>");
      expect(frame.getAttribute("sandbox")).toBe("allow-same-origin");
    });

    it("本文はアプリ本体のDOMツリーに直接展開されない", () => {
      const { container } = render(
        <MailBody mail={makeMail({ body_html: "<p id='direct'>本文</p>" })} />,
      );
      // iframe の srcdoc 属性としてのみ存在し、親文書の要素にはならない
      expect(container.querySelector("#direct")).toBeNull();
    });
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
      expect(container.querySelector("iframe")?.getAttribute("srcdoc")).toContain(
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
  // iframe隔離後もサニタイズは第1層として維持される（MailBodyが適用していることの配線確認）

  describe("HTMLサニタイズ", () => {
    it("script要素を除去する", () => {
      const srcdoc = renderFrame('<p>hi</p><script>window.x=1</script>').getAttribute("srcdoc");
      expect(srcdoc).not.toContain("window.x=1");
    });

    it("form/input/button/textarea/selectを除去する（フィッシングUI対策）", () => {
      const srcdoc = renderFrame(
        '<form action="https://evil.example/steal"><input name="pw"><button>送信</button><textarea></textarea><select></select></form>',
      ).getAttribute("srcdoc") ?? "";
      for (const tag of ["<form", "<input", "<button", "<textarea", "<select"]) {
        expect(srcdoc, tag).not.toContain(tag);
      }
    });

    it("メール由来のstyle要素とstyle属性を除去する（UIリドレッシング対策）", () => {
      const srcdoc = renderFrame(
        '<style>body{display:none}</style><div style="position:fixed;inset:0">overlay</div>',
      ).getAttribute("srcdoc") ?? "";
      expect(srcdoc).not.toContain("display:none");
      expect(srcdoc).not.toContain("position:fixed");
      // テキスト自体は残る（除去するのは属性のみ）
      expect(srcdoc).toContain("overlay");
    });

    it("iframe/object/embedを除去する", () => {
      const srcdoc = renderFrame(
        '<iframe src="https://evil.example"></iframe><object data="x"></object><embed src="x">',
      ).getAttribute("srcdoc") ?? "";
      expect(srcdoc).not.toContain("evil.example");
      expect(srcdoc).not.toContain("<object");
      expect(srcdoc).not.toContain("<embed");
    });

    it("リンクに rel=noopener noreferrer を付与し target を除去する", () => {
      const srcdoc = renderFrame(
        '<a href="https://example.com" target="_blank">link</a>',
      ).getAttribute("srcdoc") ?? "";
      expect(srcdoc).toContain('rel="noopener noreferrer"');
      expect(srcdoc).not.toContain("_blank");
    });
  });

  // --- リンククリック制御(Webview遷移の禁止・外部ブラウザ強制) ---
  // jsdomはsrcdocをロードしないため、iframeのcontentDocumentにアンカーを注入して
  // MailBodyが張ったクリックリスナーの挙動を検証する

  describe("リンククリック", () => {
    function clickLinkInFrame(href: string) {
      const frame = renderFrame("<p>本文</p>");
      const doc = frame.contentDocument;
      expect(doc).toBeTruthy();
      const anchor = doc!.createElement("a");
      anchor.setAttribute("href", href);
      anchor.textContent = "link";
      doc!.body.appendChild(anchor);
      fireEvent.click(anchor);
    }

    beforeEach(() => {
      mockOpenUrl.mockClear();
    });

    it("httpsリンクは外部ブラウザで開きWebviewは遷移しない", () => {
      clickLinkInFrame("https://example.com/page");
      expect(mockOpenUrl).toHaveBeenCalledWith("https://example.com/page");
    });

    it("mailtoリンクは既定メールクライアントに渡す", () => {
      clickLinkInFrame("mailto:someone@example.com");
      expect(mockOpenUrl).toHaveBeenCalledWith("mailto:someone@example.com");
    });

    it("カスタムスキームのリンクは開かない（deep-link悪用対策）", () => {
      clickLinkInFrame("com.haiso666.pigeon://oauth/callback?x=1");
      expect(mockOpenUrl).not.toHaveBeenCalled();
    });

    it("相対URLのリンクは開かない", () => {
      clickLinkInFrame("/local/path");
      expect(mockOpenUrl).not.toHaveBeenCalled();
    });
  });

  it("get_inline_imagesが失敗しても本文表示は壊れない", async () => {
    mockInvoke.mockRejectedValueOnce(new Error("network error"));
    const { container } = render(
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
    expect(
      container.querySelector("iframe")?.getAttribute("srcdoc"),
    ).toContain("cid:missing@example.com");
  });
});
