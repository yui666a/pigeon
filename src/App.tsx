import "./App.css";
import { Sidebar } from "./components/sidebar/Sidebar";
import { ThreadList } from "./components/thread-list/ThreadList";
import { MailView } from "./components/mail-view/MailView";

function App() {
  return (
    <div className="flex h-screen">
      <Sidebar />
      <div className="w-80 border-r">
        <ThreadList />
      </div>
      <div className="flex-1">
        <MailView />
      </div>
    </div>
  );
}

export default App;
