import { useState } from "react";
import ConfigSection from "./ConfigSection";
import NotificationsSection from "./NotificationsSection";
import MqttSection from "./MqttSection";
import ApiTokensSection from "./ApiTokensSection";
import RemoteAccessSection from "./RemoteAccessSection";
import DiagnosticsAboutSection from "./DiagnosticsAboutSection";

export default function Settings() {
  const [apiTokenCount, setApiTokenCount] = useState(0);

  return (
    <div>
      <h1 className="text-xl font-bold text-zinc-100 mb-6">Settings</h1>

      <div className="space-y-8">
        <ConfigSection />
        <NotificationsSection />
        <MqttSection />
        <ApiTokensSection onTokenCountChange={setApiTokenCount} />
        <RemoteAccessSection apiTokenCount={apiTokenCount} />
        <DiagnosticsAboutSection />
      </div>
    </div>
  );
}
