import "./App.css";
import { Sidebar } from "./components/sidebar/Sidebar";
import { ThreadList } from "./components/thread-list/ThreadList";
import { UnclassifiedList } from "./components/thread-list/UnclassifiedList";
import { MailView } from "./components/mail-view/MailView";
import { DragOverlay } from "./components/common/DragOverlay";
import { ErrorToast } from "./components/common/ErrorToast";
import { useUiStore } from "./stores/uiStore";

function App() {
  const viewMode = useUiStore((s) => s.viewMode);

  return (
    <div className="flex h-screen">
      <Sidebar />
      <div className="w-80 border-r">
        {viewMode === "unclassified" ? (
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
    </div>
  );
}

export default App;
