import { getCurrentWindow } from "@tauri-apps/api/window";
import QuickPanel from "./components/QuickPanel";
import ChatThread from "./components/ChatThread";

const label = (() => {
  try {
    const l = getCurrentWindow().label;
    if (l === "main" || l === "quick") return l;
  } catch {}
  return "main";
})();

function App() {
  if (label === "quick") return <QuickPanel />;
  return <ChatThread />;
}

export default App;
