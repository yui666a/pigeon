import type { CloudRule } from "../types/directory";

export interface RuleOp {
  action: "set" | "delete";
  scope: "directory" | "file";
  allow?: boolean; // action === "set" のときのみ
}

function hasDotDotSegment(path: string): boolean {
  return path.split("/").some((seg) => seg === "..");
}

function ruleMatches(rule: CloudRule, relativePath: string): boolean {
  if (rule.scope === "file") {
    return rule.relative_path === relativePath;
  }
  if (rule.scope === "directory") {
    return (
      rule.relative_path === "" ||
      relativePath === rule.relative_path ||
      relativePath.startsWith(`${rule.relative_path}/`)
    );
  }
  return false; // 未知 scope はフェイルクローズ（Rust 側と同一）
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
 * チェックボックス切替時に適用すべきルール操作列を返す。
 * 同一 relative_path の逆スコープの残留ルール（ファイル⇔ディレクトリの入れ替わり等）も
 * 掃除するため、適用後は必ず effectiveAllow が反転する。
 */
export function planToggle(
  rules: CloudRule[],
  scope: "directory" | "file",
  relativePath: string,
): RuleOp[] {
  const desired = !effectiveAllow(rules, relativePath);
  const withoutPath = rules.filter((r) => r.relative_path !== relativePath);
  const inherited = effectiveAllow(withoutPath, relativePath);

  const ops: RuleOp[] = [];
  // 逆スコープの残留ルールを削除（存在する場合のみ）
  const otherScope = scope === "file" ? "directory" : "file";
  if (rules.some((r) => r.scope === otherScope && r.relative_path === relativePath)) {
    ops.push({ action: "delete", scope: otherScope });
  }
  if (desired === inherited) {
    // 継承と同じ状態にしたい → 自スコープのルールも消す（無ければ backend の delete は冪等なので不要）
    if (rules.some((r) => r.scope === scope && r.relative_path === relativePath)) {
      ops.push({ action: "delete", scope });
    }
  } else {
    ops.push({ action: "set", scope, allow: desired });
  }
  return ops;
}
