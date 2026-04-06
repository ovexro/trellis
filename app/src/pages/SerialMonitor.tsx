import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Terminal, PlugZap } from "lucide-react";
import type { SerialPortInfo } from "@/lib/types";

export default function SerialMonitor() {
  const [ports, setPorts] = useState<SerialPortInfo[]>([]);
  const [selectedPort, setSelectedPort] = useState("");
  const [baudRate, setBaudRate] = useState(115200);
  const [connected, setConnected] = useState(false);
  const [output, setOutput] = useState<string[]>([]);
  const [input, setInput] = useState("");

  useEffect(() => {
    refreshPorts();
  }, []);

  const refreshPorts = async () => {
    try {
      const result = await invoke<SerialPortInfo[]>("list_serial_ports");
      setPorts(result);
      if (result.length > 0 && !selectedPort) {
        setSelectedPort(result[0].name);
      }
    } catch (err) {
      console.error("Failed to list ports:", err);
    }
  };

  const toggleConnection = async () => {
    try {
      if (connected) {
        await invoke("close_serial", { port: selectedPort });
        setConnected(false);
      } else {
        await invoke("open_serial", { port: selectedPort, baud: baudRate });
        setConnected(true);
      }
    } catch (err) {
      console.error("Serial error:", err);
    }
  };

  const send = async () => {
    if (!input.trim()) return;
    try {
      await invoke("send_serial", { port: selectedPort, data: input });
      setOutput((prev) => [...prev, `> ${input}`]);
      setInput("");
    } catch (err) {
      console.error("Send error:", err);
    }
  };

  return (
    <div className="flex flex-col h-full">
      <div className="flex items-center gap-3 mb-4">
        <select
          value={selectedPort}
          onChange={(e) => setSelectedPort(e.target.value)}
          disabled={connected}
          className="bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-1.5 text-sm text-zinc-300"
        >
          {ports.length === 0 && <option>No ports found</option>}
          {ports.map((p) => (
            <option key={p.name} value={p.name}>
              {p.name} ({p.port_type})
            </option>
          ))}
        </select>

        <select
          value={baudRate}
          onChange={(e) => setBaudRate(Number(e.target.value))}
          disabled={connected}
          className="bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-1.5 text-sm text-zinc-300"
        >
          {[9600, 19200, 38400, 57600, 115200, 230400, 460800, 921600].map(
            (b) => (
              <option key={b} value={b}>
                {b} baud
              </option>
            ),
          )}
        </select>

        <button
          onClick={toggleConnection}
          className={`flex items-center gap-2 px-3 py-1.5 rounded-lg text-sm transition-colors ${
            connected
              ? "bg-red-500/10 text-red-400 hover:bg-red-500/20"
              : "bg-trellis-500/10 text-trellis-400 hover:bg-trellis-500/20"
          }`}
        >
          <PlugZap size={14} />
          {connected ? "Disconnect" : "Connect"}
        </button>

        <button
          onClick={refreshPorts}
          className="px-3 py-1.5 rounded-lg text-sm text-zinc-400 hover:text-zinc-200 bg-zinc-800 hover:bg-zinc-700 transition-colors"
        >
          Refresh
        </button>
      </div>

      <div className="flex-1 bg-zinc-900 border border-zinc-800 rounded-lg p-4 font-mono text-xs text-zinc-300 overflow-auto mb-3">
        {output.length === 0 ? (
          <div className="flex items-center gap-2 text-zinc-600">
            <Terminal size={14} />
            Serial output will appear here
          </div>
        ) : (
          output.map((line, i) => (
            <div key={i} className="whitespace-pre-wrap">
              {line}
            </div>
          ))
        )}
      </div>

      <div className="flex gap-2">
        <input
          type="text"
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && send()}
          disabled={!connected}
          placeholder="Type command and press Enter..."
          className="flex-1 bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-sm text-zinc-300 placeholder-zinc-600 disabled:opacity-50"
        />
        <button
          onClick={send}
          disabled={!connected}
          className="px-4 py-2 bg-trellis-500 hover:bg-trellis-600 text-white rounded-lg text-sm transition-colors disabled:opacity-50"
        >
          Send
        </button>
      </div>
    </div>
  );
}
