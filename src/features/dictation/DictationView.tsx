import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { DirectionalText } from "../../components/DirectionalText";
import { attachDictationEvents, useDictationStore } from "../../stores/dictation";

interface Profile {
  id: string;
  name: string;
  hotkey: string;
  language: string;
  model_id: string;
}

interface ModelStatus {
  id: string;
  path: string;
  present: boolean;
}

function prettyHotkey(hotkey: string) {
  return hotkey
    .replace(/Alt/g, "⌥")
    .replace(/Shift/g, "⇧")
    .replace(/(CommandOrControl|Command|Super)/g, "⌘")
    .replace(/Control|Ctrl/g, "⌃")
    .replace(/\+/g, " ");
}

export function DictationView() {
  const [profiles, setProfiles] = useState<Profile[]>([]);
  const [models, setModels] = useState<ModelStatus[]>([]);
  const [axTrusted, setAxTrusted] = useState<boolean | null>(null);
  const phase = useDictationStore((s) => s.phase);
  const activeProfile = useDictationStore((s) => s.profileId);
  const last = useDictationStore((s) => s.last);
  const warning = useDictationStore((s) => s.warning);

  useEffect(() => {
    attachDictationEvents();
    void invoke<Profile[]>("get_profiles").then(setProfiles);
    void invoke<{ models: ModelStatus[] }>("get_model_status").then((r) =>
      setModels(r.models),
    );
    const refreshAx = () => void invoke<boolean>("accessibility_status").then(setAxTrusted);
    refreshAx();
    window.addEventListener("focus", refreshAx);
    return () => window.removeEventListener("focus", refreshAx);
  }, []);

  const missingModels = models.filter((m) => !m.present);

  return (
    <div className="flex w-full max-w-2xl flex-col gap-4">
      {axTrusted === false && (
        <div className="flex items-center justify-between gap-3 rounded-lg border border-amber-300 bg-amber-50 px-4 py-3 text-sm text-amber-900 dark:border-amber-700 dark:bg-amber-950 dark:text-amber-200">
          <span>
            Auto-paste needs the Accessibility permission. Until then, dictations are
            copied for manual ⌘V.
          </span>
          <button
            className="shrink-0 rounded-md bg-amber-600 px-3 py-1.5 font-medium text-white hover:bg-amber-700"
            onClick={() => void invoke("open_accessibility_settings")}
          >
            Open System Settings
          </button>
        </div>
      )}
      {missingModels.length > 0 && (
        <div className="rounded-lg border border-red-300 bg-red-50 px-4 py-3 text-sm text-red-900 dark:border-red-700 dark:bg-red-950 dark:text-red-200">
          Missing model files: {missingModels.map((m) => m.id).join(", ")} — the model
          manager arrives next; until then the profile using them won't start.
        </div>
      )}

      <div className="grid grid-cols-2 gap-3">
        {profiles.map((p) => {
          const active = activeProfile === p.id && phase !== "idle";
          return (
            <div
              key={p.id}
              className={`rounded-xl border p-4 transition-colors ${
                active
                  ? "border-accent bg-accent/5"
                  : "border-neutral-200 dark:border-neutral-800"
              }`}
            >
              <div className="flex items-center justify-between">
                <span className="font-medium">{p.name}</span>
                <kbd className="rounded-md border border-neutral-300 bg-neutral-100 px-2 py-0.5 text-xs text-neutral-600 dark:border-neutral-700 dark:bg-neutral-800 dark:text-neutral-300">
                  {prettyHotkey(p.hotkey)}
                </kbd>
              </div>
              <div className="mt-1 text-xs text-neutral-500">
                hold to talk · {p.language} · {p.model_id}
              </div>
              {active && (
                <div className="mt-2 text-xs font-medium text-accent-strong dark:text-accent">
                  {phase}…
                </div>
              )}
            </div>
          );
        })}
      </div>

      {warning && phase === "idle" && (
        <div className="text-sm text-neutral-500">{warning}</div>
      )}

      {last && (
        <div className="rounded-xl border border-neutral-200 p-4 dark:border-neutral-800">
          <div className="mb-2 flex items-center justify-between text-xs text-neutral-500">
            <span>Last dictation {last.phase === "copied" ? "(copied)" : "(pasted)"}</span>
            <span>
              {(last.utteranceMs / 1000).toFixed(1)}s spoken · decoded {last.decodeMs}ms ·
              total {last.latencyMs}ms
            </span>
          </div>
          <DirectionalText className="selectable text-[15px] leading-relaxed">
            {last.text}
          </DirectionalText>
          {last.note && (
            <div className="mt-2 text-xs text-amber-600 dark:text-amber-400">{last.note}</div>
          )}
        </div>
      )}

      {!last && (
        <div className="rounded-xl border border-dashed border-neutral-300 p-8 text-center text-sm text-neutral-400 dark:border-neutral-700">
          Hold a profile hotkey anywhere, speak, release — the text lands at your cursor.
        </div>
      )}
    </div>
  );
}
