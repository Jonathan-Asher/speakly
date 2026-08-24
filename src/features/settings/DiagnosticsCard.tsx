import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

interface EngineInfo {
  version: string;
  backend: string;
  autoCheckUpdates: boolean;
  models: { id: string; present: boolean }[];
  logDir: string;
}

/** Engine facts + the local log file, for support without any telemetry. */
export function DiagnosticsCard() {
  const [info, setInfo] = useState<EngineInfo | null>(null);
  const [tail, setTail] = useState("");
  const [copied, setCopied] = useState(false);

  const refresh = () => {
    void invoke<EngineInfo>("engine_info").then(setInfo);
    void invoke<string>("read_log_tail", { lines: 200 }).then(setTail);
  };

  useEffect(refresh, []);

  const copy = async () => {
    await navigator.clipboard.writeText(tail);
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  };

  return (
    <div className="rounded-xl border border-neutral-200 p-4 dark:border-neutral-800">
      <div className="flex items-center justify-between">
        <div className="font-medium">Diagnostics</div>
        <div className="flex gap-2">
          <button
            className="rounded-md border border-neutral-300 px-2.5 py-1 text-xs hover:bg-neutral-100 dark:border-neutral-700 dark:hover:bg-neutral-800"
            onClick={refresh}
          >
            Refresh
          </button>
          <button
            className="rounded-md border border-neutral-300 px-2.5 py-1 text-xs hover:bg-neutral-100 dark:border-neutral-700 dark:hover:bg-neutral-800"
            onClick={() => void invoke("reveal_logs")}
          >
            Reveal in Finder
          </button>
          <button
            className="rounded-md border border-neutral-300 px-2.5 py-1 text-xs hover:bg-neutral-100 dark:border-neutral-700 dark:hover:bg-neutral-800"
            onClick={() => void copy()}
          >
            {copied ? "Copied ✓" : "Copy"}
          </button>
        </div>
      </div>

      {info && (
        <div className="mt-2 grid grid-cols-2 gap-x-4 gap-y-1 text-xs text-neutral-500">
          <span>Version {info.version}</span>
          <span>{info.backend}</span>
          <span className="col-span-2">
            Models:{" "}
            {info.models
              .map((m) => `${m.id} ${m.present ? "✓" : "—"}`)
              .join(" · ")}
          </span>
        </div>
      )}

      <pre className="selectable mt-3 max-h-56 overflow-auto rounded-lg bg-neutral-100 p-3 font-mono text-[11px] leading-snug whitespace-pre-wrap text-neutral-700 dark:bg-neutral-900 dark:text-neutral-300">
        {tail || "No log entries yet."}
      </pre>
    </div>
  );
}
