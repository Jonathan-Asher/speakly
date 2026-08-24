import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  onModelError,
  onModelProgress,
  onModelReady,
  type ModelProgressEvent,
} from "../../ipc/events";

interface ModelCard {
  id: string;
  name: string;
  sizeBytes: number;
  languages: string;
  license: string;
  installed: boolean;
  path: string;
  downloading: boolean;
  usedBy: string[];
}

function gb(bytes: number) {
  return `${(bytes / 1_000_000_000).toFixed(2)} GB`;
}

function speed(bps: number) {
  if (bps <= 0) return "";
  return bps >= 1_000_000
    ? `${(bps / 1_000_000).toFixed(1)} MB/s`
    : `${Math.round(bps / 1_000)} KB/s`;
}

export function ModelsView() {
  const [models, setModels] = useState<ModelCard[]>([]);
  const [progress, setProgress] = useState<Record<string, ModelProgressEvent>>({});
  const [errors, setErrors] = useState<Record<string, string>>({});

  const refresh = useCallback(() => {
    void invoke<{ models: ModelCard[] }>("list_models").then((r) => setModels(r.models));
  }, []);

  useEffect(() => {
    refresh();
    const unsubs = [
      onModelProgress((e) => setProgress((p) => ({ ...p, [e.id]: e }))),
      onModelReady((e) => {
        setProgress((p) => {
          const next = { ...p };
          delete next[e.id];
          return next;
        });
        refresh();
      }),
      onModelError((e) => {
        setErrors((prev) => ({ ...prev, [e.id]: e.message }));
        setProgress((p) => {
          const next = { ...p };
          delete next[e.id];
          return next;
        });
        refresh();
      }),
    ];
    return () => {
      unsubs.forEach((u) => void u.then((f) => f()));
    };
  }, [refresh]);

  const download = (id: string) => {
    setErrors((prev) => {
      const next = { ...prev };
      delete next[id];
      return next;
    });
    void invoke("download_model", { id })
      .then(refresh)
      .catch((e) => setErrors((prev) => ({ ...prev, [id]: String(e) })));
  };

  const cancel = (id: string) => {
    void invoke("cancel_download", { id }).then(refresh);
  };

  const remove = (id: string) => {
    void invoke<{ warning: string | null }>("delete_model", { id })
      .then((r) => {
        if (r.warning) setErrors((prev) => ({ ...prev, [id]: r.warning! }));
        refresh();
      })
      .catch((e) => setErrors((prev) => ({ ...prev, [id]: String(e) })));
  };

  return (
    <div className="flex w-full max-w-2xl flex-col gap-3">
      {models.map((m) => {
        const p = progress[m.id];
        const downloading = m.downloading || Boolean(p);
        const pct = p?.total ? Math.min(100, (p.bytes / p.total) * 100) : null;
        return (
          <div
            key={m.id}
            className="rounded-xl border border-neutral-200 p-4 dark:border-neutral-800"
          >
            <div className="flex items-start justify-between gap-3">
              <div className="min-w-0">
                <div className="flex items-center gap-2">
                  <span className="font-medium">{m.name}</span>
                  {m.usedBy.map((u) => (
                    <span
                      key={u}
                      className="rounded-full bg-accent/10 px-2 py-0.5 text-[11px] font-medium text-accent-strong dark:text-accent"
                    >
                      {u}
                    </span>
                  ))}
                </div>
                <div className="mt-0.5 text-xs text-neutral-500">
                  {gb(m.sizeBytes)} · {m.languages} · {m.license}
                </div>
                {m.installed && (
                  <div className="mt-1 truncate text-[11px] text-neutral-400" title={m.path}>
                    {m.path}
                  </div>
                )}
              </div>
              <div className="flex shrink-0 items-center gap-2">
                {m.installed && !downloading && (
                  <>
                    <span className="text-xs font-medium text-emerald-600 dark:text-emerald-400">
                      Installed
                    </span>
                    <button
                      onClick={() => remove(m.id)}
                      className="rounded-md border border-neutral-300 px-2.5 py-1 text-xs text-neutral-600 hover:bg-neutral-100 dark:border-neutral-700 dark:text-neutral-300 dark:hover:bg-neutral-800"
                    >
                      Delete
                    </button>
                  </>
                )}
                {!m.installed && !downloading && (
                  <button
                    onClick={() => download(m.id)}
                    className="rounded-md bg-accent px-3 py-1.5 text-xs font-medium text-white hover:bg-accent-strong"
                  >
                    Download
                  </button>
                )}
                {downloading && (
                  <button
                    onClick={() => cancel(m.id)}
                    className="rounded-md border border-neutral-300 px-2.5 py-1 text-xs text-neutral-600 hover:bg-neutral-100 dark:border-neutral-700 dark:text-neutral-300 dark:hover:bg-neutral-800"
                  >
                    Cancel
                  </button>
                )}
              </div>
            </div>

            {downloading && (
              <div className="mt-3">
                <div className="h-1.5 w-full overflow-hidden rounded-full bg-neutral-200 dark:bg-neutral-800">
                  <div
                    className={`h-full rounded-full bg-accent transition-[width] ${pct === null ? "w-1/3 animate-pulse" : ""}`}
                    style={pct === null ? undefined : { width: `${pct}%` }}
                  />
                </div>
                <div className="mt-1 flex justify-between text-[11px] text-neutral-500">
                  <span>
                    {p ? gb(p.bytes) : "…"}
                    {p?.total ? ` / ${gb(p.total)}` : ""}
                  </span>
                  <span>{p ? speed(p.bps) : "starting…"}</span>
                </div>
              </div>
            )}

            {errors[m.id] && !downloading && (
              <div className="mt-2 text-xs text-amber-600 dark:text-amber-400">
                {errors[m.id]}
              </div>
            )}
          </div>
        );
      })}
      <p className="mt-2 text-xs text-neutral-400">
        Downloads resume automatically if interrupted. Models live in Application
        Support and run fully on-device.
      </p>
    </div>
  );
}
