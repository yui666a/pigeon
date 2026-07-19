import { projectPathString } from "../../stores/projectTree";
import type { Project } from "../../types/project";

interface ProjectSelectProps {
  projects: Project[];
  /** 未選択時に出す選択肢の文言（disabled） */
  placeholder: string;
  ariaLabel: string;
  onSelect: (projectId: string) => void;
  /** 選択を保持したい場合に渡す。省略時は選択後に placeholder へ戻る */
  value?: string;
  className?: string;
}

/**
 * 案件をパス表記・パス順で選ばせるセレクト。
 * 階層内では同名案件が共存し得るため、選択肢は常にフルパスで出す。
 *
 * `value` を渡さない場合は非制御で、選択のたびに placeholder へ戻る
 * （同じ案件を続けて選べるようにするため。一括移動がこの挙動を使う）。
 */
export function ProjectSelect({
  projects,
  placeholder,
  ariaLabel,
  onSelect,
  value,
  className = "min-w-0 flex-1 rounded border px-2 py-1 text-sm",
}: ProjectSelectProps) {
  const sortedProjects = [...projects].sort((a, b) =>
    projectPathString(projects, a.id).localeCompare(
      projectPathString(projects, b.id),
    ),
  );

  return (
    <select
      aria-label={ariaLabel}
      {...(value === undefined ? { defaultValue: "" } : { value })}
      onChange={(e) => {
        if (!e.target.value) return;
        onSelect(e.target.value);
        // 非制御のときのみ、次の選択に備えて placeholder へ戻す
        if (value === undefined) e.target.value = "";
      }}
      className={className}
    >
      <option value="" disabled>
        {placeholder}
      </option>
      {sortedProjects.map((p) => (
        <option key={p.id} value={p.id}>
          {projectPathString(projects, p.id)}
        </option>
      ))}
    </select>
  );
}
