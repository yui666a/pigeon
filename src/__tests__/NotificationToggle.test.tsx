import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, beforeEach } from "vitest";
import { NotificationToggle } from "../components/sidebar/NotificationToggle";
import {
  NOTIFY_NEW_MAIL_KEY,
  NOTIFY_SUBJECT_PREVIEW_KEY,
} from "../utils/notifyNewMail";

describe("NotificationToggle", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it("is checked by default (no localStorage key)", () => {
    render(<NotificationToggle />);
    expect(
      screen.getByRole("checkbox", { name: "新着メールのデスクトップ通知" }),
    ).toBeChecked();
  });

  it("is unchecked when localStorage has 'false'", () => {
    localStorage.setItem(NOTIFY_NEW_MAIL_KEY, "false");
    render(<NotificationToggle />);
    expect(
      screen.getByRole("checkbox", { name: "新着メールのデスクトップ通知" }),
    ).not.toBeChecked();
  });

  it("is checked for any value other than 'false'", () => {
    localStorage.setItem(NOTIFY_NEW_MAIL_KEY, "true");
    render(<NotificationToggle />);
    expect(
      screen.getByRole("checkbox", { name: "新着メールのデスクトップ通知" }),
    ).toBeChecked();
  });

  it("writes 'false' to localStorage when turned off", () => {
    render(<NotificationToggle />);
    const checkbox = screen.getByRole("checkbox", {
      name: "新着メールのデスクトップ通知",
    });

    fireEvent.click(checkbox);

    expect(checkbox).not.toBeChecked();
    expect(localStorage.getItem(NOTIFY_NEW_MAIL_KEY)).toBe("false");
  });

  it("removes the key from localStorage when turned back on (default ON)", () => {
    localStorage.setItem(NOTIFY_NEW_MAIL_KEY, "false");
    render(<NotificationToggle />);
    const checkbox = screen.getByRole("checkbox", {
      name: "新着メールのデスクトップ通知",
    });

    fireEvent.click(checkbox);

    expect(checkbox).toBeChecked();
    expect(localStorage.getItem(NOTIFY_NEW_MAIL_KEY)).toBeNull();
  });

  describe("件名プレビュー", () => {
    it("is unchecked by default (privacy-first default OFF)", () => {
      render(<NotificationToggle />);
      expect(
        screen.getByRole("checkbox", { name: "通知に件名を表示" }),
      ).not.toBeChecked();
    });

    it("is checked when localStorage has 'true'", () => {
      localStorage.setItem(NOTIFY_SUBJECT_PREVIEW_KEY, "true");
      render(<NotificationToggle />);
      expect(
        screen.getByRole("checkbox", { name: "通知に件名を表示" }),
      ).toBeChecked();
    });

    it("writes 'true' to localStorage when turned on", () => {
      render(<NotificationToggle />);
      const checkbox = screen.getByRole("checkbox", {
        name: "通知に件名を表示",
      });

      fireEvent.click(checkbox);

      expect(checkbox).toBeChecked();
      expect(localStorage.getItem(NOTIFY_SUBJECT_PREVIEW_KEY)).toBe("true");
    });

    it("removes the key from localStorage when turned back off (default OFF)", () => {
      localStorage.setItem(NOTIFY_SUBJECT_PREVIEW_KEY, "true");
      render(<NotificationToggle />);
      const checkbox = screen.getByRole("checkbox", {
        name: "通知に件名を表示",
      });

      fireEvent.click(checkbox);

      expect(checkbox).not.toBeChecked();
      expect(localStorage.getItem(NOTIFY_SUBJECT_PREVIEW_KEY)).toBeNull();
    });

    it("is hidden when the main notification toggle is off", () => {
      localStorage.setItem(NOTIFY_NEW_MAIL_KEY, "false");
      render(<NotificationToggle />);
      expect(
        screen.queryByRole("checkbox", { name: "通知に件名を表示" }),
      ).not.toBeInTheDocument();
    });
  });
});
