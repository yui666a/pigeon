import { describe, it, expect } from "vitest";
import { effectiveAllow, planToggle } from "../../utils/cloudPolicy";
import type { CloudRule } from "../../types/directory";

function rule(scope: "directory" | "file", path: string, allow: boolean): CloudRule {
  return { id: `r-${scope}-${path}`, directory_id: "d1", scope, relative_path: path, allow };
}

describe("effectiveAllow", () => {
  it("returns false when no rules match (deny by default)", () => {
    expect(effectiveAllow([], "図面/平面図.pdf")).toBe(false);
  });

  it("directory allow covers children but not lookalike prefixes", () => {
    const rules = [rule("directory", "図面", true)];
    expect(effectiveAllow(rules, "図面/平面図.pdf")).toBe(true);
    expect(effectiveAllow(rules, "図面/sub/詳細.pdf")).toBe(true);
    expect(effectiveAllow(rules, "図面")).toBe(true);
    expect(effectiveAllow(rules, "契約/見積.pdf")).toBe(false);
    expect(effectiveAllow(rules, "図面外.txt")).toBe(false);
  });

  it("root directory rule ('') covers everything", () => {
    const rules = [rule("directory", "", true)];
    expect(effectiveAllow(rules, "anything.txt")).toBe(true);
    expect(effectiveAllow(rules, "a/b/c.txt")).toBe(true);
  });

  it("explicit file deny beats parent allow (longest match wins)", () => {
    const rules = [rule("directory", "", true), rule("file", "予算メモ.md", false)];
    expect(effectiveAllow(rules, "他.txt")).toBe(true);
    expect(effectiveAllow(rules, "予算メモ.md")).toBe(false);
  });

  it("file scope requires exact match", () => {
    const rules = [rule("file", "香盤表.md", true)];
    expect(effectiveAllow(rules, "香盤表.md")).toBe(true);
    expect(effectiveAllow(rules, "香盤表.md.bak")).toBe(false);
    expect(effectiveAllow(rules, "sub/香盤表.md")).toBe(false);
  });

  it("file scope beats directory scope at same length, regardless of order", () => {
    const a = [rule("directory", "a/b.txt", true), rule("file", "a/b.txt", false)];
    const b = [rule("file", "a/b.txt", false), rule("directory", "a/b.txt", true)];
    expect(effectiveAllow(a, "a/b.txt")).toBe(false);
    expect(effectiveAllow(b, "a/b.txt")).toBe(false);
  });

  it("paths containing .. segments are always denied", () => {
    const rules = [rule("directory", "", true)];
    expect(effectiveAllow(rules, "図面/../契約/x.pdf")).toBe(false);
    expect(effectiveAllow(rules, "..")).toBe(false);
  });
});

describe("planToggle", () => {
  it("sets an explicit allow rule when currently denied by default", () => {
    expect(planToggle([], "directory", "図面")).toEqual({ action: "set", allow: true });
  });

  it("deletes own rule when toggling back to the inherited state", () => {
    const rules = [rule("directory", "図面", true)];
    // 図面 は自ルールで true。トグルで false にしたいが、親（ルール無し）の継承も false → 自ルール削除でよい
    expect(planToggle(rules, "directory", "図面")).toEqual({ action: "delete" });
  });

  it("sets an explicit deny when parent allows", () => {
    const rules = [rule("directory", "", true)];
    // 予算メモ.md は親から true を継承。トグルで false → 明示 deny が必要
    expect(planToggle(rules, "file", "予算メモ.md")).toEqual({ action: "set", allow: false });
  });

  it("deletes explicit deny when toggling back on under an allowing parent", () => {
    const rules = [rule("directory", "", true), rule("file", "予算メモ.md", false)];
    // 現在 false → トグルで true。親の継承が true なので自ルール削除でよい
    expect(planToggle(rules, "file", "予算メモ.md")).toEqual({ action: "delete" });
  });
});
