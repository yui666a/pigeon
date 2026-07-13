import { describe, it, expect } from "vitest";
import { render } from "@testing-library/react";
import { TrashIcon } from "../components/common/icons/TrashIcon";

describe("TrashIcon", () => {
  it("装飾用アイコンとして aria-hidden 付きの svg を描画する", () => {
    const { container } = render(<TrashIcon />);
    const svg = container.querySelector("svg");
    expect(svg).not.toBeNull();
    expect(svg).toHaveAttribute("aria-hidden", "true");
  });

  it("className でサイズを指定できる", () => {
    const { container } = render(<TrashIcon className="h-3.5 w-3.5" />);
    const svg = container.querySelector("svg");
    expect(svg?.getAttribute("class")).toBe("h-3.5 w-3.5");
  });
});
