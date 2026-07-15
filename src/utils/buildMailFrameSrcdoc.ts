/**
 * sandbox iframe に流し込むメール本文の完全な HTML 文書を組み立てる。
 *
 * iframe 内には親文書の Tailwind が届かないため、メール表示に必要な
 * 最小限の基本スタイルをここで同梱する。入力はサニタイズ済み HTML を
 * 前提とし、この関数自体は無害化を行わない（第1層は sanitizeMailHtml、
 * 第2層は CSP、第3層が iframe 隔離）。
 */
const BASE_STYLE = `
:root { color-scheme: light; }
body {
  margin: 0;
  font-family: system-ui, -apple-system, "Hiragino Sans", sans-serif;
  font-size: 14px;
  line-height: 1.6;
  color: #111827;
  word-break: break-word;
  overflow-wrap: anywhere;
}
img { max-width: 100%; height: auto; }
a { color: #2563eb; }
pre { white-space: pre-wrap; }
table { max-width: 100%; }
blockquote {
  border-left: 3px solid #e5e7eb;
  margin-left: 0;
  padding-left: 12px;
  color: #4b5563;
}
`;

export function buildMailFrameSrcdoc(sanitizedHtml: string): string {
  return `<!doctype html><html><head><meta charset="utf-8"><style>${BASE_STYLE}</style></head><body>${sanitizedHtml}</body></html>`;
}
