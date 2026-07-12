import { useEffect } from "react";
import { useEditor, EditorContent } from "@tiptap/react";
import StarterKit from "@tiptap/starter-kit";
import Link from "@tiptap/extension-link";

interface RichTextEditorProps {
  /** 現在の HTML 本文 */
  value: string;
  /** 編集内容が変わるたびに HTML を返す */
  onChange: (html: string) => void;
}

/**
 * TipTap による最小構成のリッチテキストエディタ。
 * 太字・斜体・リンク・箇条書きのみ（設計書 2026-07-13-rich-compose-design.md）。
 * 本文は HTML として composeStore の body に保持する。
 */
export function RichTextEditor({ value, onChange }: RichTextEditorProps) {
  const editor = useEditor({
    extensions: [
      StarterKit.configure({ heading: false }),
      Link.configure({ openOnClick: false }),
    ],
    content: value,
    onUpdate: ({ editor }) => onChange(editor.getHTML()),
    editorProps: {
      attributes: {
        class:
          "prose prose-sm max-w-none min-h-[12rem] rounded border px-2 py-1 focus:outline-none",
        "aria-label": "本文（リッチテキスト）",
      },
    },
  });

  // 外部から body がまるごと差し替わった場合（フォーマット切替など）に同期する
  useEffect(() => {
    if (editor && editor.getHTML() !== value) {
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
