import type { Project } from "../types/project";

export interface ProjectTreeNode {
  project: Project;
  children: ProjectTreeNode[];
}

/** フラット配列から木を組む。親が配列内に居ない場合（アーカイブ済み祖先等）はルート扱い。 */
export function buildProjectTree(projects: Project[]): ProjectTreeNode[] {
  const nodes = new Map<string, ProjectTreeNode>();
  for (const project of projects) nodes.set(project.id, { project, children: [] });
  const roots: ProjectTreeNode[] = [];
  for (const node of nodes.values()) {
    const parentId = node.project.parent_id;
    const parent = parentId ? nodes.get(parentId) : undefined;
    if (parent) parent.children.push(node);
    else roots.push(node);
  }
  return roots;
}

/** ノード直接所属の未読数を、自分+子孫の合算へボトムアップ集約する。 */
export function aggregateUnread(
  projects: Project[],
  direct: Record<string, number>,
): Record<string, number> {
  const result: Record<string, number> = {};
  const childrenOf = new Map<string, string[]>();
  for (const p of projects) {
    if (p.parent_id) {
      childrenOf.set(p.parent_id, [...(childrenOf.get(p.parent_id) ?? []), p.id]);
    }
  }
  const sum = (id: string): number => {
    if (id in result) return result[id];
    let total = direct[id] ?? 0;
    for (const child of childrenOf.get(id) ?? []) total += sum(child);
    result[id] = total;
    return total;
  };
  for (const p of projects) sum(p.id);
  return result;
}
