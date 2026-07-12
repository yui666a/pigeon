import "./App.css";
import { Sidebar } from "./components/sidebar/Sidebar";
import { ThreadList } from "./components/thread-list/ThreadList";
import { UnclassifiedList } from "./components/thread-list/UnclassifiedList";
import { SearchResults } from "./components/thread-list/SearchResults";
import { MailView } from "./components/mail-view/MailView";
import { DragOverlay } from "./components/common/DragOverlay";
import { ErrorToast } from "./components/common/ErrorToast";
import { ComposeModal } from "./components/compose/ComposeModal";
import { useUiStore } from "./stores/uiStore";
import { useKeyboardShortcuts } from "./hooks/useKeyboardShortcuts";

function App() {
  const viewMode = useUiStore((s) => s.viewMode);
  useKeyboardShortcuts();

  return (
    <div className="flex h-screen">
      <Sidebar />
      <div className="w-80 border-r">
        {viewMode === "search" ? (
          <SearchResults />
        ) : viewMode === "unclassified" ? (
          <UnclassifiedList />
        ) : (
          <ThreadList viewMode={viewMode} />
        )}
      </div>
      <div className="flex-1">
        <MailView />
      </div>
      <DragOverlay />
      <ErrorToast />
      <ComposeModal />
    </div>
  );
}

export default App;
