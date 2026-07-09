import { describe, it, expect } from "vitest";
import { effectiveAllow, planToggle, type RuleOp } from "../../utils/cloudPolicy";
import type { CloudRule } from "../../types/directory";

function rule(scope: "directory" | "file", path: string, allow: boolean): CloudRule {
  return { id: `r-${scope}-${path}`, directory_id: "d1", scope, relative_path: path, allow };
}

/** RuleOp[] を rules に適用するテスト用ヘルパー（バックエンドの set/delete 相当）。 */
function applyOps(rules: CloudRule[], ops: RuleOp[], relativePath: string): CloudRule[] {
  let result = rules;
  for (const op of ops) {
    result = result.filter((r) => !(r.scope === op.scope && r.relative_path === relativePath));
    if (op.action === "set") {
      result = [
        ...result,
        {
          id: `r-${op.scope}-${relativePath}`,
          directory_id: "d1",
          scope: op.scope,
          relative_path: relativePath,
          allow: op.allow as boolean,
        },
      ];
    }
  }
  return result;
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

  it("directory-vs-directory longest match wins", () => {
    const rules = [rule("directory", "図面", true), rule("directory", "図面/社外秘", false)];
    expect(effectiveAllow(rules, "図面/平面図.pdf")).toBe(true);
    expect(effectiveAllow(rules, "図面/社外秘/原価.txt")).toBe(false);
  });

  it("paths containing .. segments are always denied", () => {
    const rules = [rule("directory", "", true)];
    expect(effectiveAllow(rules, "図面/../契約/x.pdf")).toBe(false);
    expect(effectiveAllow(rules, "..")).toBe(false);
    expect(effectiveAllow(rules, "../図面/x.pdf")).toBe(false);
    expect(effectiveAllow(rules, "図面/..")).toBe(false);
  });

  it("unknown scope fails closed (never matches)", () => {
    const rules = [
      { id: "r1", directory_id: "d1", scope: "bogus" as CloudRule["scope"], relative_path: "", allow: true },
    ];
    expect(effectiveAllow(rules, "anything.txt")).toBe(false);
  });
});

describe("planToggle", () => {
  it("sets an explicit allow rule when currently denied by default", () => {
    expect(planToggle([], "directory", "図面")).toEqual([
      { action: "set", scope: "directory", allow: true },
    ]);
  });

  it("deletes own rule when toggling back to the inherited state", () => {
    const rules = [rule("directory", "図面", true)];
    // 図面 は自ルールで true。トグルで false にしたいが、親（ルール無し）の継承も false → 自ルール削除でよい
    expect(planToggle(rules, "directory", "図面")).toEqual([
      { action: "delete", scope: "directory" },
    ]);
  });

  it("sets an explicit deny when parent allows", () => {
    const rules = [rule("directory", "", true)];
    // 予算メモ.md は親から true を継承。トグルで false → 明示 deny が必要
    expect(planToggle(rules, "file", "予算メモ.md")).toEqual([
      { action: "set", scope: "file", allow: false },
    ]);
  });

  it("deletes explicit deny when toggling back on under an allowing parent", () => {
    const rules = [rule("directory", "", true), rule("file", "予算メモ.md", false)];
    // 現在 false → トグルで true。親の継承が true なので自ルール削除でよい
    expect(planToggle(rules, "file", "予算メモ.md")).toEqual([
      { action: "delete", scope: "file" },
    ]);
  });

  it("cleans up leftover opposite-scope rule when toggling directory over a stale file rule", () => {
    // file ルールが false で残っている状態で、同じ path を directory としてトグルする
    const rules = [rule("file", "a/b", false)];
    const before = effectiveAllow(rules, "a/b");
    expect(before).toBe(false);

    const ops = planToggle(rules, "directory", "a/b");
    // 逆スコープ(file)の残留ルールを削除する操作が含まれる
    expect(ops).toContainEqual({ action: "delete", scope: "file" });

    const after = applyOps(rules, ops, "a/b");
    expect(effectiveAllow(after, "a/b")).toBe(!before);
  });

  it("cleans up leftover opposite-scope rule when toggling file over a stale directory rule", () => {
    // directory ルールが true で残っている状態で、同じ path を file としてトグルする
    const rules = [rule("directory", "a/b", true)];
    const before = effectiveAllow(rules, "a/b");
    expect(before).toBe(true);

    const ops = planToggle(rules, "file", "a/b");
    expect(ops).toContainEqual({ action: "delete", scope: "directory" });

    const after = applyOps(rules, ops, "a/b");
    expect(effectiveAllow(after, "a/b")).toBe(!before);
  });

  it("does not emit a delete op for an opposite scope that has no rule", () => {
    const rules = [rule("directory", "図面", true)];
    const ops = planToggle(rules, "directory", "図面");
    expect(ops.some((op) => op.scope === "file")).toBe(false);
  });
});
