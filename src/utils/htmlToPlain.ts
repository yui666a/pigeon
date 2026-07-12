/**
 * リッチ本文（TipTap の HTML）をプレーンテキストへ変換する。
 * 下書き保存はプレーンに落として保存するため（`drafts` テーブルは body_text のみ。
 * 設計書 2026-07-13-rich-compose-design.md の v1 制限）フロント側でも変換が必要。
 *
 * 変換規則は Rust 側 `html_to_plain`（送信時の plain フォールバック）と揃える:
 * ブロック要素の終わりを改行、リスト項目を先頭 `- `、連続空行は1つに圧縮。
 * こちらはブラウザの DOMParser でタグ境界を解釈する。
 */
export function htmlToPlain(html: string): string {
  const doc = new DOMParser().parseFromString(html, "text/html");
  // 出力を文字列バッファに組み立て、最後に空行を正規化する（Rust 実装と同じ方針）
  let out = "";

  const BLOCK = /^(p|div|h[1-6]|ul|ol|li)$/;

  const walk = (node: Node): void => {
    node.childNodes.forEach((child) => {
      if (child.nodeType === Node.TEXT_NODE) {
        out += child.textContent ?? "";
        return;
      }
      if (child.nodeType !== Node.ELEMENT_NODE) return;
      const el = child as HTMLElement;
      const tag = el.tagName.toLowerCase();

      if (tag === "br") {
        out += "\n";
        return;
      }
      if (tag === "li") out += "- ";
      walk(el);
      // ブロック要素の終わりで改行（隣接ブロックは単一改行で区切られる）
      if (BLOCK.test(tag)) out += "\n";
    });
  };

  walk(doc.body);
  return normalizeBlankLines(out);
}

/** 各行末の空白を除去し、連続する空行を最大1つに圧縮し、全体を trim する */
function normalizeBlankLines(s: string): string {
  const lines: string[] = [];
  let blankRun = 0;
  for (const line of s.split("\n")) {
    const trimmed = line.replace(/\s+$/, "");
    if (trimmed.trim() === "") {
      blankRun += 1;
      if (blankRun <= 1) lines.push("");
    } else {
      blankRun = 0;
      lines.push(trimmed);
    }
  }
  return lines.join("\n").trim();
}
