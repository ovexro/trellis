import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Terminal, PlugZap, Trash2, Copy, RefreshCw } from "lucide-react";
import type { SerialPortInfo } from "@/lib/types";

export default function SerialMonitor() {
  const [ports, setPorts] = useState<SerialPortInfo[]>([]);
  const [selectedPort, setSelectedPort] = useState("");
  const [baudRate, setBaudRate] = useState(115200);
  const [connected, setConnected] = useState(false);
  const [output, setOutput] = useState<string[]>([]);
  const [input, setInput] = useState("");
  const [autoScroll, setAutoScroll] = useState(true);
  const outputRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    refreshPorts();

    const unlisten = listen<{ port: string; data: string }>(
      "serial-data",
      (event) => {
        setOutput((prev) => {
          const next = [...prev, event.payload.data];
          // Keep last 5000 lines
          return next.length > 5000 ? next.slice(-5000) : next;
        });
      },
    );

    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  useEffect(() => {
    if (autoScroll && outputRef.current) {
      outputRef.current.scrollTop = outputRef.current.scrollHeight;
    }
  }, [output, autoScroll]);

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
        setOutput((prev) => [...prev, `[Disconnected from ${selectedPort}]`]);
      } else {
        await invoke("open_serial", { port: selectedPort, baud: baudRate });
        setConnected(true);
        setOutput((prev) => [
          ...prev,
          `[Connected to ${selectedPort} at ${baudRate} baud]`,
        ]);
        inputRef.current?.focus();
      }
    } catch (err) {
      setOutput((prev) => [...prev, `[ERROR] ${err}`]);
    }
  };

  const send = async () => {
    if (!input.trim() || !connected) return;
    try {
      await invoke("send_serial", { port: selectedPort, data: input });
      setOutput((prev) => [...prev, `> ${input}`]);
      setInput("");
    } catch (err) {
      setOutput((prev) => [...prev, `[SEND ERROR] ${err}`]);
    }
  };

  const clearOutput = () => setOutput([]);

  const copyOutput = () => {
    navigator.clipboard.writeText(output.join("\n"));
  };

  return (
    <div className="flex flex-col h-full">
      {/* Toolbar */}
      <div className="flex items-center gap-3 mb-3 flex-wrap">
        <select
          value={selectedPort}
          onChange={(e) => setSelectedPort(e.target.value)}
          disabled={connected}
          className="bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-1.5 text-sm text-zinc-300 min-w-[180px]"
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
                {b}
              </option>
            ),
          )}
        </select>

        <button
          onClick={toggleConnection}
          className={`flex items-center gap-2 px-3 py-1.5 rounded-lg text-sm font-medium transition-colors ${
            connected
              ? "bg-red-500/10 text-red-400 hover:bg-red-500/20 border border-red-500/20"
              : "bg-trellis-500 text-white hover:bg-trellis-600"
          }`}
        >
          <PlugZap size={14} />
          {connected ? "Disconnect" : "Connect"}
        </button>

        <div className="flex-1" />

        <button
          onClick={refreshPorts}
          disabled={connected}
          className="p-1.5 rounded-lg text-zinc-400 hover:text-zinc-200 hover:bg-zinc-800 transition-colors disabled:opacity-30"
          title="Refresh ports"
        >
          <RefreshCw size={14} />
        </button>

        <button
          onClick={copyOutput}
          className="p-1.5 rounded-lg text-zinc-400 hover:text-zinc-200 hover:bg-zinc-800 transition-colors"
          title="Copy output"
        >
          <Copy size={14} />
        </button>

        <button
          onClick={clearOutput}
          className="p-1.5 rounded-lg text-zinc-400 hover:text-zinc-200 hover:bg-zinc-800 transition-colors"
          title="Clear output"
        >
          <Trash2 size={14} />
        </button>

        <label className="flex items-center gap-1.5 text-xs text-zinc-500">
          <input
            type="checkbox"
            checked={autoScroll}
            onChange={(e) => setAutoScroll(e.target.checked)}
            className="accent-trellis-500"
          />
          Auto-scroll
        </label>
      </div>

      {/* Output */}
      <div
        ref={outputRef}
        className="flex-1 bg-zinc-950 border border-zinc-800 rounded-lg p-3 font-mono text-xs text-zinc-300 overflow-auto mb-3 select-text"
      >
        {output.length === 0 ? (
          <div className="flex items-center gap-2 text-zinc-600 h-full justify-center">
            <Terminal size={16} />
            {connected
              ? "Waiting for data..."
              : "Select a port and click Connect"}
          </div>
        ) : (
          output.map((line, i) => (
            <div
              key={i}
              className={`whitespace-pre-wrap leading-5 ${
                line.startsWith(">")
                  ? "text-trellis-400"
                  : line.startsWith("[")
                    ? "text-zinc-500"
                    : "text-zinc-300"
              }`}
            >
              {line}
            </div>
          ))
        )}
      </div>

      {/* Input */}
      <div className="flex gap-2">
        <input
          ref={inputRef}
          type="text"
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && send()}
          disabled={!connected}
          placeholder={
            connected
              ? "Type command and press Enter..."
              : "Connect to a port first"
          }
          className="flex-1 bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-sm text-zinc-300 font-mono placeholder-zinc-600 disabled:opacity-50 focus:border-trellis-500 focus:outline-none"
        />
        <button
          onClick={send}
          disabled={!connected || !input.trim()}
          className="px-4 py-2 bg-trellis-500 hover:bg-trellis-600 text-white rounded-lg text-sm font-medium transition-colors disabled:opacity-50"
        >
          Send
        </button>
      </div>
    </div>
  );
}
