import type { Editor } from "@tiptap/core";
import StarterKit from "@tiptap/starter-kit";
import Link from "@tiptap/extension-link";
// @tiptap/extension-table@3.28.0 は default export を持たず、名前付き export のみ
import { Table } from "@tiptap/extension-table";
import TableRow from "@tiptap/extension-table-row";
import TableCell from "@tiptap/extension-table-cell";
import TableHeader from "@tiptap/extension-table-header";
import { Markdown, type MarkdownStorage } from "tiptap-markdown";

/**
 * 案件ノート用の TipTap 拡張セット。
 * 見出し・太字・斜体・箇条書き・番号リスト・リンク・表をサポートする。
 * 画像は設計上サポートしない（設計書 2026-07-19-project-notes-design.md §2）。
 */
export const NOTE_EXTENSIONS = [
  // TipTap 3.x の StarterKit は Link を内蔵するため、無効化した上で
  // 明示的な Link 拡張（openOnClick: false）を別途登録する
  StarterKit.configure({ link: false }),
  Link.configure({ openOnClick: false }),
  Table.configure({ resizable: false }),
  TableRow,
  TableHeader,
  TableCell,
  Markdown.configure({ html: false, breaks: true, transformPastedText: true }),
];

/**
 * editor.storage.markdown は tiptap-markdown が動的に生やす storage であり、
 * TipTap の Storage 型に自動反映されない（拡張ごとの module augmentation が必要なため）。
 * ここで一箇所だけ型を明示し、呼び出し側での `any` 使用を避ける。
 */
export function getMarkdownStorage(editor: Editor): MarkdownStorage {
  return (editor.storage as unknown as { markdown: MarkdownStorage }).markdown;
}
