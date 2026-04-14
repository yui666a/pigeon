import { useEffect, useRef, useState } from "react";
import { useProjectStore } from "../stores/projectStore";
import type { Project } from "../types/project";

export function useProjectRename(projects: Project[]) {
  const updateProject = useProjectStore((s) => s.updateProject);
  const [renamingProjectId, setRenamingProjectId] = useState<string | null>(null);
  const [renameValue, setRenameValue] = useState("");
  const renameInputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (renamingProjectId && renameInputRef.current) {
      renameInputRef.current.focus();
      renameInputRef.current.select();
    }
  }, [renamingProjectId]);

  const startRename = (projectId: string) => {
    const project = projects.find((p) => p.id === projectId);
    if (!project) return;
    setRenamingProjectId(projectId);
    setRenameValue(project.name);
  };

  const submitRename = async () => {
    if (renamingProjectId && renameValue.trim()) {
      await updateProject(renamingProjectId, renameValue.trim());
    }
    setRenamingProjectId(null);
    setRenameValue("");
  };

  const cancelRename = () => {
    setRenamingProjectId(null);
    setRenameValue("");
  };

  return {
    renamingProjectId,
    renameValue,
    setRenameValue,
    renameInputRef,
    startRename,
    submitRename,
    cancelRename,
  };
}
