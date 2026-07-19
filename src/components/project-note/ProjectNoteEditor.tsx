import { useEffect } from "react";
import { useEditor, EditorContent } from "@tiptap/react";
import { NOTE_EXTENSIONS, getMarkdownStorage } from "../../utils/markdown";

interface ProjectNoteEditorProps {
  /** 現在の Markdown 本文 */
  value: string;
  /** 編集内容が変わるたびに Markdown を返す */
  onChange: (markdown: string) => void;
  ariaLabel: string;
}

/**
 * 案件ノート用の TipTap エディタ。
 * 保存形式は Markdown（設計書 2026-07-19-project-notes-design.md §3）。
 * 見出し・太字・斜体・箇条書き・番号リスト・リンク・表をサポート（画像は非対応）。
 */
export function ProjectNoteEditor({
  value,
  onChange,
  ariaLabel,
}: ProjectNoteEditorProps) {
  const editor = useEditor({
    extensions: NOTE_EXTENSIONS,
    content: value,
    onUpdate: ({ editor }) => onChange(getMarkdownStorage(editor).getMarkdown()),
    editorProps: {
      attributes: {
        class:
          "prose prose-sm max-w-none min-h-[12rem] rounded border px-2 py-1 focus:outline-none",
        "aria-label": ariaLabel,
      },
    },
  });

  // 外部から value がまるごと差し替わった場合（タブ切替・AI生成後）に同期する
  useEffect(() => {
    if (editor && getMarkdownStorage(editor).getMarkdown() !== value) {
      editor.commands.setContent(value, { emitUpdate: false });
    }
  }, [editor, value]);

  if (!editor) return null;

  const btn = (active: boolean) =>
    `rounded px-2 py-0.5 text-sm hover:bg-gray-100 ${
      active ? "bg-gray-200 font-semibold" : ""
    }`;

  const setLink = () => {
    const prev = editor.getAttributes("link").href as string | undefined;
    const url = window.prompt("リンク先URL", prev ?? "https://");
    if (url === null) return;
    if (url === "") {
      editor.chain().focus().unsetLink().run();
      return;
    }
    editor.chain().focus().extendMarkRange("link").setLink({ href: url }).run();
  };

  return (
    <div className="flex flex-1 flex-col gap-1">
      <div className="flex items-center gap-1 border-b pb-1" role="toolbar">
        <button
          type="button"
          onClick={() => editor.chain().focus().toggleHeading({ level: 2 }).run()}
          className={btn(editor.isActive("heading", { level: 2 }))}
          aria-label="見出し"
        >
          H
        </button>
        <button
          type="button"
          onClick={() => editor.chain().focus().toggleBold().run()}
          className={btn(editor.isActive("bold"))}
          aria-label="太字"
        >
          B
        </button>
        <button
          type="button"
          onClick={() => editor.chain().focus().toggleItalic().run()}
          className={`${btn(editor.isActive("italic"))} italic`}
          aria-label="斜体"
        >
          I
        </button>
        <button
          type="button"
          onClick={() => editor.chain().focus().toggleBulletList().run()}
          className={btn(editor.isActive("bulletList"))}
          aria-label="箇条書き"
        >
          •
        </button>
        <button
          type="button"
          onClick={() =>
            editor
              .chain()
              .focus()
              .insertTable({ rows: 3, cols: 3, withHeaderRow: true })
              .run()
          }
          className={btn(editor.isActive("table"))}
          aria-label="表を挿入"
        >
          ⊞
        </button>
        <button
          type="button"
          onClick={setLink}
          className={btn(editor.isActive("link"))}
          aria-label="リンク"
        >
          🔗
        </button>
      </div>
      <EditorContent editor={editor} className="flex-1 overflow-y-auto" />
    </div>
  );
}
