import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { AttachmentList } from "../components/mail-view/AttachmentList";
import { useErrorStore } from "../stores/errorStore";
import type { Attachment } from "../types/attachment";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  save: vi.fn(),
}));

import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";

const mockInvoke = vi.mocked(invoke);
const mockSave = vi.mocked(save);

function makeAttachment(overrides: Partial<Attachment> = {}): Attachment {
  return {
    id: "att1",
    mail_id: "m1",
    filename: "report.pdf",
    mime_type: "application/pdf",
    size: 2048,
    file_path: "/cache/m1/report.pdf",
    content_id: null,
    ...overrides,
  };
}

describe("AttachmentList", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useErrorStore.setState({ toasts: [] });
  });

  it("最初は表示ボタンだけを描画する", () => {
    render(<AttachmentList mailId="m1" />);
    expect(
      screen.getByRole("button", { name: /添付ファイルを表示/ }),
    ).toBeInTheDocument();
    expect(mockInvoke).not.toHaveBeenCalled();
  });

  it("ボタン押下で list_attachments を呼び一覧を表示する", async () => {
    mockInvoke.mockResolvedValueOnce([
      makeAttachment(),
      makeAttachment({ id: "att2", filename: "pic.png", size: 512 }),
    ]);
    render(<AttachmentList mailId="m1" />);

    fireEvent.click(screen.getByRole("button", { name: /添付ファイルを表示/ }));

    expect(mockInvoke).toHaveBeenCalledWith("list_attachments", {
      mailId: "m1",
    });
    expect(await screen.findByText("report.pdf")).toBeInTheDocument();
    expect(screen.getByText("pic.png")).toBeInTheDocument();
    expect(screen.getByText("2.0 KB")).toBeInTheDocument();
    expect(screen.getByText("512 B")).toBeInTheDocument();
  });

  it("取得中はローディングを表示する", async () => {
    let resolve!: (v: Attachment[]) => void;
    mockInvoke.mockReturnValueOnce(
      new Promise<Attachment[]>((r) => {
        resolve = r;
      }) as Promise<unknown>,
    );
    render(<AttachmentList mailId="m1" />);

    fireEvent.click(screen.getByRole("button", { name: /添付ファイルを表示/ }));
    expect(await screen.findByText(/取得中/)).toBeInTheDocument();

    resolve([]);
    expect(
      await screen.findByText(/添付ファイルはありません/),
    ).toBeInTheDocument();
  });

  it("保存ボタンでダイアログを開き save_attachment を呼ぶ", async () => {
    mockInvoke.mockResolvedValueOnce([makeAttachment()]);
    mockSave.mockResolvedValueOnce("/Users/me/Downloads/report.pdf");
    mockInvoke.mockResolvedValueOnce(undefined);
    render(<AttachmentList mailId="m1" />);

    fireEvent.click(screen.getByRole("button", { name: /添付ファイルを表示/ }));
    fireEvent.click(await screen.findByRole("button", { name: "保存" }));

    await waitFor(() => {
      expect(mockSave).toHaveBeenCalledWith({ defaultPath: "report.pdf" });
    });
    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith("save_attachment", {
        attachmentId: "att1",
        destPath: "/Users/me/Downloads/report.pdf",
      });
    });
  });

  it("保存ダイアログをキャンセルしたら save_attachment を呼ばない", async () => {
    mockInvoke.mockResolvedValueOnce([makeAttachment()]);
    mockSave.mockResolvedValueOnce(null);
    render(<AttachmentList mailId="m1" />);

    fireEvent.click(screen.getByRole("button", { name: /添付ファイルを表示/ }));
    fireEvent.click(await screen.findByRole("button", { name: "保存" }));

    await waitFor(() => {
      expect(mockSave).toHaveBeenCalled();
    });
    expect(mockInvoke).toHaveBeenCalledTimes(1); // list_attachments のみ
  });

  it("取得に失敗したらエラーを通知しボタンに戻る", async () => {
    mockInvoke.mockRejectedValueOnce("IMAP error: connection failed");
    render(<AttachmentList mailId="m1" />);

    fireEvent.click(screen.getByRole("button", { name: /添付ファイルを表示/ }));

    await waitFor(() => {
      expect(useErrorStore.getState().toasts.length).toBeGreaterThan(0);
    });
    expect(
      screen.getByRole("button", { name: /添付ファイルを表示/ }),
    ).toBeInTheDocument();
  });
});
