import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { ThreadItem } from "../components/thread-list/ThreadItem";
import type { Thread } from "../types/mail";

function makeThread(overrides: Partial<Thread> = {}): Thread {
  return {
    thread_id: "<thread-1@example.com>",
    subject: "テストスレッド",
    last_date: "2026-04-13T10:00:00+09:00",
    mail_count: 1,
    from_addrs: ["alice@example.com"],
    mails: [],
    ...overrides,
  };
}

describe("ThreadItem", () => {
  it("renders subject and date", () => {
    render(<ThreadItem thread={makeThread()} selected={false} onClick={vi.fn()} />);
    expect(screen.getByText("テストスレッド")).toBeInTheDocument();
    expect(screen.getByText("4/13")).toBeInTheDocument();
  });

  it("renders from addresses", () => {
    render(<ThreadItem thread={makeThread({ from_addrs: ["alice@example.com", "bob@example.com"] })} selected={false} onClick={vi.fn()} />);
    expect(screen.getByText("alice@example.com, bob@example.com")).toBeInTheDocument();
  });

  it("shows mail count badge when > 1", () => {
    render(<ThreadItem thread={makeThread({ mail_count: 5 })} selected={false} onClick={vi.fn()} />);
    expect(screen.getByText("5")).toBeInTheDocument();
  });

  it("hides mail count badge for single mail", () => {
    render(<ThreadItem thread={makeThread({ mail_count: 1 })} selected={false} onClick={vi.fn()} />);
    expect(screen.queryByText("1")).not.toBeInTheDocument();
  });

  it("applies selected style", () => {
    render(<ThreadItem thread={makeThread()} selected={true} onClick={vi.fn()} />);
    const button = screen.getByRole("button");
    expect(button.className).toContain("bg-blue-50");
  });

  it("calls onClick when clicked", () => {
    const onClick = vi.fn();
    render(<ThreadItem thread={makeThread()} selected={false} onClick={onClick} />);
    fireEvent.click(screen.getByRole("button"));
    expect(onClick).toHaveBeenCalledTimes(1);
  });
});
