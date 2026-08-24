import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open } from "@tauri-apps/plugin-dialog";
import { attachJobEvents, useJobsStore, type FileJob } from "../../stores/jobs";
import { TranscriptView } from "./TranscriptView";

interface Language {
  code: string;
  label: string;
}

interface ModelInfo {
  id: string;
  name: string;
  installed: boolean;
}

const AUDIO_EXTENSIONS = [
  "wav", "mp3", "m4a", "m4b", "mp4", "mov", "m4v", "aac", "flac", "ogg", "oga",
  "alac", "caf", "webm", "mka", "mkv", "opus",
];

export function FilesView() {
  const [languages, setLanguages] = useState<Language[]>([]);
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [language, setLanguage] = useState("auto");
  const [modelId, setModelId] = useState<string | null>(null);
  const [dragOver, setDragOver] = useState(false);
  const [queueError, setQueueError] = useState<string | null>(null);
  const jobs = useJobsStore((s) => s.jobs);
  const order = useJobsStore((s) => s.order);
  const selectedId = useJobsStore((s) => s.selectedId);
  const [diarize, setDiarize] = useState(false);
  const [numSpeakers, setNumSpeakers] = useState<number | null>(null);
  const optionsRef = useRef({ language, modelId, diarize, numSpeakers });
  optionsRef.current = { language, modelId, diarize, numSpeakers };

  useEffect(() => {
    attachJobEvents();
    void invoke<Language[]>("list_languages").then(setLanguages);
    void invoke<{ models: ModelInfo[] }>("list_models").then((r) => {
      setModels(r.models);
      const installed = r.models.filter((m) => m.installed);
      const preferred =
        installed.find((m) => m.id === "he-turbo") ?? installed[0] ?? null;
      setModelId((current) => current ?? preferred?.id ?? null);
    });
    const unlisten = getCurrentWebview().onDragDropEvent((event) => {
      if (event.payload.type === "over" || event.payload.type === "enter") {
        setDragOver(true);
      } else if (event.payload.type === "leave") {
        setDragOver(false);
      } else if (event.payload.type === "drop") {
        setDragOver(false);
        void queuePaths(event.payload.paths);
      }
    });
    return () => {
      void unlisten.then((f) => f());
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const queuePaths = async (paths: string[]) => {
    const { language, modelId, diarize, numSpeakers } = optionsRef.current;
    if (!paths.length) return;
    if (!modelId) {
      setQueueError("Install a model in Models first.");
      return;
    }
    setQueueError(null);
    try {
      const ids = await invoke<string[]>("queue_file_jobs", {
        paths,
        language,
        modelId,
        diarize,
        numSpeakers,
      });
      useJobsStore.getState().addQueued(
        ids.map((id, i) => ({ id, path: paths[i], language, modelId })),
      );
    } catch (e) {
      setQueueError(String(e));
    }
  };

  const browse = async () => {
    const picked = await open({
      multiple: true,
      filters: [{ name: "Audio & Video", extensions: AUDIO_EXTENSIONS }],
    });
    if (picked) void queuePaths(Array.isArray(picked) ? picked : [picked]);
  };

  const retry = (job: FileJob) => {
    useJobsStore.getState().remove(job.id);
    void queuePaths([job.path]);
  };

  const selected = selectedId ? jobs[selectedId] : null;
  const hasCompleted = order.some((id) =>
    ["done", "error", "cancelled"].includes(jobs[id]?.status),
  );

  return (
    <div className="flex h-full w-full max-w-5xl gap-5">
      <div className="flex w-80 shrink-0 flex-col gap-3">
        <button
          onClick={() => void browse()}
          className={`rounded-xl border-2 border-dashed p-6 text-center text-sm transition-colors ${
            dragOver
              ? "border-accent bg-accent/10 text-accent-strong dark:text-accent"
              : "border-neutral-300 text-neutral-500 hover:border-neutral-400 dark:border-neutral-700"
          }`}
        >
          Drop audio or video files here
          <span className="mt-1 block text-xs text-neutral-400">or click to browse</span>
        </button>

        <div className="flex gap-2 text-sm">
          <select
            value={language}
            onChange={(e) => setLanguage(e.target.value)}
            className="min-w-0 flex-1 rounded-md border border-neutral-300 bg-white px-2 py-1.5 dark:border-neutral-700 dark:bg-neutral-800"
          >
            {languages.map((l) => (
              <option key={l.code} value={l.code}>
                {l.label}
              </option>
            ))}
          </select>
          <select
            value={modelId ?? ""}
            onChange={(e) => setModelId(e.target.value)}
            className="min-w-0 flex-1 rounded-md border border-neutral-300 bg-white px-2 py-1.5 dark:border-neutral-700 dark:bg-neutral-800"
          >
            {models
              .filter((m) => m.installed)
              .map((m) => (
                <option key={m.id} value={m.id}>
                  {m.name}
                </option>
              ))}
            {models.filter((m) => m.installed).length === 0 && (
              <option value="">No models installed</option>
            )}
          </select>
        </div>

        <div className="flex items-center gap-2 text-sm text-neutral-700 dark:text-neutral-300">
          <label className="flex items-center gap-2">
            <input
              type="checkbox"
              checked={diarize}
              onChange={(e) => setDiarize(e.target.checked)}
            />
            Identify speakers
          </label>
          {diarize && (
            <select
              value={numSpeakers ?? "auto"}
              onChange={(e) =>
                setNumSpeakers(e.target.value === "auto" ? null : Number(e.target.value))
              }
              className="rounded-md border border-neutral-300 bg-white px-2 py-1 text-xs dark:border-neutral-700 dark:bg-neutral-800"
            >
              <option value="auto">Auto count</option>
              {[2, 3, 4, 5, 6].map((n) => (
                <option key={n} value={n}>
                  {n} speakers
                </option>
              ))}
            </select>
          )}
        </div>

        {queueError && (
          <div className="rounded-md border border-red-300 bg-red-50 px-3 py-2 text-xs text-red-900 dark:border-red-700 dark:bg-red-950 dark:text-red-200">
            {queueError}
          </div>
        )}

        <div className="flex min-h-0 flex-1 flex-col gap-1.5 overflow-y-auto">
          {order.length === 0 && (
            <div className="py-6 text-center text-xs text-neutral-400">
              Queued files appear here.
            </div>
          )}
          {order.map((id) => {
            const job = jobs[id];
            if (!job) return null;
            return (
              <QueueRow
                key={id}
                job={job}
                selected={selectedId === id}
                onSelect={() => useJobsStore.getState().select(id)}
                onCancel={() => void invoke("cancel_job", { id })}
                onRetry={() => retry(job)}
                onRemove={() => useJobsStore.getState().remove(id)}
              />
            );
          })}
        </div>

        {hasCompleted && (
          <button
            onClick={() => useJobsStore.getState().clearCompleted()}
            className="rounded-md px-2 py-1 text-xs text-neutral-500 hover:bg-neutral-100 dark:hover:bg-neutral-800"
          >
            Clear completed
          </button>
        )}
      </div>

      <div className="min-w-0 flex-1">
        {selected ? (
          <TranscriptView job={selected} />
        ) : (
          <div className="flex h-full items-center justify-center rounded-xl border border-dashed border-neutral-300 text-sm text-neutral-400 dark:border-neutral-700">
            Select a file to see its transcript.
          </div>
        )}
      </div>
    </div>
  );
}

function QueueRow({
  job,
  selected,
  onSelect,
  onCancel,
  onRetry,
  onRemove,
}: {
  job: FileJob;
  selected: boolean;
  onSelect: () => void;
  onCancel: () => void;
  onRetry: () => void;
  onRemove: () => void;
}) {
  const running = job.status === "decoding" || job.status === "transcribing";
  return (
    <div
      onClick={onSelect}
      className={`cursor-default rounded-lg border p-2.5 ${
        selected
          ? "border-accent bg-accent/5"
          : "border-neutral-200 hover:border-neutral-300 dark:border-neutral-800 dark:hover:border-neutral-700"
      }`}
    >
      <div className="flex items-center justify-between gap-2">
        <span className="truncate text-sm" title={job.path}>
          {job.fileName}
        </span>
        <span className="shrink-0 text-xs text-neutral-400">
          {job.status === "queued" && "queued"}
          {job.status === "decoding" && "decoding…"}
          {job.status === "transcribing" && `${Math.round(job.pct * 100)}%`}
          {job.status === "done" && "✓"}
          {job.status === "error" && "failed"}
          {job.status === "cancelled" && "cancelled"}
        </span>
      </div>
      {running && (
        <div className="mt-1.5 h-1 overflow-hidden rounded-full bg-neutral-200 dark:bg-neutral-800">
          <div
            className="h-full rounded-full bg-accent transition-[width]"
            style={{ width: `${Math.max(4, Math.round(job.pct * 100))}%` }}
          />
        </div>
      )}
      {job.error && (
        <div className="mt-1 text-xs text-red-600 dark:text-red-400">{job.error}</div>
      )}
      <div className="mt-1.5 flex gap-2 text-xs">
        {running || job.status === "queued" ? (
          <button
            onClick={(e) => {
              e.stopPropagation();
              onCancel();
            }}
            className="text-neutral-500 hover:text-red-600"
          >
            Cancel
          </button>
        ) : (
          <>
            {(job.status === "error" || job.status === "cancelled") && (
              <button
                onClick={(e) => {
                  e.stopPropagation();
                  onRetry();
                }}
                className="text-neutral-500 hover:text-accent-strong"
              >
                Retry
              </button>
            )}
            <button
              onClick={(e) => {
                e.stopPropagation();
                onRemove();
              }}
              className="text-neutral-500 hover:text-red-600"
            >
              Remove
            </button>
          </>
        )}
      </div>
    </div>
  );
}
