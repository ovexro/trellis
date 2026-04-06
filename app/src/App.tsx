import { Routes, Route } from "react-router-dom";
import Shell from "./components/layout/Shell";
import Dashboard from "./pages/Dashboard";
import DeviceDetail from "./pages/DeviceDetail";
import SerialMonitor from "./pages/SerialMonitor";
import OtaManager from "./pages/OtaManager";
import FirmwareGenerator from "./pages/FirmwareGenerator";
import Scenes from "./pages/Scenes";
import Settings from "./pages/Settings";

function App() {
  return (
    <Shell>
      <Routes>
        <Route path="/" element={<Dashboard />} />
        <Route path="/device/:id" element={<DeviceDetail />} />
        <Route path="/serial" element={<SerialMonitor />} />
        <Route path="/ota" element={<OtaManager />} />
        <Route path="/new-device" element={<FirmwareGenerator />} />
        <Route path="/scenes" element={<Scenes />} />
        <Route path="/settings" element={<Settings />} />
      </Routes>
    </Shell>
  );
}

export default App;
