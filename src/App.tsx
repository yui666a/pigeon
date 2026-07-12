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
import { useUiStore } from "./stores/uiStore";
import { useKeyboardShortcuts } from "./hooks/useKeyboardShortcuts";
import { useMailStore } from "./stores/mailStore";

function App() {
  const viewMode = useUiStore((s) => s.viewMode);
  const initNewMailListener = useMailStore((s) => s.initNewMailListener);
  useKeyboardShortcuts();

  // IMAP IDLE の新着検知イベントを受けて自動同期する（アプリ全体の関心事）
  useEffect(() => {
    const promise = initNewMailListener();
    return () => {
      promise.then((unlisten) => unlisten());
    };
  }, [initNewMailListener]);

  return (
    <div className="flex h-screen">
      <Sidebar />
      <div className="w-80 border-r">
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
