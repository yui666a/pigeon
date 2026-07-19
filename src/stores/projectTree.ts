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

/** ストア上の projects からパンくず文字列を合成（" > " 区切り、ルート→自ノード順）。
 * id が projects に無い場合は空文字。祖先が配列内に居ない場合（アーカイブ済み等）はそこで打ち切る。 */
export function projectPathString(projects: Project[], id: string): string {
  const byId = new Map(projects.map((p) => [p.id, p]));
  const parts: string[] = [];
  let cur = byId.get(id);
  while (cur) {
    parts.unshift(cur.name);
    cur = cur.parent_id ? byId.get(cur.parent_id) : undefined;
  }
  return parts.join(" > ");
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

/** projectId 自身とその子孫すべての id 集合（親の付け替え先・マージ先から除外する対象） */
export function collectSubtreeIds(
  nodes: ProjectTreeNode[],
  projectId: string,
): Set<string> {
  const found = (function find(list: ProjectTreeNode[]): ProjectTreeNode | null {
    for (const node of list) {
      if (node.project.id === projectId) return node;
      const inChildren = find(node.children);
      if (inChildren) return inChildren;
    }
    return null;
  })(nodes);

  const ids = new Set<string>();
  if (!found) return ids;
  (function collect(node: ProjectTreeNode) {
    ids.add(node.project.id);
    for (const child of node.children) collect(child);
  })(found);
  return ids;
}
