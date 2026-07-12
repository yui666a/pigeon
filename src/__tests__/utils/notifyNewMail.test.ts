import { describe, it, expect, vi, beforeEach } from "vitest";
import {
  notifyNewMail,
  isNotificationEnabled,
  isSubjectPreviewEnabled,
  buildNotificationBody,
  NOTIFY_NEW_MAIL_KEY,
  NOTIFY_SUBJECT_PREVIEW_KEY,
} from "../../utils/notifyNewMail";

const mockIsPermissionGranted = vi.fn();
const mockRequestPermission = vi.fn();
const mockSendNotification = vi.fn();
vi.mock("@tauri-apps/plugin-notification", () => ({
  isPermissionGranted: (...args: unknown[]) => mockIsPermissionGranted(...args),
  requestPermission: (...args: unknown[]) => mockRequestPermission(...args),
  sendNotification: (...args: unknown[]) => mockSendNotification(...args),
}));

const mockInvoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => mockInvoke(...args),
}));

describe("notifyNewMail", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    localStorage.clear();
  });

  it("sends a notification with the count when permission is already granted", async () => {
    mockIsPermissionGranted.mockResolvedValue(true);

    await notifyNewMail(3);

    expect(mockRequestPermission).not.toHaveBeenCalled();
    expect(mockSendNotification).toHaveBeenCalledWith({
      title: "Pigeon",
      body: "3件の新着メールを受信しました",
    });
  });

  it("requests permission when not yet granted and sends if granted", async () => {
    mockIsPermissionGranted.mockResolvedValue(false);
    mockRequestPermission.mockResolvedValue("granted");

    await notifyNewMail(1);

    expect(mockRequestPermission).toHaveBeenCalled();
    expect(mockSendNotification).toHaveBeenCalledWith({
      title: "Pigeon",
      body: "1件の新着メールを受信しました",
    });
  });

  it("silently skips when permission is denied", async () => {
    mockIsPermissionGranted.mockResolvedValue(false);
    mockRequestPermission.mockResolvedValue("denied");

    await expect(notifyNewMail(2)).resolves.toBeUndefined();

    expect(mockSendNotification).not.toHaveBeenCalled();
  });

  it("skips without even checking permission when disabled via localStorage", async () => {
    localStorage.setItem(NOTIFY_NEW_MAIL_KEY, "false");

    await notifyNewMail(5);

    expect(mockIsPermissionGranted).not.toHaveBeenCalled();
    expect(mockRequestPermission).not.toHaveBeenCalled();
    expect(mockSendNotification).not.toHaveBeenCalled();
  });

  it("swallows plugin errors instead of throwing", async () => {
    mockIsPermissionGranted.mockRejectedValue(new Error("plugin unavailable"));

    await expect(notifyNewMail(1)).resolves.toBeUndefined();

    expect(mockSendNotification).not.toHaveBeenCalled();
  });

  it("shows count only when accountId is given but preview is disabled (default) — no invoke", async () => {
    mockIsPermissionGranted.mockResolvedValue(true);

    await notifyNewMail(2, "acc1");

    expect(mockInvoke).not.toHaveBeenCalled();
    expect(mockSendNotification).toHaveBeenCalledWith({
      title: "Pigeon",
      body: "2件の新着メールを受信しました",
    });
  });

  it("shows count only when preview is enabled but accountId is omitted — no invoke", async () => {
    mockIsPermissionGranted.mockResolvedValue(true);
    localStorage.setItem(NOTIFY_SUBJECT_PREVIEW_KEY, "true");

    await notifyNewMail(2);

    expect(mockInvoke).not.toHaveBeenCalled();
    expect(mockSendNotification).toHaveBeenCalledWith({
      title: "Pigeon",
      body: "2件の新着メールを受信しました",
    });
  });

  it("fetches and shows subject preview when accountId is given and preview is enabled", async () => {
    mockIsPermissionGranted.mockResolvedValue(true);
    localStorage.setItem(NOTIFY_SUBJECT_PREVIEW_KEY, "true");
    mockInvoke.mockResolvedValue(["件名A", "件名B"]);

    await notifyNewMail(2, "acc1");

    expect(mockInvoke).toHaveBeenCalledWith("get_recent_unread_subjects", {
      accountId: "acc1",
      limit: 3,
    });
    expect(mockSendNotification).toHaveBeenCalledWith({
      title: "Pigeon",
      body: "件名A\n件名B",
    });
  });

  it("falls back to count-only when the subject fetch fails", async () => {
    mockIsPermissionGranted.mockResolvedValue(true);
    localStorage.setItem(NOTIFY_SUBJECT_PREVIEW_KEY, "true");
    mockInvoke.mockRejectedValue(new Error("db error"));

    await notifyNewMail(2, "acc1");

    expect(mockSendNotification).toHaveBeenCalledWith({
      title: "Pigeon",
      body: "2件の新着メールを受信しました",
    });
  });
});

describe("isNotificationEnabled", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it("is enabled by default (no localStorage key)", () => {
    expect(isNotificationEnabled()).toBe(true);
  });

  it("is disabled when the key is 'false'", () => {
    localStorage.setItem(NOTIFY_NEW_MAIL_KEY, "false");
    expect(isNotificationEnabled()).toBe(false);
  });

  it("is enabled for any other value", () => {
    localStorage.setItem(NOTIFY_NEW_MAIL_KEY, "true");
    expect(isNotificationEnabled()).toBe(true);
  });
});

describe("isSubjectPreviewEnabled", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it("is disabled by default (no localStorage key) — privacy-first default", () => {
    expect(isSubjectPreviewEnabled()).toBe(false);
  });

  it("is enabled only when the key is exactly 'true'", () => {
    localStorage.setItem(NOTIFY_SUBJECT_PREVIEW_KEY, "true");
    expect(isSubjectPreviewEnabled()).toBe(true);
  });

  it("is disabled for any other value", () => {
    localStorage.setItem(NOTIFY_SUBJECT_PREVIEW_KEY, "1");
    expect(isSubjectPreviewEnabled()).toBe(false);
  });
});

describe("buildNotificationBody", () => {
  it("shows count only when preview is disabled", () => {
    expect(buildNotificationBody(5, ["A", "B"], false)).toBe(
      "5件の新着メールを受信しました",
    );
  });

  it("shows count only when there are no subjects, even if preview is enabled", () => {
    expect(buildNotificationBody(3, [], true)).toBe(
      "3件の新着メールを受信しました",
    );
  });

  it("shows all subjects when count is within the preview limit", () => {
    expect(buildNotificationBody(2, ["件名A", "件名B"], true)).toBe(
      "件名A\n件名B",
    );
  });

  it("shows up to 3 subjects plus a remaining-count suffix", () => {
    const subjects = ["件名A", "件名B", "件名C", "件名D", "件名E"];
    expect(buildNotificationBody(5, subjects, true)).toBe(
      "件名A\n件名B\n件名C\n他2件",
    );
  });

  it("omits the remaining-count suffix when subjects exactly cover count", () => {
    const subjects = ["件名A", "件名B", "件名C"];
    expect(buildNotificationBody(3, subjects, true)).toBe(
      "件名A\n件名B\n件名C",
    );
  });
});
