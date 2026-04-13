import { useState } from "react";
import "./App.css";
import { Sidebar } from "./components/sidebar/Sidebar";
import { ThreadList } from "./components/thread-list/ThreadList";
import { UnclassifiedList } from "./components/thread-list/UnclassifiedList";
import { MailView } from "./components/mail-view/MailView";

type ViewMode = "threads" | "unclassified" | "project";

function App() {
  const [viewMode, setViewMode] = useState<ViewMode>("threads");

  return (
    <div className="flex h-screen">
      <Sidebar onViewChange={setViewMode} />
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
    </div>
  );
}

export default App;
