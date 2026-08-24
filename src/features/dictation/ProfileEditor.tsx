import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { HotkeyRecorder } from "./HotkeyRecorder";

export interface TranslateDraft {
  enabled: boolean;
  provider: string;
  target_language: string;
  system_prompt?: string | null;
  model?: string | null;
  endpoint?: string | null;
}

export interface ProfileDraft {
  id: string;
  name: string;
  hotkey: string;
  mode: "hold" | "toggle";
  language: string;
  model_id: string;
  translate: TranslateDraft | null;
  auto_paste: boolean;
  restore_clipboard: boolean;
}

interface Language {
  code: string;
  label: string;
}

interface ModelOption {
  id: string;
  name: string;
  installed: boolean;
}

const PROVIDER_OPTIONS = ["groq", "anthropic", "openai", "google", "custom"];

export function newProfileDraft(): ProfileDraft {
  return {
    id: crypto.randomUUID(),
    name: "",
    hotkey: "",
    mode: "hold",
    language: "he",
    model_id: "he-turbo",
    translate: null,
    auto_paste: true,
    restore_clipboard: true,
  };
}

function Row({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <label className="flex items-center justify-between gap-4 text-sm">
      <span className="w-32 shrink-0 text-neutral-600 dark:text-neutral-400">{label}</span>
      <div className="flex flex-1 justify-end">{children}</div>
    </label>
  );
}

const inputCls =
  "w-full max-w-56 rounded-md border border-neutral-300 bg-white px-2.5 py-1.5 text-sm dark:border-neutral-700 dark:bg-neutral-800";

