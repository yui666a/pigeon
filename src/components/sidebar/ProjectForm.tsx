import { useState } from "react";

interface ProjectFormProps {
  onSubmit: (name: string, description?: string, color?: string) => void;
  onCancel: () => void;
}

export function ProjectForm({ onSubmit, onCancel }: ProjectFormProps) {
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [color, setColor] = useState("#6b7280");

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!name.trim()) return;
    onSubmit(name.trim(), description.trim() || undefined, color);
  };

  return (
    <form onSubmit={handleSubmit} className="border-b px-4 py-3">
      <div className="mb-2">
        <label className="mb-1 block text-xs font-medium text-gray-600">
          案件名
          <span className="text-red-500">*</span>
        </label>
        <input
          type="text"
          value={name}
          onChange={(e) => setName(e.target.value)}
          required
          className="w-full rounded border border-gray-300 px-2 py-1 text-sm focus:border-blue-400 focus:outline-none"
          placeholder="案件名を入力"
        />
      </div>
      <div className="mb-2">
        <label className="mb-1 block text-xs font-medium text-gray-600">
          説明
        </label>
        <input
          type="text"
          value={description}
          onChange={(e) => setDescription(e.target.value)}
          className="w-full rounded border border-gray-300 px-2 py-1 text-sm focus:border-blue-400 focus:outline-none"
          placeholder="説明（任意）"
        />
      </div>
      <div className="mb-3">
        <label className="mb-1 block text-xs font-medium text-gray-600">
          色
        </label>
        <input
          type="color"
          value={color}
          onChange={(e) => setColor(e.target.value)}
          className="h-7 w-10 cursor-pointer rounded border border-gray-300"
        />
      </div>
      <div className="flex gap-2">
        <button
          type="submit"
          className="flex-1 rounded bg-blue-600 px-3 py-1.5 text-sm font-medium text-white hover:bg-blue-700"
        >
          作成
        </button>
        <button
          type="button"
          onClick={onCancel}
          className="flex-1 rounded border border-gray-300 px-3 py-1.5 text-sm font-medium text-gray-600 hover:bg-gray-100"
        >
          キャンセル
        </button>
      </div>
    </form>
  );
}
