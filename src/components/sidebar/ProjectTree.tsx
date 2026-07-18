import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { useAccountStore } from "../../stores/accountStore";
import { useProjectStore } from "../../stores/projectStore";
import { useMailStore } from "../../stores/mailStore";
import { useDragStore } from "../../stores/dragStore";
import { buildProjectTree, aggregateUnread } from "../../stores/projectTree";
import type { ProjectTreeNode } from "../../stores/projectTree";
import type { DeleteImpact, Project } from "../../types/project";
import { projectApi } from "../../api/projectApi";
import { errorMessage } from "../../api/errors";
import { useErrorStore } from "../../stores/errorStore";
import { ProjectListItem } from "./ProjectListItem";
import { ProjectRenameProvider, useProjectRenameContext } from "./ProjectRenameContext";
import { ContextMenu } from "../common/ContextMenu";
import { MergeProjectDialog } from "./MergeProjectDialog";
import { MoveProjectDialog } from "./MoveProjectDialog";
import { CloudSettingsDialog } from "./CloudSettingsDialog";
import { ProjectDeleteConfirmDialog } from "./ProjectDeleteConfirmDialog";
import { ProjectForm } from "./ProjectForm";

interface ProjectTreeProps {
  onSelectUnclassified: () => void;
  onSelectProject: () => void;
}

export function ProjectTree({ onSelectUnclassified, onSelectProject }: ProjectTreeProps) {
  const selectedAccountId = useAccountStore((s) => s.selectedAccountId);
  const projects = useProjectStore((s) => s.projects);
  const fetchProjects = useProjectStore((s) => s.fetchProjects);
  // length のみ購読する: unclassifiedMails 全体を購読すると、メール操作の
  // たびに（件数が変わらなくても）サイドバー全体が再レンダリングされる
  const unclassifiedCount = useMailStore((s) => s.unclassifiedMails.length);
  const fetchUnclassified = useMailStore((s) => s.fetchUnclassified);
  const fetchUnreadCounts = useMailStore((s) => s.fetchUnreadCounts);

  useEffect(() => {
    if (selectedAccountId) {
      fetchProjects(selectedAccountId);
      fetchUnclassified(selectedAccountId);
      fetchUnreadCounts(selectedAccountId);
    }
  }, [selectedAccountId, fetchProjects, fetchUnclassified, fetchUnreadCounts]);

  if (!selectedAccountId) {
    return null;
  }

  return (
    <div className="mt-2">
      <div className="px-4 py-1">
        <span className="text-xs font-semibold uppercase tracking-wide text-gray-400">
          案件
        </span>
      </div>
      <ProjectRenameProvider projects={projects}>
        <ProjectListInner
          accountId={selectedAccountId}
          onSelectProject={onSelectProject}
        />
      </ProjectRenameProvider>
      {projects.length > 0 && <hr className="mx-4 my-1 border-gray-200" />}
      <button
        onClick={onSelectUnclassified}
        className="w-full px-4 py-2 text-left text-sm hover:bg-gray-100"
      >
        <div className="flex items-center gap-2">
          <span className="text-amber-500">!</span>
          <span>未分類</span>
          {unclassifiedCount > 0 && (
            <span className="ml-auto rounded-full bg-amber-100 px-1.5 py-0.5 text-xs font-semibold text-amber-600">
              {unclassifiedCount}
            </span>
          )}
        </div>
      </button>
    </div>
  );
}

