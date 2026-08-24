import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { DirectionalText } from "../../components/DirectionalText";
import {
  onMeetingFinished,
  onMeetingSegment,
  onMeetingStatus,
  type MeetingSegmentEvent,
  type MeetingStatusEvent,
} from "../../ipc/events";

interface CapturableApp {
  bundleId: string;
  name: string;
  pid: number;
}

interface ModelStatus {
  id: string;
  path: string;
  present: boolean;
}

const SYSTEM = "__system__";

function fmtTime(ms: number) {
  const s = Math.floor(ms / 1000);
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

export function MeetingsView() {
  const [permitted, setPermitted] = useState<boolean | null>(null);
  const [apps, setApps] = useState<CapturableApp[]>([]);
  const [appsError, setAppsError] = useState<string | null>(null);
  const [selection, setSelection] = useState<string>(SYSTEM);
  const [mic, setMic] = useState(true);
  const [models, setModels] = useState<ModelStatus[]>([]);
  const [modelId, setModelId] = useState("he-turbo");
  const [language, setLanguage] = useState("he");
  const [sessionId, setSessionId] = useState<number | null>(null);
  const [status, setStatus] = useState<MeetingStatusEvent | null>(null);
  const [segments, setSegments] = useState<MeetingSegmentEvent[]>([]);
  const [savedNote, setSavedNote] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [pinned, setPinned] = useState(true);
  const scrollRef = useRef<HTMLDivElement>(null);
  const pinnedRef = useRef(pinned);
  pinnedRef.current = pinned;

  const refreshPermission = () =>
    void invoke<boolean>("screen_recording_status").then(setPermitted);

  useEffect(() => {
    refreshPermission();
    window.addEventListener("focus", refreshPermission);
    void invoke<{ models: ModelStatus[] }>("get_model_status").then((r) =>
      setModels(r.models.filter((m) => m.present)),
    );
    const subs = [
      onMeetingSegment((e) => {
        setSegments((prev) => [...prev, e]);
        if (pinnedRef.current) {
          requestAnimationFrame(() => {
            scrollRef.current?.scrollTo({ top: scrollRef.current.scrollHeight });
          });
        }
      }),
      onMeetingStatus((e) => {
        setStatus(e);
        if (e.state === "error") setError(e.message ?? "capture failed");
      }),
      onMeetingFinished((e) => {
        setSessionId(null);
        if (e.saved) setSavedNote(true);
      }),
    ];
    return () => {
      window.removeEventListener("focus", refreshPermission);
      subs.forEach((p) => void p.then((un) => un()));
    };
  }, []);

  useEffect(() => {
    if (permitted) {
      void invoke<CapturableApp[]>("meeting_list_apps")
        .then((list) =>
          setApps(
            list
              .filter((a) => a.name && a.bundleId)
              .sort((a, b) => a.name.localeCompare(b.name)),
          ),
        )
        .catch((e) => setAppsError(String(e)));
    }
  }, [permitted]);

  const start = async () => {
    setError(null);
    setSavedNote(false);
    setSegments([]);
    try {
      const id = await invoke<number>("meeting_start", {
        args: {
          apps: selection === SYSTEM ? [] : [selection],
          system: selection === SYSTEM,
          mic,
          model_id: modelId,
          language,
        },
      });
      setSessionId(id);
    } catch (e) {
      setError(String(e));
    }
  };

  const stop = async () => {
    if (sessionId == null) return;
    try {
      await invoke("meeting_stop", { sessionId });
    } catch (e) {
      setError(String(e));
    }
  };

  const running = sessionId != null;

  return (
    <div className="flex w-full max-w-2xl flex-col gap-4">
      {permitted === false && (
        <div className="flex items-center justify-between gap-3 rounded-lg border border-amber-300 bg-amber-50 px-4 py-3 text-sm text-amber-900 dark:border-amber-700 dark:bg-amber-950 dark:text-amber-200">
          <span>
            Meeting capture needs the Screen Recording permission (that's how macOS
            grants access to app audio). After granting, quit and reopen Speakly.
          </span>
          <button
            className="shrink-0 rounded-md bg-amber-600 px-3 py-1.5 font-medium text-white hover:bg-amber-700"
            onClick={() => void invoke("open_screen_recording_settings")}
          >
            Open System Settings
          </button>
        </div>
      )}

      <div className="flex flex-wrap items-end gap-3 rounded-xl border border-neutral-200 p-4 dark:border-neutral-800">
        <label className="flex flex-col gap-1 text-xs text-neutral-500">
          Capture
          <select
            className="rounded-md border border-neutral-300 bg-white px-2 py-1.5 text-sm text-neutral-900 dark:border-neutral-700 dark:bg-neutral-800 dark:text-neutral-100"
            value={selection}
            onChange={(e) => setSelection(e.target.value)}
            disabled={running}
          >
            <option value={SYSTEM}>Entire system</option>
            {apps.map((a) => (
              <option key={`${a.bundleId}-${a.pid}`} value={a.bundleId}>
                {a.name}
              </option>
            ))}
          </select>
        </label>
        <label className="flex flex-col gap-1 text-xs text-neutral-500">
          Model
          <select
            className="rounded-md border border-neutral-300 bg-white px-2 py-1.5 text-sm text-neutral-900 dark:border-neutral-700 dark:bg-neutral-800 dark:text-neutral-100"
            value={modelId}
            onChange={(e) => setModelId(e.target.value)}
            disabled={running}
          >
            {models.map((m) => (
              <option key={m.id} value={m.id}>
                {m.id}
              </option>
            ))}
          </select>
        </label>
        <label className="flex flex-col gap-1 text-xs text-neutral-500">
          Language
          <select
            className="rounded-md border border-neutral-300 bg-white px-2 py-1.5 text-sm text-neutral-900 dark:border-neutral-700 dark:bg-neutral-800 dark:text-neutral-100"
            value={language}
            onChange={(e) => setLanguage(e.target.value)}
            disabled={running}
          >
            <option value="he">עברית</option>
            <option value="en">English</option>
          </select>
        </label>
        <label className="mb-1.5 flex items-center gap-2 text-sm text-neutral-700 dark:text-neutral-300">
          <input
            type="checkbox"
            checked={mic}
            onChange={(e) => setMic(e.target.checked)}
            disabled={running}
          />
          Include my microphone
        </label>
        <div className="ms-auto">
          {!running ? (
            <button
              className="rounded-md bg-accent px-4 py-2 text-sm font-medium text-white hover:bg-accent-strong disabled:opacity-50"
              onClick={() => void start()}
              disabled={permitted === false}
            >
              Start capture
            </button>
          ) : (
            <button
              className="rounded-md bg-red-600 px-4 py-2 text-sm font-medium text-white hover:bg-red-700"
              onClick={() => void stop()}
            >
              Stop
            </button>
          )}
        </div>
      </div>

      {appsError && <div className="text-sm text-red-600">{appsError}</div>}
      {error && <div className="text-sm text-red-600">{error}</div>}
      {running && (
        <div className="flex items-center gap-2 text-sm text-neutral-500">
          <span className="relative flex size-2">
            <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-red-400 opacity-75" />
            <span className="relative inline-flex size-2 rounded-full bg-red-500" />
          </span>
          {status?.state === "live" ? "Capturing — transcript updates every 15s" : "Starting…"}
          <label className="ms-auto flex items-center gap-1.5 text-xs">
            <input
              type="checkbox"
              checked={pinned}
              onChange={(e) => setPinned(e.target.checked)}
            />
            Follow live
          </label>
        </div>
      )}
      {savedNote && (
        <div className="rounded-lg border border-emerald-300 bg-emerald-50 px-4 py-2 text-sm text-emerald-900 dark:border-emerald-700 dark:bg-emerald-950 dark:text-emerald-200">
          Saved to History.
        </div>
      )}

      <div
        ref={scrollRef}
        className="max-h-96 overflow-y-auto rounded-xl border border-neutral-200 p-4 dark:border-neutral-800"
      >
        {segments.length === 0 ? (
          <div className="py-10 text-center text-sm text-neutral-400">
            {running
              ? "Listening — the first segment lands after ~15 seconds."
              : "Pick an app (or the entire system), then Start capture."}
          </div>
        ) : (
          <div className="flex flex-col gap-3">
            {segments.map((s, i) => (
              <div key={i}>
                <div className="mb-0.5 text-[11px] text-neutral-400">
                  {fmtTime(s.t0Ms)}–{fmtTime(s.t1Ms)}
                </div>
                <DirectionalText className="selectable text-[15px] leading-relaxed">
                  {s.text}
                </DirectionalText>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
