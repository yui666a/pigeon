import { render, screen } from "@testing-library/react";
import { describe, it, expect } from "vitest";
import { AccountList } from "../components/sidebar/AccountList";
import type { Account } from "../types/account";

const baseAccount: Account = {
  id: "1",
  name: "Test Account",
  email: "test@example.com",
  imap_host: "imap.example.com",
  imap_port: 993,
  smtp_host: "smtp.example.com",
  smtp_port: 587,
  auth_type: "plain",
  provider: "other",
  needs_reauth: false,
  created_at: "2026-01-01T00:00:00Z",
};

describe("AccountList", () => {
  it("shows Google icon for google provider accounts", () => {
    const googleAccount: Account = {
      ...baseAccount,
      id: "2",
      name: "Gmail Account",
      email: "user@gmail.com",
      provider: "google",
      auth_type: "oauth2",
    };
    render(
      <AccountList
        accounts={[googleAccount]}
        selectedId={null}
        onSelect={() => {}}
      />,
    );
    expect(screen.getByTitle("Google")).toBeInTheDocument();
    expect(screen.getByText("G")).toBeInTheDocument();
  });

  it("does not show Google icon for non-google accounts", () => {
    render(
      <AccountList
        accounts={[baseAccount]}
        selectedId={null}
        onSelect={() => {}}
      />,
    );
    expect(screen.queryByTitle("Google")).not.toBeInTheDocument();
  });

  it("shows reauth warning when needs_reauth is true", () => {
    const reauthAccount: Account = {
      ...baseAccount,
      id: "3",
      provider: "google",
      auth_type: "oauth2",
      needs_reauth: true,
    };
    render(
      <AccountList
        accounts={[reauthAccount]}
        selectedId={null}
        onSelect={() => {}}
      />,
    );
    expect(screen.getByTitle("再認証が必要です")).toBeInTheDocument();
  });

  it("does not show reauth warning when needs_reauth is false", () => {
    render(
      <AccountList
        accounts={[baseAccount]}
        selectedId={null}
        onSelect={() => {}}
      />,
    );
    expect(screen.queryByTitle("再認証が必要です")).not.toBeInTheDocument();
  });

  it("shows empty message when no accounts", () => {
    render(
      <AccountList accounts={[]} selectedId={null} onSelect={() => {}} />,
    );
    expect(screen.getByText("アカウントなし")).toBeInTheDocument();
  });
});