function ProjectListInner({
  accountId,
  onSelectProject,
}: {
  accountId: string;
  onSelectProject: () => void;
}) {
  const projects = useProjectStore((s) => s.projects);
  const selectedProjectId = useProjectStore((s) => s.selectedProjectId);
  const selectProject = useProjectStore((s) => s.selectProject);
  const archiveProject = useProjectStore((s) => s.archiveProject);
  const deleteProject = useProjectStore((s) => s.deleteProject);
  const mergeProject = useProjectStore((s) => s.mergeProject);
  const createProject = useProjectStore((s) => s.createProject);
  const directories = useProjectStore((s) => s.directories);
  const scanningProjects = useProjectStore((s) => s.scanningProjects);
  const rescanProject = useProjectStore((s) => s.rescanProject);
  const unlinkDirectory = useProjectStore((s) => s.unlinkDirectory);
  const linkDirectory = useProjectStore((s) => s.linkDirectory);
  const expandedIds = useProjectStore((s) => s.expandedIds);
  const toggleExpanded = useProjectStore((s) => s.toggleExpanded);
  const draggingMailIds = useDragStore((s) => s.draggingMailIds);
  const endDrag = useDragStore((s) => s.endDrag);
  const bulkMoveMails = useMailStore((s) => s.bulkMoveMails);
  const removeUnclassifiedMail = useMailStore((s) => s.removeUnclassifiedMail);
  const unreadByProject = useMailStore((s) => s.unreadCounts.by_project);
  const { startRename } = useProjectRenameContext();
  const [contextMenu, setContextMenu] = useState<{
    x: number;
    y: number;
    projectId: string;
  } | null>(null);
  const [mergeSourceId, setMergeSourceId] = useState<string | null>(null);
  const [moveSourceId, setMoveSourceId] = useState<string | null>(null);
  const [cloudSettingsProjectId, setCloudSettingsProjectId] = useState<string | null>(null);
  const [childFormParentId, setChildFormParentId] = useState<string | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<{
    project: Project;
    impact: DeleteImpact;
  } | null>(null);
  const [structuralActionInFlight, setStructuralActionInFlight] = useState(false);

  const tree = buildProjectTree(projects);
  const aggregatedUnread = aggregateUnread(projects, unreadByProject);

  const handleDropOnProject = async (projectId: string) => {
    if (!draggingMailIds) return;
    const mailIds = [...draggingMailIds];
    endDrag();
    // 一括移動。結果はトーストで要約され、部分失敗もエラーとして通知される
    const result = await bulkMoveMails(mailIds, projectId);
    // 成功したメールだけを未分類一覧から除去する（失敗分は残して再操作できるようにする）
    for (const mailId of result?.succeeded ?? []) {
      removeUnclassifiedMail(mailId);
    }
  };

  const handleLinkDirectory = async (projectId: string) => {
    const selected = await open({
      directory: true,
      multiple: false,
      title: "案件フォルダを選択",
    });
    if (typeof selected === "string") {
      try {
        await linkDirectory(projectId, selected);
        void rescanProject(projectId); // 紐付け直後に初回スキャン
      } catch {
        // linkDirectory は projectStore 内で errorStore へ通知済み
      }
    }
  };

  const handleRequestDelete = async (projectId: string) => {
    const project = projects.find((p) => p.id === projectId);
    if (!project) return;
    try {
      const impact = await projectApi.getProjectDeleteImpact(projectId);
      setDeleteTarget({ project, impact });
    } catch (e) {
      useErrorStore.getState().addError(errorMessage(e));
    }
  };

  const handleConfirmDelete = async () => {
    if (!deleteTarget || structuralActionInFlight) return;
    setStructuralActionInFlight(true);
    try {
      await deleteProject(deleteTarget.project.id);
      setDeleteTarget(null);
    } finally {
      setStructuralActionInFlight(false);
    }
  };

  const handleArchive = async (projectId: string) => {
    if (structuralActionInFlight) return;
    setStructuralActionInFlight(true);
    try {
      await archiveProject(projectId);
    } finally {
      setStructuralActionInFlight(false);
    }
  };

  const handleCreateChild = async (
    name: string,
    description?: string,
    color?: string,
    directoryPath?: string,
  ) => {
    if (structuralActionInFlight || !childFormParentId) return;
    setStructuralActionInFlight(true);
    try {
      const project = await createProject(accountId, name, description, color, childFormParentId);
      if (directoryPath) {
        try {
          await linkDirectory(project.id, directoryPath);
          void rescanProject(project.id);
        } catch {
          // linkDirectory は projectStore 内で errorStore へ通知済み
        }
      }
      setChildFormParentId(null);
    } finally {
      setStructuralActionInFlight(false);
    }
  };

  const getProjectMenuItems = (projectId: string) => {
    const directory = directories[projectId] ?? null;
    return [
      {
        label: directory ? "フォルダを変更…" : "フォルダを紐付け…",
        onClick: () => void handleLinkDirectory(projectId),
      },
      ...(directory
        ? [
            { label: "再スキャン", onClick: () => void rescanProject(projectId) },
            {
              label: "クラウド送信設定…",
              onClick: () => setCloudSettingsProjectId(projectId),
            },
            { label: "紐付け解除", onClick: () => void unlinkDirectory(projectId) },
          ]
        : []),
      { label: "名前変更", onClick: () => startRename(projectId) },
      { label: "＋ 子案件を作成", onClick: () => setChildFormParentId(projectId) },
      { label: "親を変更...", onClick: () => setMoveSourceId(projectId) },
      { label: "マージ", onClick: () => setMergeSourceId(projectId) },
      { label: "アーカイブ", onClick: () => void handleArchive(projectId) },
      { label: "削除", danger: true, onClick: () => void handleRequestDelete(projectId) },
    ];
  };

  const renderNode = (node: ProjectTreeNode, depth: number) => {
    const hasChildren = node.children.length > 0;
    const expanded = expandedIds.has(node.project.id);
    return (
      <li key={node.project.id}>
        <div className="flex items-center" style={{ paddingLeft: depth * 16 }}>
          {hasChildren ? (
            <button
              onClick={() => toggleExpanded(node.project.id)}
              aria-label={`${node.project.name}を${expanded ? "折りたたむ" : "展開する"}`}
              className="flex h-6 w-4 flex-shrink-0 items-center justify-center text-xs text-gray-400 hover:text-gray-600"
            >
              {expanded ? "▾" : "▸"}
            </button>
          ) : (
            <span className="w-4 flex-shrink-0" />
          )}
          <div className="min-w-0 flex-1">
            <ProjectListItem
              project={node.project}
              selected={selectedProjectId === node.project.id}
              onSelect={() => {
                selectProject(node.project.id);
                onSelectProject();
              }}
              onContextMenu={(e) => {
                e.preventDefault();
                setContextMenu({ x: e.clientX, y: e.clientY, projectId: node.project.id });
              }}
              onDrop={handleDropOnProject}
              directory={directories[node.project.id] ?? null}
              scanning={!!scanningProjects[node.project.id]}
              unreadCount={aggregatedUnread[node.project.id] ?? 0}
            />
          </div>
        </div>
        {childFormParentId === node.project.id && (
          <div style={{ paddingLeft: (depth + 1) * 16 }}>
            <ProjectForm
              onSubmit={(...args) => void handleCreateChild(...args)}
              onCancel={() => setChildFormParentId(null)}
            />
          </div>
        )}
        {hasChildren && expanded && (
          <ul className="flex flex-col">
            {node.children.map((child) => renderNode(child, depth + 1))}
          </ul>
        )}
      </li>
    );
  };

  return (
    <>
      <ul className="flex flex-col">{tree.map((node) => renderNode(node, 0))}</ul>

      {contextMenu && (
        <ContextMenu
          x={contextMenu.x}
          y={contextMenu.y}
          items={getProjectMenuItems(contextMenu.projectId)}
          onClose={() => setContextMenu(null)}
        />
      )}

      {mergeSourceId && (() => {
        const sourceProject = projects.find((p) => p.id === mergeSourceId);
        if (!sourceProject) return null;
        const candidates = projects.filter((p) => p.id !== mergeSourceId);
        return (
          <MergeProjectDialog
            sourceProject={sourceProject}
            candidates={candidates}
            onMerge={async (targetId) => {
              await mergeProject(mergeSourceId, targetId);
              setMergeSourceId(null);
            }}
            onCancel={() => setMergeSourceId(null)}
          />
        );
      })()}

      {moveSourceId && (
        <MoveProjectDialog projectId={moveSourceId} onClose={() => setMoveSourceId(null)} />
      )}

      {deleteTarget && (
        <ProjectDeleteConfirmDialog
          project={deleteTarget.project}
          impact={deleteTarget.impact}
          onConfirm={handleConfirmDelete}
          onCancel={() => setDeleteTarget(null)}
        />
      )}

      {cloudSettingsProjectId && (() => {
        const targetProject = projects.find((p) => p.id === cloudSettingsProjectId);
        const targetDirectory = directories[cloudSettingsProjectId];
        if (!targetProject || !targetDirectory) return null;
        return (
          <CloudSettingsDialog
            project={targetProject}
            directory={targetDirectory}
            onClose={() => setCloudSettingsProjectId(null)}
          />
        );
      })()}
    </>
  );
}
