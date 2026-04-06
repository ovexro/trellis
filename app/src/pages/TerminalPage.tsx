import { useState, useRef, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { TerminalSquare } from "lucide-react";

export default function TerminalPage() {
  const [history, setHistory] = useState<Array<{ cmd: string; output: string }>>([]);
  const [input, setInput] = useState("");
  const [running, setRunning] = useState(false);
  const [cmdHistory, setCmdHistory] = useState<string[]>([]);
  const [historyIdx, setHistoryIdx] = useState(-1);
  const outputRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  useEffect(() => {
    if (outputRef.current) {
      outputRef.current.scrollTop = outputRef.current.scrollHeight;
    }
  }, [history]);

  const runCommand = async () => {
    const cmd = input.trim();
    if (!cmd) return;

    setRunning(true);
    setInput("");
    setCmdHistory((prev) => [...prev, cmd]);
    setHistoryIdx(-1);

    try {
      const output = await invoke<string>("run_terminal_command", { command: cmd });
      setHistory((prev) => [...prev, { cmd, output: output.trimEnd() }]);
    } catch (err) {
      setHistory((prev) => [...prev, { cmd, output: `Error: ${err}` }]);
    } finally {
      setRunning(false);
      inputRef.current?.focus();
    }
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter") {
      runCommand();
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      if (cmdHistory.length > 0) {
        const newIdx = historyIdx < cmdHistory.length - 1 ? historyIdx + 1 : historyIdx;
        setHistoryIdx(newIdx);
        setInput(cmdHistory[cmdHistory.length - 1 - newIdx]);
      }
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      if (historyIdx > 0) {
        const newIdx = historyIdx - 1;
        setHistoryIdx(newIdx);
        setInput(cmdHistory[cmdHistory.length - 1 - newIdx]);
      } else {
        setHistoryIdx(-1);
        setInput("");
      }
    }
  };

  return (
    <div className="flex flex-col h-full">
      <div className="flex items-center gap-2 mb-3">
        <TerminalSquare size={18} className="text-zinc-400" />
        <h1 className="text-lg font-bold text-zinc-100">Terminal</h1>
        <p className="text-xs text-zinc-600 ml-2">
          Run shell commands — arduino-cli, esptool, or any Linux command
        </p>
      </div>

      <div
        ref={outputRef}
        className="flex-1 bg-zinc-950 border border-zinc-800 rounded-xl p-4 overflow-auto font-mono text-xs select-text mb-3"
      >
        {history.length === 0 ? (
          <div className="text-zinc-600 space-y-1">
            <p>Welcome to Trellis Terminal.</p>
            <p>Try: <span className="text-zinc-400">arduino-cli board list</span></p>
            <p>Or:  <span className="text-zinc-400">arduino-cli compile --fqbn esp32:esp32:esp32 ~/Arduino/MySketch</span></p>
          </div>
        ) : (
          history.map((entry, i) => (
            <div key={i} className="mb-3">
              <div className="flex items-center gap-1.5">
                <span className="text-trellis-400">$</span>
                <span className="text-zinc-200">{entry.cmd}</span>
              </div>
              {entry.output && (
                <pre className="text-zinc-400 mt-0.5 whitespace-pre-wrap leading-5">
                  {entry.output}
                </pre>
              )}
            </div>
          ))
        )}
        {running && (
          <div className="flex items-center gap-2 text-zinc-500">
            <span className="w-1.5 h-1.5 bg-trellis-400 rounded-full animate-pulse" />
            Running...
          </div>
        )}
      </div>

      <div className="flex gap-2">
        <div className="flex-1 flex items-center bg-zinc-900 border border-zinc-800 rounded-xl px-3">
          <span className="text-trellis-400 font-mono text-sm mr-2">$</span>
          <input
            ref={inputRef}
            type="text"
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={handleKeyDown}
            disabled={running}
            placeholder="Type a command..."
            className="flex-1 bg-transparent border-none py-2.5 text-sm text-zinc-200 font-mono placeholder-zinc-600 focus:outline-none"
          />
        </div>
        <button
          onClick={runCommand}
          disabled={running || !input.trim()}
          className="px-4 py-2.5 bg-trellis-500 hover:bg-trellis-600 text-white rounded-xl text-sm font-medium transition-colors disabled:opacity-50"
        >
          Run
        </button>
      </div>
    </div>
  );
}
