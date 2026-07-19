import { useEffect } from "react";
import "./App.css";
import { Sidebar } from "./components/sidebar/Sidebar";
import { ThreadList } from "./components/thread-list/ThreadList";
import { UnclassifiedList } from "./components/thread-list/UnclassifiedList";
import { SearchResults } from "./components/thread-list/SearchResults";
import { DraftList } from "./components/thread-list/DraftList";
import { MailView } from "./components/mail-view/MailView";
import { DragOverlay } from "./components/common/DragOverlay";
import { ToastContainer } from "./components/common/ToastContainer";
import { ComposeModal } from "./components/compose/ComposeModal";
import { ProjectNotePanel } from "./components/project-note/ProjectNotePanel";
import { useUiStore } from "./stores/uiStore";
import { useProjectStore } from "./stores/projectStore";
import { useKeyboardShortcuts } from "./hooks/useKeyboardShortcuts";
import { useMailStore } from "./stores/mailStore";
import { ensureNotificationPermission } from "./utils/notifyNewMail";

function App() {
  const viewMode = useUiStore((s) => s.viewMode);
  const selectedProjectId = useProjectStore((s) => s.selectedProjectId);
  const initNewMailListener = useMailStore((s) => s.initNewMailListener);
  useKeyboardShortcuts();

  // IMAP IDLE の新着検知イベントを受けて自動同期する（アプリ全体の関心事）
  useEffect(() => {
    const promise = initNewMailListener();
    return () => {
      promise.then((unlisten) => unlisten());
    };
  }, [initNewMailListener]);

  // 通知が有効なら起動時に権限を確保しておく。新着検知を待って初めて権限を
  // 求める設計だと、一度も新着が来ないと通知が届かないため前倒しで要求する
  useEffect(() => {
    void ensureNotificationPermission();
  }, []);

  return (
    <div className="flex h-screen">
      <Sidebar />
      <div className="flex w-80 flex-col border-r">
        {viewMode === "project" && selectedProjectId && (
          <details className="border-b">
            <summary className="cursor-pointer px-2 py-1 text-sm font-semibold">
              案件ノート
            </summary>
            <ProjectNotePanel projectId={selectedProjectId} />
          </details>
        )}
        <div className="min-h-0 flex-1">
          {viewMode === "search" ? (
            <SearchResults />
          ) : viewMode === "unclassified" ? (
            <UnclassifiedList />
          ) : viewMode === "drafts" ? (
            <DraftList />
          ) : (
            <ThreadList viewMode={viewMode} />
          )}
        </div>
      </div>
      <div className="flex-1">
        <MailView />
      </div>
      <DragOverlay />
      <ToastContainer />
      <ComposeModal />
    </div>
  );
}

export default App;
