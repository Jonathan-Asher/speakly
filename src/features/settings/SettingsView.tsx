import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import {
  checkPermissions,
  openPrivacyPane,
  type PermissionStatus,
} from "../../ipc/settings";
import { patchSettings, useSettingsStore } from "../../stores/settings";
import { strings } from "../../lib/strings";
import { UpdatesCard } from "./UpdatesCard";
import { DiagnosticsCard } from "./DiagnosticsCard";
import { AcknowledgementsCard } from "./AcknowledgementsCard";

function Card({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="rounded-xl border border-neutral-200 p-5 dark:border-neutral-800">
      <h2 className="mb-4 text-sm font-semibold text-neutral-500 uppercase tracking-wide">
        {title}
      </h2>
      <div className="flex flex-col gap-4">{children}</div>
    </section>
  );
}

function Row({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex items-center justify-between gap-6">
      <div>
        <div className="text-sm">{label}</div>
        {hint && <div className="text-xs text-neutral-400">{hint}</div>}
      </div>
      {children}
    </div>
  );
}

function Toggle({
  checked,
  onChange,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
}) {
  return (
    <button
      role="switch"
      aria-checked={checked}
      onClick={() => onChange(!checked)}
      className={`relative h-6 w-10 shrink-0 rounded-full transition-colors ${
        checked ? "bg-accent" : "bg-neutral-300 dark:bg-neutral-700"
      }`}
    >
      <span
        className={`absolute top-0.5 size-5 rounded-full bg-white shadow transition-all ${
          checked ? "start-[calc(100%-1.375rem)]" : "start-0.5"
        }`}
      />
    </button>
  );
}

export function SettingsView() {
  const settings = useSettingsStore((s) => s.settings);
  const [perms, setPerms] = useState<PermissionStatus | null>(null);
  const [version, setVersion] = useState("");

  useEffect(() => {
    const refresh = () => void checkPermissions().then(setPerms);
    refresh();
    window.addEventListener("focus", refresh);
    void getVersion().then(setVersion);
    return () => window.removeEventListener("focus", refresh);
  }, []);

  if (!settings) return null;
  const { general, history } = settings;

  return (
    <div className="flex w-full max-w-2xl flex-col gap-4">
      <Card title="General">
        <Row label="Launch at login" hint="Start Speakly when you sign in">
          <Toggle
            checked={general.launch_at_login}
            onChange={(v) => void patchSettings({ general: { launch_at_login: v } })}
          />
        </Row>
        <Row label="Show Dock icon" hint="Off = menu-bar only (no ⌘-Tab entry)">
          <Toggle
            checked={general.show_dock_icon}
            onChange={(v) => void patchSettings({ general: { show_dock_icon: v } })}
          />
        </Row>
        <Row label="Theme">
          <select
            value={general.theme}
            onChange={(e) => void patchSettings({ general: { theme: e.target.value } })}
            className="rounded-md border border-neutral-300 bg-white px-2 py-1.5 text-sm dark:border-neutral-700 dark:bg-neutral-800"
          >
            <option value="system">System</option>
            <option value="light">Light</option>
            <option value="dark">Dark</option>
          </select>
        </Row>
        <Row label="Sound feedback" hint="Subtle click when dictation starts and finishes">
          <Toggle
            checked={general.sound_feedback}
            onChange={(v) => void patchSettings({ general: { sound_feedback: v } })}
          />
        </Row>
      </Card>

      <Card title="Permissions">
        <Row label="Microphone" hint="Required for dictation">
          {perms?.microphone === "granted" ? (
            <span className="text-sm text-emerald-600 dark:text-emerald-400">Granted</span>
          ) : (
            <button
              onClick={() => void openPrivacyPane("microphone")}
              className="rounded-md border border-neutral-300 px-3 py-1.5 text-sm hover:bg-neutral-100 dark:border-neutral-700 dark:hover:bg-neutral-800"
            >
              Open System Settings
            </button>
          )}
        </Row>
        <Row label="Accessibility" hint="Lets Speakly press ⌘V to auto-paste">
          {perms?.accessibility ? (
            <span className="text-sm text-emerald-600 dark:text-emerald-400">Granted</span>
          ) : (
            <button
              onClick={() => void openPrivacyPane("accessibility")}
              className="rounded-md border border-neutral-300 px-3 py-1.5 text-sm hover:bg-neutral-100 dark:border-neutral-700 dark:hover:bg-neutral-800"
            >
              Open System Settings
            </button>
          )}
        </Row>
        <Row label="Setup assistant" hint="Walk through first-run setup again">
          <button
            onClick={() => void patchSettings({ onboarding: { completed: false } })}
            className="rounded-md border border-neutral-300 px-3 py-1.5 text-sm hover:bg-neutral-100 dark:border-neutral-700 dark:hover:bg-neutral-800"
          >
            Run again
          </button>
        </Row>
      </Card>

      <Card title="History">
        <Row label="Save history" hint="Keep transcripts in the local database">
          <Toggle
            checked={history.enabled}
            onChange={(v) => void patchSettings({ history: { enabled: v } })}
          />
        </Row>
        <Row label="Save dictation snippets" hint="The most privacy-sensitive kind">
          <Toggle
            checked={history.save_dictation}
            onChange={(v) => void patchSettings({ history: { save_dictation: v } })}
          />
        </Row>
        <Row label="Keep transcripts for" hint="Older entries are deleted daily">
          <select
            value={history.retention_days ?? ""}
            onChange={(e) =>
              void patchSettings({
                history: {
                  retention_days: e.target.value === "" ? null : Number(e.target.value),
                },
              })
            }
            className="rounded-md border border-neutral-300 bg-white px-2 py-1.5 text-sm dark:border-neutral-700 dark:bg-neutral-800"
          >
            <option value="">Forever</option>
            <option value="7">7 days</option>
            <option value="30">30 days</option>
            <option value="90">90 days</option>
            <option value="365">1 year</option>
          </select>
        </Row>
      </Card>

      <Card title="About">
        <Row label={strings.appName} hint={strings.tagline}>
          <span className="text-sm text-neutral-500">v{version}</span>
        </Row>
        <p className="text-xs text-neutral-400">
          Everything runs on your Mac. Translation, when enabled, sends only
          text to the provider you configured.
        </p>
      </Card>

      <UpdatesCard />
      <DiagnosticsCard />
      <AcknowledgementsCard />
    </div>
  );
}
