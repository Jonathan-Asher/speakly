import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { patchSettings } from "../../stores/settings";
import type { MicChoice } from "../../ipc/settings";

interface AudioDevice {
  id: string;
  name: string;
}

interface AudioDevices {
  devices: AudioDevice[];
  systemDefault: AudioDevice | null;
}

/** Microphone priority order. Speakly records from the first entry that is
 * actually connected, so a headset that drops off Bluetooth falls through to
 * the next one instead of taking the dictation with it. An empty list means
 * "whatever macOS is set to", which is why the OS default is named here. */
export function MicrophoneCard({ priority }: { priority: MicChoice[] }) {
  const [available, setAvailable] = useState<AudioDevices>({
    devices: [],
    systemDefault: null,
  });

  useEffect(() => {
    const load = () =>
      void invoke<AudioDevices>("list_audio_devices").then(setAvailable);
    load();
    // Microphones get plugged and unplugged while the window sits open.
    window.addEventListener("focus", load);
    return () => window.removeEventListener("focus", load);
  }, []);

  const { devices, systemDefault } = available;
  const connected = useCallback(
    (id: string) => devices.some((d) => d.id === id),
    [devices],
  );
  const save = (next: MicChoice[]) =>
    void patchSettings({ general: { mic_priority: next } });

  const move = (from: number, to: number) => {
    if (to < 0 || to >= priority.length) return;
    const next = [...priority];
    const [moved] = next.splice(from, 1);
    next.splice(to, 0, moved);
    save(next);
  };

  // The one a dictation started right now would actually use.
  const inUse = priority.find((m) => connected(m.id))?.id ?? null;
  const unlisted = devices.filter((d) => !priority.some((m) => m.id === d.id));

  return (
    <section className="rounded-xl border border-neutral-200 p-5 dark:border-neutral-800">
      <h2 className="text-sm font-semibold text-neutral-500 uppercase tracking-wide">
        Microphone
      </h2>
      <p className="mt-1 text-xs text-neutral-400">
        Speakly records from the first one that is connected. If it disconnects
        mid-sentence, it moves to the next.
      </p>

      <ul className="mt-4 flex flex-col gap-1">
        {priority.map((mic, i) => {
          const live = devices.find((d) => d.id === mic.id);
          const active = mic.id === inUse;
          return (
            <li
              key={mic.id}
              className={`flex items-center gap-3 rounded-lg border px-3 py-2 ${
                active
                  ? "border-accent/40 bg-accent/5"
                  : "border-neutral-200 dark:border-neutral-800"
              }`}
            >
              <span className="w-4 shrink-0 text-xs tabular-nums text-neutral-400">
                {i + 1}
              </span>
              <span
                aria-hidden
                className={`size-2 shrink-0 rounded-full ${
                  live ? "bg-emerald-500" : "bg-neutral-300 dark:bg-neutral-600"
                }`}
              />
              <span className="min-w-0 flex-1 truncate text-sm">
                {live?.name ?? (mic.name || mic.id)}
              </span>
              <span className="shrink-0 text-xs text-neutral-400">
                {active ? "In use" : live ? "Connected" : "Not connected"}
              </span>
              <div className="flex shrink-0 items-center gap-0.5">
                <Icon
                  label={`Move ${mic.name} up`}
                  disabled={i === 0}
                  onClick={() => move(i, i - 1)}
                >
                  ↑
                </Icon>
                <Icon
                  label={`Move ${mic.name} down`}
                  disabled={i === priority.length - 1}
                  onClick={() => move(i, i + 1)}
                >
                  ↓
                </Icon>
                <Icon
                  label={`Remove ${mic.name}`}
                  onClick={() => save(priority.filter((m) => m.id !== mic.id))}
                >
                  ✕
                </Icon>
              </div>
            </li>
          );
        })}
        {/* Always the last resort, whatever the list says — so show it as the
            final link in the chain rather than leaving it implied. */}
        <li
          className={`flex items-center gap-3 rounded-lg border border-dashed px-3 py-2 ${
            inUse === null
              ? "border-accent/40 bg-accent/5"
              : "border-neutral-200 dark:border-neutral-800"
          }`}
        >
          <span className="w-4 shrink-0 text-xs tabular-nums text-neutral-400">
            {priority.length + 1}
          </span>
          <span
            aria-hidden
            className="size-2 shrink-0 rounded-full bg-neutral-300 dark:bg-neutral-600"
          />
          <span className="min-w-0 flex-1 truncate text-sm text-neutral-500">
            System default
            {systemDefault && (
              <span className="text-neutral-400"> ({systemDefault.name})</span>
            )}
          </span>
          <span className="shrink-0 text-xs text-neutral-400">
            {inUse === null ? "In use" : "Fallback"}
          </span>
        </li>
      </ul>

      <div className="mt-3 flex items-center justify-between gap-4">
        <span className="text-xs text-neutral-400">
          {priority.length === 0 && "Add one to override what macOS picks."}
        </span>
        <select
          value=""
          onChange={(e) => {
            const picked = devices.find((d) => d.id === e.target.value);
            if (picked) save([...priority, { id: picked.id, name: picked.name }]);
          }}
          disabled={unlisted.length === 0}
          className="max-w-64 rounded-md border border-neutral-300 bg-white px-2 py-1.5 text-sm disabled:opacity-40 dark:border-neutral-700 dark:bg-neutral-800"
        >
          <option value="">
            {unlisted.length === 0 ? "All microphones listed" : "Add a microphone…"}
          </option>
          {unlisted.map((d) => (
            <option key={d.id} value={d.id}>
              {d.name}
              {d.id === systemDefault?.id ? " (system default)" : ""}
            </option>
          ))}
        </select>
      </div>
    </section>
  );
}

function Icon({
  label,
  disabled,
  onClick,
  children,
}: {
  label: string;
  disabled?: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      disabled={disabled}
      onClick={onClick}
      className="size-6 rounded text-xs text-neutral-500 hover:bg-neutral-100 disabled:opacity-25 disabled:hover:bg-transparent dark:hover:bg-neutral-800"
    >
      {children}
    </button>
  );
}
