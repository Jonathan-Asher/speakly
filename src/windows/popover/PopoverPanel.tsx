import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { DirectionalText } from "../../components/DirectionalText";
import { attachDictationEvents, useDictationStore } from "../../stores/dictation";
import { onSettingsChanged } from "../../ipc/events";
import { prettyHotkey } from "../../lib/hotkey";

interface Profile {
  id: string;
  name: string;
  hotkey: string;
  translate?: {
    enabled: boolean;
    refine?: boolean;
    provider?: string;
    target_language: string;
    system_prompt?: string | null;
    model?: string | null;
    endpoint?: string | null;
  } | null;
}

interface HistoryItem {
  id: number;
  createdAt: number;
  text: string;
  translatedText: string | null;
}

const PHASE_LABEL: Record<string, string> = {
  idle: "Ready",
  listening: "Listening…",
  transcribing: "Transcribing…",
  translating: "Translating…",
  pasting: "Pasting…",
  pasted: "Pasted",
  copied: "Copied",
  error: "Error",
};

export function PopoverPanel() {
  const [profiles, setProfiles] = useState<Profile[]>([]);
  const [recent, setRecent] = useState<HistoryItem[]>([]);
  const [copiedId, setCopiedId] = useState<number | null>(null);
  const phase = useDictationStore((s) => s.phase);
  const last = useDictationStore((s) => s.last);

  const refetchProfiles = () => void invoke<Profile[]>("get_profiles").then(setProfiles);

  // Flip Refine without opening the main window; settings://changed refetches.
  const toggleRefine = (p: Profile) => {
    const c = p.translate;
    const next = {
      enabled: c?.enabled ?? false,
      refine: !(c?.refine ?? false),
      provider: c?.provider ?? "groq",
      target_language: c?.target_language ?? "English",
      ...(c?.system_prompt ? { system_prompt: c.system_prompt } : {}),
      ...(c?.model ? { model: c.model } : {}),
      ...(c?.endpoint ? { endpoint: c.endpoint } : {}),
    };
    void invoke("upsert_profile", { profile: { ...p, translate: next } }).then(
      refetchProfiles,
      () => refetchProfiles(),
    );
  };
  const refetchRecent = () =>
    void invoke<{ items: HistoryItem[] }>("history_search", { query: null, page: 0 })
      .then((r) => setRecent(r.items.slice(0, 3)))
      .catch(() => setRecent([]));

  useEffect(() => {
    attachDictationEvents();
    refetchProfiles();
    refetchRecent();
    const unlisten = onSettingsChanged(refetchProfiles);
    const onFocus = () => refetchRecent();
    window.addEventListener("focus", onFocus);
    return () => {
      window.removeEventListener("focus", onFocus);
      void unlisten.then((f) => f());
    };
  }, []);

  // A finished dictation while the popover is open refreshes the list.
  useEffect(() => {
    if (last) refetchRecent();
  }, [last]);

  const copy = (item: HistoryItem) => {
    void navigator.clipboard
      .writeText(item.translatedText ?? item.text)
      .then(() => {
        setCopiedId(item.id);
        setTimeout(() => setCopiedId(null), 1200);
      })
      .catch(() => {});
  };

  return (
    <div className="flex h-screen flex-col bg-transparent p-1.5">
      <div className="flex min-h-0 flex-1 flex-col rounded-2xl border border-neutral-200 bg-white/95 shadow-lg backdrop-blur dark:border-neutral-700 dark:bg-neutral-900/95">
        <div className="flex items-center justify-between border-b border-neutral-200 px-4 py-2.5 dark:border-neutral-800">
          <span className="text-sm font-semibold">Speakly</span>
          <span
            className={`text-xs font-medium ${
              phase === "listening"
                ? "text-red-500"
                : phase === "idle"
                  ? "text-neutral-400"
                  : "text-accent-strong dark:text-accent"
            }`}
          >
            {PHASE_LABEL[phase] ?? phase}
          </span>
        </div>

        <div className="flex flex-col gap-1 px-3 py-2">
          {profiles.map((p) => (
            <div
              key={p.id}
              className="flex items-center justify-between rounded-md px-1.5 py-1 text-sm"
            >
              <span className="truncate text-neutral-700 dark:text-neutral-300">
                {p.name}
                {p.translate?.enabled && (
                  <span className="ms-1.5 text-xs text-accent-strong dark:text-accent">
                    → {p.translate.target_language}
                  </span>
                )}
              </span>
              <span className="ms-2 flex shrink-0 items-center gap-1.5">
                <button
                  onClick={() => toggleRefine(p)}
                  title="Refine: clean up filler sounds before pasting"
                  className={`rounded-full px-1.5 py-0.5 text-[11px] transition-colors ${
                    p.translate?.refine
                      ? "bg-violet-500/15 text-violet-700 dark:text-violet-300"
                      : "text-neutral-400 hover:bg-neutral-100 dark:hover:bg-neutral-800"
                  }`}
                >
                  ✨
                </button>
                <kbd className="rounded border border-neutral-300 bg-neutral-100 px-1.5 py-0.5 text-[11px] text-neutral-600 dark:border-neutral-700 dark:bg-neutral-800 dark:text-neutral-300">
                  {prettyHotkey(p.hotkey)}
                </kbd>
              </span>
            </div>
          ))}
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto border-t border-neutral-200 px-3 py-2 dark:border-neutral-800">
          <div className="mb-1 px-1.5 text-[11px] font-medium uppercase tracking-wide text-neutral-400">
            Recent
          </div>
          {recent.length === 0 && (
            <div className="px-1.5 py-3 text-center text-xs text-neutral-400">
              Dictations show up here
            </div>
          )}
          {recent.map((item) => (
            <button
              key={item.id}
              onClick={() => copy(item)}
              title="Click to copy"
              className="block w-full rounded-md px-1.5 py-1.5 text-start hover:bg-neutral-100 dark:hover:bg-neutral-800"
            >
              <DirectionalText className="line-clamp-2 text-xs leading-relaxed text-neutral-700 dark:text-neutral-300">
                {item.translatedText ?? item.text}
              </DirectionalText>
              {copiedId === item.id && (
                <span className="text-[11px] text-emerald-500">Copied ✓</span>
              )}
            </button>
          ))}
        </div>

        <div className="flex items-center justify-between border-t border-neutral-200 px-3 py-2 dark:border-neutral-800">
          <button
            onClick={() => void invoke("show_main_window")}
            className="rounded-md px-2.5 py-1 text-xs font-medium text-neutral-700 hover:bg-neutral-100 dark:text-neutral-300 dark:hover:bg-neutral-800"
          >
            Open Speakly
          </button>
          <button
            onClick={() => void invoke("quit_app")}
            className="rounded-md px-2.5 py-1 text-xs text-neutral-500 hover:bg-neutral-100 dark:hover:bg-neutral-800"
          >
            Quit
          </button>
        </div>
      </div>
    </div>
  );
}
