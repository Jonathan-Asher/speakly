import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { DirectionalText } from "../../components/DirectionalText";
import { attachDictationEvents, useDictationStore } from "../../stores/dictation";
import { onSettingsChanged } from "../../ipc/events";
import { prettyHotkey } from "../../lib/hotkey";
import { newProfileDraft, ProfileEditor, type ProfileDraft } from "./ProfileEditor";

type Profile = ProfileDraft;

const PROVIDERS = [
  { id: "groq", label: "Groq" },
  { id: "anthropic", label: "Anthropic" },
  { id: "openai", label: "OpenAI" },
  { id: "google", label: "Google" },
  { id: "custom", label: "Custom" },
] as const;

interface ModelStatus {
  id: string;
  path: string;
  present: boolean;
}

export function DictationView() {
  const [profiles, setProfiles] = useState<Profile[]>([]);
  const [models, setModels] = useState<ModelStatus[]>([]);
  const [axTrusted, setAxTrusted] = useState<boolean | null>(null);
  const [editor, setEditor] = useState<{ draft: ProfileDraft; isNew: boolean } | null>(null);
  const phase = useDictationStore((s) => s.phase);
  const activeProfile = useDictationStore((s) => s.profileId);
  const last = useDictationStore((s) => s.last);
  const warning = useDictationStore((s) => s.warning);

  const refetchProfiles = useCallback(
    () => void invoke<Profile[]>("get_profiles").then(setProfiles),
    [],
  );

  useEffect(() => {
    attachDictationEvents();
    refetchProfiles();
    const unlisten = onSettingsChanged(refetchProfiles);
    void invoke<{ models: ModelStatus[] }>("get_model_status").then((r) =>
      setModels(r.models),
    );
    const refreshAx = () => void invoke<boolean>("accessibility_status").then(setAxTrusted);
    refreshAx();
    window.addEventListener("focus", refreshAx);
    return () => {
      window.removeEventListener("focus", refreshAx);
      void unlisten.then((f) => f());
    };
  }, [refetchProfiles]);

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
              <div className="mt-1 flex items-center gap-1.5 text-xs text-neutral-500">
                <span>
                  {p.mode === "toggle" ? "press to toggle" : "hold to talk"} · {p.language} ·{" "}
                  {p.model_id}
                </span>
                {p.translate?.enabled && (
                  <span className="rounded-full bg-accent/10 px-1.5 py-0.5 font-medium text-accent-strong dark:text-accent">
                    → {p.translate.target_language}
                  </span>
                )}
              </div>
              <div className="mt-2 flex items-center justify-between">
                {active ? (
                  <span className="text-xs font-medium text-accent-strong dark:text-accent">
                    {phase}…
                  </span>
                ) : (
                  <span />
                )}
                <button
                  onClick={() => setEditor({ draft: { ...p }, isNew: false })}
                  className="rounded-md px-2 py-0.5 text-xs text-neutral-500 hover:bg-neutral-100 hover:text-neutral-800 dark:hover:bg-neutral-800 dark:hover:text-neutral-200"
                >
                  Edit
                </button>
              </div>
            </div>
          );
        })}
        <button
          onClick={() => setEditor({ draft: newProfileDraft(), isNew: true })}
          className="flex min-h-24 items-center justify-center rounded-xl border border-dashed border-neutral-300 text-sm text-neutral-400 transition-colors hover:border-accent hover:text-accent-strong dark:border-neutral-700 dark:hover:text-accent"
        >
          + Add profile
        </button>
      </div>

      {editor && (
        <ProfileEditor
          initial={editor.draft}
          isNew={editor.isNew}
          onClose={() => setEditor(null)}
          onSaved={refetchProfiles}
        />
      )}

      <TranslationSettings />

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
              {last.translated ? ` · translated ${last.translateMs}ms` : ""}
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

interface KeyStatus {
  present: boolean;
  last4: string;
}

