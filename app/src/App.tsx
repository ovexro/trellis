import { useState, useEffect } from "react";
import { Routes, Route, Navigate, useLocation } from "react-router-dom";
import { invoke } from "@tauri-apps/api/core";
import Shell from "./components/layout/Shell";
import Dashboard from "./pages/Dashboard";
import DeviceDetail from "./pages/DeviceDetail";
import SerialMonitor from "./pages/SerialMonitor";
import OtaManager from "./pages/OtaManager";
import FirmwareGenerator from "./pages/FirmwareGenerator";
import GetStarted from "./pages/GetStarted";
import Scenes from "./pages/Scenes";
import Automation from "./pages/Automation";
import TerminalPage from "./pages/TerminalPage";
import Settings from "./pages/settings";

function FirstRunRedirect({ children }: { children: React.ReactNode }) {
  const [checked, setChecked] = useState(false);
  const [needsOnboarding, setNeedsOnboarding] = useState(false);
  const location = useLocation();

  useEffect(() => {
    invoke<string | null>("get_setting", { key: "onboarding_completed" })
      .then((val) => {
        setNeedsOnboarding(val !== "true");
        setChecked(true);
      })
      .catch(() => setChecked(true));
  }, []);

  if (!checked) return null;

  if (needsOnboarding && location.pathname === "/") {
    return <Navigate to="/get-started" replace />;
  }

  return <>{children}</>;
}

function App() {
  return (
    <Shell>
      <FirstRunRedirect>
        <Routes>
          <Route path="/" element={<Dashboard />} />
          <Route path="/get-started" element={<GetStarted />} />
          <Route path="/device/:id" element={<DeviceDetail />} />
          <Route path="/serial" element={<SerialMonitor />} />
          <Route path="/ota" element={<OtaManager />} />
          <Route path="/new-device" element={<FirmwareGenerator />} />
          <Route path="/scenes" element={<Scenes />} />
          <Route path="/automation" element={<Automation />} />
          <Route path="/terminal" element={<TerminalPage />} />
          <Route path="/settings" element={<Settings />} />
        </Routes>
      </FirstRunRedirect>
    </Shell>
  );
}

export default App;