export function ProfileEditor({
  initial,
  isNew,
  onClose,
  onSaved,
}: {
  initial: ProfileDraft;
  isNew: boolean;
  onClose: () => void;
  onSaved: () => void;
}) {
  const [draft, setDraft] = useState<ProfileDraft>(initial);
  const [languages, setLanguages] = useState<Language[]>([]);
  const [models, setModels] = useState<ModelOption[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    void invoke<Language[]>("list_languages").then((r) =>
      // Dictation needs a pinned language; auto-detect is a file-jobs option.
      setLanguages(r.filter((l) => l.code !== "auto")),
    );
    void invoke<{ models: ModelOption[] }>("list_models").then((r) =>
      setModels(r.models.filter((m) => m.installed)),
    );
  }, []);

  const set = (patch: Partial<ProfileDraft>) => setDraft((d) => ({ ...d, ...patch }));
  const setTranslate = (patch: Partial<TranslateDraft>) =>
    setDraft((d) => ({
      ...d,
      translate: {
        enabled: false,
        provider: "groq",
        target_language: "English",
        ...(d.translate ?? {}),
        ...patch,
      },
    }));

  const save = async () => {
    setBusy(true);
    setError(null);
    try {
      await invoke("upsert_profile", { profile: draft });
      onSaved();
      onClose();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const remove = async () => {
    if (!window.confirm(`Delete profile "${draft.name}"?`)) return;
    setBusy(true);
    setError(null);
    try {
      await invoke("delete_profile", { id: draft.id });
      onSaved();
      onClose();
    } catch (e) {
      setError(String(e));
      setBusy(false);
    }
  };

  const translateOn = draft.translate?.enabled ?? false;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-6"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div className="max-h-full w-full max-w-md overflow-y-auto rounded-2xl border border-neutral-200 bg-white p-5 shadow-xl dark:border-neutral-700 dark:bg-neutral-900">
        <div className="mb-4 flex items-center justify-between">
          <h2 className="text-base font-semibold">
            {isNew ? "New profile" : `Edit ${initial.name}`}
          </h2>
          <button
            onClick={onClose}
            className="rounded-md px-2 py-1 text-neutral-500 hover:bg-neutral-100 dark:hover:bg-neutral-800"
          >
            ✕
          </button>
        </div>

        <div className="flex flex-col gap-3">
          <Row label="Name">
            <input
              className={inputCls}
              value={draft.name}
              placeholder="e.g. Hebrew → English"
              onChange={(e) => set({ name: e.target.value })}
            />
          </Row>
          <Row label="Hotkey">
            <HotkeyRecorder value={draft.hotkey} onChange={(hotkey) => set({ hotkey })} />
          </Row>
          <Row label="Mode">
            <select
              className={inputCls}
              value={draft.mode}
              onChange={(e) => set({ mode: e.target.value as "hold" | "toggle" })}
            >
              <option value="hold">Hold to talk</option>
              <option value="toggle">Press to toggle</option>
            </select>
          </Row>
          <Row label="Language">
            <select
              className={inputCls}
              value={draft.language}
              onChange={(e) => set({ language: e.target.value })}
            >
              {languages.map((l) => (
                <option key={l.code} value={l.code}>
                  {l.label}
                </option>
              ))}
            </select>
          </Row>
          <Row label="Model">
            <select
              className={inputCls}
              value={draft.model_id}
              onChange={(e) => set({ model_id: e.target.value })}
            >
              {models.map((m) => (
                <option key={m.id} value={m.id}>
                  {m.name}
                </option>
              ))}
            </select>
          </Row>

          <div className="mt-1 border-t border-neutral-200 pt-3 dark:border-neutral-800">
            <Row label="Translate">
              <input
                type="checkbox"
                checked={translateOn}
                onChange={(e) => setTranslate({ enabled: e.target.checked })}
              />
            </Row>
            {translateOn && (
              <div className="mt-3 flex flex-col gap-3">
                <Row label="Provider">
                  <select
                    className={inputCls}
                    value={draft.translate?.provider ?? "groq"}
                    onChange={(e) => setTranslate({ provider: e.target.value })}
                  >
                    {PROVIDER_OPTIONS.map((p) => (
                      <option key={p} value={p}>
                        {p}
                      </option>
                    ))}
                  </select>
                </Row>
                <Row label="Target language">
                  <input
                    className={inputCls}
                    value={draft.translate?.target_language ?? ""}
                    placeholder="English"
                    onChange={(e) => setTranslate({ target_language: e.target.value })}
                  />
                </Row>
                <Row label="Model override">
                  <input
                    className={inputCls}
                    value={draft.translate?.model ?? ""}
                    placeholder="provider default"
                    onChange={(e) => setTranslate({ model: e.target.value || null })}
                  />
                </Row>
                {draft.translate?.provider === "custom" && (
                  <Row label="Endpoint">
                    <input
                      className={inputCls}
                      value={draft.translate?.endpoint ?? ""}
                      placeholder="https://…/v1"
                      onChange={(e) => setTranslate({ endpoint: e.target.value || null })}
                    />
                  </Row>
                )}
              </div>
            )}
          </div>

          <div className="border-t border-neutral-200 pt-3 dark:border-neutral-800">
            <Row label="Auto-paste">
              <input
                type="checkbox"
                checked={draft.auto_paste}
                onChange={(e) => set({ auto_paste: e.target.checked })}
              />
            </Row>
            <div className="mt-3">
              <Row label="Restore clipboard">
                <input
                  type="checkbox"
                  checked={draft.restore_clipboard}
                  onChange={(e) => set({ restore_clipboard: e.target.checked })}
                />
              </Row>
            </div>
          </div>

          {error && (
            <div className="rounded-md border border-red-300 bg-red-50 px-3 py-2 text-sm text-red-800 dark:border-red-700 dark:bg-red-950 dark:text-red-200">
              {error}
            </div>
          )}

          <div className="mt-2 flex items-center justify-between">
            {!isNew ? (
              <button
                onClick={() => void remove()}
                disabled={busy}
                className="rounded-md px-3 py-1.5 text-sm text-red-600 hover:bg-red-50 disabled:opacity-50 dark:text-red-400 dark:hover:bg-red-950"
              >
                Delete
              </button>
            ) : (
              <span />
            )}
            <div className="flex gap-2">
              <button
                onClick={onClose}
                className="rounded-md px-3 py-1.5 text-sm text-neutral-600 hover:bg-neutral-100 dark:text-neutral-300 dark:hover:bg-neutral-800"
              >
                Cancel
              </button>
              <button
                onClick={() => void save()}
                disabled={busy || !draft.hotkey}
                className="rounded-md bg-accent-strong px-4 py-1.5 text-sm font-medium text-white hover:bg-accent disabled:opacity-50"
              >
                Save
              </button>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
