import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, beforeEach } from "vitest";
import { NotificationToggle } from "../components/sidebar/NotificationToggle";
import { NOTIFY_NEW_MAIL_KEY } from "../utils/notifyNewMail";

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
});
