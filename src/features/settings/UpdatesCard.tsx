import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import { listen } from "@tauri-apps/api/event";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

type UpdateState =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "none" }
  | { kind: "available"; update: Update }
  | { kind: "downloading"; pct: number | null }
  | { kind: "ready" }
  | { kind: "error"; message: string };

/** App updates: manual check, download with progress, relaunch. Auto-check on
 * launch is a setting; a found update surfaces via the update://available
 * event even when this card isn't mounted yet. */
export function UpdatesCard() {
  const [version, setVersion] = useState("");
  const [autoCheck, setAutoCheck] = useState(true);
  const [state, setState] = useState<UpdateState>({ kind: "idle" });

  useEffect(() => {
    void getVersion().then(setVersion);
    void invoke<{ autoCheckUpdates: boolean }>("engine_info").then((i) =>
      setAutoCheck(i.autoCheckUpdates),
    );
    const un = listen<{ version: string }>("update://available", () => {
      setState((s) => (s.kind === "idle" ? { kind: "checking" } : s));
      void doCheck();
    });
    return () => void un.then((f) => f());
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const doCheck = async () => {
    setState({ kind: "checking" });
    try {
      const update = await check();
      setState(update ? { kind: "available", update } : { kind: "none" });
    } catch (e) {
      setState({ kind: "error", message: String(e) });
    }
  };

  const doInstall = async (update: Update) => {
    setState({ kind: "downloading", pct: null });
    let total: number | null = null;
    let got = 0;
    try {
      await update.downloadAndInstall((ev) => {
        if (ev.event === "Started") {
          total = ev.data.contentLength ?? null;
        } else if (ev.event === "Progress") {
          got += ev.data.chunkLength;
          setState({
            kind: "downloading",
            pct: total ? Math.round((got / total) * 100) : null,
          });
        } else if (ev.event === "Finished") {
          setState({ kind: "ready" });
        }
      });
      setState({ kind: "ready" });
    } catch (e) {
      setState({ kind: "error", message: String(e) });
    }
  };

  const toggleAuto = (enabled: boolean) => {
    setAutoCheck(enabled);
    void invoke("set_update_auto_check", { enabled });
  };

  return (
    <div className="rounded-xl border border-neutral-200 p-4 dark:border-neutral-800">
      <div className="flex items-center justify-between">
        <div>
          <div className="font-medium">Updates</div>
          <div className="mt-0.5 text-xs text-neutral-500">Speakly {version}</div>
        </div>
        {state.kind === "available" ? (
          <button
            className="rounded-md bg-accent px-3 py-1.5 text-sm font-medium text-white hover:bg-accent-strong"
            onClick={() => void doInstall(state.update)}
          >
            Install {state.update.version}
          </button>
        ) : state.kind === "ready" ? (
          <button
            className="rounded-md bg-emerald-600 px-3 py-1.5 text-sm font-medium text-white hover:bg-emerald-700"
            onClick={() => void relaunch()}
          >
            Restart to finish
          </button>
        ) : (
          <button
            className="rounded-md border border-neutral-300 px-3 py-1.5 text-sm hover:bg-neutral-100 disabled:opacity-50 dark:border-neutral-700 dark:hover:bg-neutral-800"
            disabled={state.kind === "checking" || state.kind === "downloading"}
            onClick={() => void doCheck()}
          >
            {state.kind === "checking" ? "Checking…" : "Check for updates"}
          </button>
        )}
      </div>

      {state.kind === "none" && (
        <div className="mt-2 text-xs text-neutral-500">You're up to date.</div>
      )}
      {state.kind === "downloading" && (
        <div className="mt-3">
          <div className="h-1.5 w-full overflow-hidden rounded-full bg-neutral-200 dark:bg-neutral-800">
            <div
              className="h-full rounded-full bg-accent transition-all"
              style={{ width: `${state.pct ?? 30}%` }}
            />
          </div>
          <div className="mt-1 text-xs text-neutral-500">
            Downloading{state.pct != null ? ` ${state.pct}%` : "…"}
          </div>
        </div>
      )}
      {state.kind === "error" && (
        <div className="mt-2 text-xs text-red-600 dark:text-red-400">{state.message}</div>
      )}

      <label className="mt-3 flex items-center gap-2 text-sm">
        <input
          type="checkbox"
          checked={autoCheck}
          onChange={(e) => toggleAuto(e.target.checked)}
          className="accent-accent"
        />
        Check for updates automatically at launch
      </label>
    </div>
  );
}
