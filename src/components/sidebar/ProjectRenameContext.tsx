import { createContext, useContext } from "react";
import { useProjectRename } from "../../hooks/useProjectRename";
import type { Project } from "../../types/project";

type ProjectRenameContextValue = ReturnType<typeof useProjectRename>;

const ProjectRenameCtx = createContext<ProjectRenameContextValue | null>(null);

export function ProjectRenameProvider({
  projects,
  children,
}: {
  projects: Project[];
  children: React.ReactNode;
}) {
  const rename = useProjectRename(projects);
  return (
    <ProjectRenameCtx.Provider value={rename}>
      {children}
    </ProjectRenameCtx.Provider>
  );
}

export function useProjectRenameContext() {
  const ctx = useContext(ProjectRenameCtx);
  if (!ctx)
    throw new Error(
      "useProjectRenameContext must be used within ProjectRenameProvider"
    );
  return ctx;
}
