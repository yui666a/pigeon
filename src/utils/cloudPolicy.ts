import type { CloudRule } from "../types/directory";

export type ToggleAction = { action: "set"; allow: boolean } | { action: "delete" };

function hasDotDotSegment(path: string): boolean {
  return path.split("/").some((seg) => seg === "..");
}

function ruleMatches(rule: CloudRule, relativePath: string): boolean {
  if (rule.scope === "file") {
    return rule.relative_path === relativePath;
  }
  return (
    rule.relative_path === "" ||
    relativePath === rule.relative_path ||
    relativePath.startsWith(`${rule.relative_path}/`)
  );
}

/**
 * Rust 側 cloud_policy::is_cloud_allowed と同一セマンティクスの表示用判定。
 * マッチするルールが無ければ常に false（危険側に倒れない）。
 * 最長 relative_path のルールが勝ち、同長なら file スコープが勝つ。
 */
export function effectiveAllow(rules: CloudRule[], relativePath: string): boolean {
  if (hasDotDotSegment(relativePath)) return false;
  let best: CloudRule | null = null;
  for (const rule of rules) {
    if (!ruleMatches(rule, relativePath)) continue;
    if (
      best === null ||
      rule.relative_path.length > best.relative_path.length ||
      (rule.relative_path.length === best.relative_path.length &&
        rule.scope === "file" &&
        best.scope !== "file")
    ) {
      best = rule;
    }
  }
  return best?.allow ?? false;
}

/**
 * チェックボックス切替時のルール操作を決める。
 * 望む状態が「自ルールを消したときの継承状態」と同じなら delete（ルールを増やさない）、
 * 違うなら明示ルールを set する。
 */
export function planToggle(
  rules: CloudRule[],
  scope: "directory" | "file",
  relativePath: string,
): ToggleAction {
  const desired = !effectiveAllow(rules, relativePath);
  const withoutOwn = rules.filter(
    (r) => !(r.scope === scope && r.relative_path === relativePath),
  );
  const inherited = effectiveAllow(withoutOwn, relativePath);
  if (desired === inherited) {
    return { action: "delete" };
  }
  return { action: "set", allow: desired };
}