/** API keys for the translation stage. Keys go straight to the macOS
 * Keychain; the UI only ever sees `{present, last4}`. */
function TranslationSettings() {
  const [status, setStatus] = useState<Record<string, KeyStatus>>({});
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const [testResult, setTestResult] = useState<Record<string, string>>({});

  const refresh = (provider: string) =>
    void invoke<KeyStatus>("provider_key_status", { provider }).then((s) =>
      setStatus((prev) => ({ ...prev, [provider]: s })),
    );

  useEffect(() => {
    PROVIDERS.forEach((p) => refresh(p.id));
  }, []);

  const save = async (provider: string) => {
    const key = drafts[provider]?.trim();
    if (!key) return;
    try {
      await invoke("set_provider_key", { provider, key });
      setDrafts((prev) => ({ ...prev, [provider]: "" }));
      refresh(provider);
    } catch (e) {
      setTestResult((prev) => ({ ...prev, [provider]: String(e) }));
    }
  };

  const remove = async (provider: string) => {
    await invoke("delete_provider_key", { provider });
    setTestResult((prev) => ({ ...prev, [provider]: "" }));
    refresh(provider);
  };

  const test = async (provider: string) => {
    setTestResult((prev) => ({ ...prev, [provider]: "…" }));
    try {
      const result = await invoke<string>("test_translation", {
        provider,
        targetLanguage: "English",
      });
      setTestResult((prev) => ({ ...prev, [provider]: `✓ ${result}` }));
    } catch (e) {
      setTestResult((prev) => ({ ...prev, [provider]: String(e) }));
    }
  };

  return (
    <div className="rounded-xl border border-neutral-200 p-4 dark:border-neutral-800">
      <div className="mb-1 font-medium">Translation</div>
      <p className="mb-3 text-xs text-neutral-500">
        Powers profiles like Hebrew → English. Keys are stored in the macOS Keychain;
        only the dictated text is sent to the provider you choose.
      </p>
      <div className="flex flex-col gap-2">
        {PROVIDERS.map((p) => {
          const s = status[p.id];
          return (
            <div key={p.id} className="flex flex-col gap-1">
              <div className="flex items-center gap-2">
                <span className="w-24 shrink-0 text-sm">{p.label}</span>
                {s?.present ? (
                  <>
                    <span className="text-xs text-neutral-500">●●●{s.last4}</span>
                    <button
                      className="rounded-md border border-neutral-300 px-2 py-1 text-xs hover:bg-neutral-100 dark:border-neutral-700 dark:hover:bg-neutral-800"
                      onClick={() => void test(p.id)}
                    >
                      Test
                    </button>
                    <button
                      className="rounded-md border border-neutral-300 px-2 py-1 text-xs text-red-600 hover:bg-red-50 dark:border-neutral-700 dark:text-red-400 dark:hover:bg-red-950"
                      onClick={() => void remove(p.id)}
                    >
                      Delete
                    </button>
                  </>
                ) : (
                  <>
                    <input
                      type="password"
                      placeholder="API key"
                      value={drafts[p.id] ?? ""}
                      onChange={(e) =>
                        setDrafts((prev) => ({ ...prev, [p.id]: e.target.value }))
                      }
                      className="w-56 rounded-md border border-neutral-300 bg-transparent px-2 py-1 text-xs outline-none focus:border-accent dark:border-neutral-700"
                    />
                    <button
                      className="rounded-md bg-accent px-2.5 py-1 text-xs font-medium text-white hover:bg-accent-strong disabled:opacity-40"
                      disabled={!drafts[p.id]?.trim()}
                      onClick={() => void save(p.id)}
                    >
                      Save
                    </button>
                  </>
                )}
              </div>
              {testResult[p.id] && (
                <DirectionalText className="selectable ps-24 text-xs text-neutral-500">
                  {testResult[p.id]}
                </DirectionalText>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}
