import type { MapProject } from "../../types/embeddingMap";

const DEFAULT_PROJECT_COLOR = "#6b7280";

interface Props {
  projects: MapProject[];
  /** 点をドラッグ中か。true のときだけドロップを受け付け、見た目も受け入れ可能を示す */
  dropActive: boolean;
  onDrop: (project: MapProject) => void;
}

/**
 * マップウィンドウ内の案件パネル（D&D ドロップ先）。
 * メインのサイドバー（ProjectListItem）と同じ mouseup 方式でドロップを受ける。
 */
export function ProjectPanel({ projects, dropActive, onDrop }: Props) {
  return (
    <div className="border-b">
      <div className="px-3 py-2 text-xs font-semibold text-gray-500">
        {dropActive ? "ここにドロップで割り当て" : "案件"}
      </div>
      <ul className="max-h-72 overflow-y-auto">
        {projects.map((p) => (
          <li
            key={p.id}
            onMouseUp={() => {
              if (dropActive) onDrop(p);
            }}
            className={`flex items-center gap-2 px-3 py-1 text-sm select-none ${
              dropActive ? "cursor-copy hover:bg-blue-50" : ""
            }`}
          >
            <span
              className="inline-block h-2.5 w-2.5 shrink-0 rounded-full"
              style={{ backgroundColor: p.color ?? DEFAULT_PROJECT_COLOR }}
            />
            <span className="truncate">{p.name}</span>
          </li>
        ))}
      </ul>
    </div>
  );
}
