import { describe, it, expect, vi, beforeEach } from "vitest";
import {
  notifyNewMail,
  isNotificationEnabled,
  NOTIFY_NEW_MAIL_KEY,
} from "../../utils/notifyNewMail";

const mockIsPermissionGranted = vi.fn();
const mockRequestPermission = vi.fn();
const mockSendNotification = vi.fn();
vi.mock("@tauri-apps/plugin-notification", () => ({
  isPermissionGranted: (...args: unknown[]) => mockIsPermissionGranted(...args),
  requestPermission: (...args: unknown[]) => mockRequestPermission(...args),
  sendNotification: (...args: unknown[]) => mockSendNotification(...args),
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
