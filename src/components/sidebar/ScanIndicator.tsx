import { useProjectStore } from "../../stores/projectStore";

export function ScanIndicator() {
  const scanningProjects = useProjectStore((s) => s.scanningProjects);
  const projects = useProjectStore((s) => s.projects);

  const scanningNames = projects
    .filter((p) => scanningProjects[p.id])
    .map((p) => p.name);

  if (scanningNames.length === 0) return null;

  return (
    <div className="border-t px-4 py-1.5 text-xs text-gray-500">
      スキャン中… {scanningNames.join(", ")}
    </div>
  );
}
