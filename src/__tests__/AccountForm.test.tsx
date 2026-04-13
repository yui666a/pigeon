import { render, screen } from "@testing-library/react";
import { describe, it, expect } from "vitest";
import { AccountForm } from "../components/sidebar/AccountForm";

describe("AccountForm", () => {
  it("renders all required input fields", () => {
    render(<AccountForm onSubmit={() => {}} onCancel={() => {}} />);
    expect(screen.getByLabelText("アカウント名")).toBeInTheDocument();
    expect(screen.getByLabelText("メールアドレス")).toBeInTheDocument();
    expect(screen.getByLabelText("IMAPサーバー")).toBeInTheDocument();
    expect(screen.getByLabelText("SMTPサーバー")).toBeInTheDocument();
    expect(screen.getByLabelText("パスワード")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "追加" })).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "キャンセル" }),
    ).toBeInTheDocument();
  });
});
