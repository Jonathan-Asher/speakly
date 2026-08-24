import { useCallback, useEffect, useState } from "react";
import {
  checkPermissions,
  openPrivacyPane,
  probeMicrophone,
  type PermissionStatus,
} from "../../ipc/settings";
import { patchSettings } from "../../stores/settings";
import { attachDictationEvents, useDictationStore } from "../../stores/dictation";
import { strings } from "../../lib/strings";

const STEPS = ["Welcome", "Microphone", "Auto-paste", "Hotkeys", "Try it"] as const;

/** First-run flow. All steps are revisitable; finishing sets
 * `onboarding.completed` and drops the user into the main app. */
export function OnboardingWizard() {
  const [step, setStep] = useState(0);
  const [perms, setPerms] = useState<PermissionStatus | null>(null);

  const refresh = useCallback(() => {
    void checkPermissions().then(setPerms);
  }, []);

  // Poll while the permission steps are visible: TCC has no change
  // notification, and grants happen in System Settings out of our sight.
  useEffect(() => {
    refresh();
    window.addEventListener("focus", refresh);
    const interval =
      step === 1 || step === 2 ? window.setInterval(refresh, 1000) : undefined;
    return () => {
      window.removeEventListener("focus", refresh);
      if (interval) window.clearInterval(interval);
    };
  }, [step, refresh]);

  const finish = () => void patchSettings({ onboarding: { completed: true } });

  return (
    <div className="flex h-full flex-col items-center justify-center gap-8 p-10">
      <div className="w-full max-w-lg">
        {step === 0 && <WelcomeStep />}
        {step === 1 && <MicrophoneStep perms={perms} refresh={refresh} />}
        {step === 2 && <AccessibilityStep perms={perms} />}
        {step === 3 && <HotkeysStep />}
        {step === 4 && <TryItStep />}
      </div>

      <div className="flex items-center gap-2">
        {STEPS.map((label, i) => (
          <button
            key={label}
            aria-label={label}
            onClick={() => setStep(i)}
            className={`size-2 rounded-full transition-colors ${
              i === step ? "bg-accent" : "bg-neutral-300 dark:bg-neutral-700"
            }`}
          />
        ))}
      </div>

      <div className="flex w-full max-w-lg items-center justify-between">
        <button
          onClick={() => setStep((s) => Math.max(0, s - 1))}
          className={`rounded-md px-4 py-2 text-sm text-neutral-500 hover:bg-neutral-100 dark:hover:bg-neutral-800 ${
            step === 0 ? "invisible" : ""
          }`}
        >
          Back
        </button>
        <div className="flex items-center gap-3">
          {step === 2 && !perms?.accessibility && (
            <button
              onClick={() => setStep(3)}
              className="rounded-md px-4 py-2 text-sm text-neutral-500 hover:bg-neutral-100 dark:hover:bg-neutral-800"
            >
              Skip for now
            </button>
          )}
          {step < STEPS.length - 1 ? (
            <button
              onClick={() => setStep((s) => s + 1)}
              className="rounded-md bg-accent px-5 py-2 text-sm font-medium text-white hover:bg-accent-strong"
            >
              Continue
            </button>
          ) : (
            <button
              onClick={finish}
              className="rounded-md bg-accent px-5 py-2 text-sm font-medium text-white hover:bg-accent-strong"
            >
              Start using {strings.appName}
            </button>
          )}
        </div>
      </div>
    </div>
  );
}

function StepShell({
  title,
  children,
}: {
  title: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex flex-col gap-4">
      <h1 className="text-2xl font-semibold">{title}</h1>
      {children}
    </div>
  );
}

function WelcomeStep() {
  return (
    <StepShell title={`Welcome to ${strings.appName}`}>
      <p className="text-neutral-600 dark:text-neutral-300">
        Hold a hotkey, speak, release — your words are typed at the cursor in
        any app. Transcription runs entirely on your Mac: no accounts, no
        cloud, no telemetry.
      </p>
      <div className="flex gap-4 pt-2">
        {["Hebrew", "English"].map((lang) => (
          <label
            key={lang}
            className="flex items-center gap-2 rounded-lg border border-neutral-200 px-4 py-2.5 text-sm dark:border-neutral-800"
          >
            <input type="checkbox" defaultChecked className="accent-accent" />
            {lang}
          </label>
        ))}
      </div>
      <p className="text-xs text-neutral-400">
        Both are set up out of the box — including a Hebrew → English profile.
      </p>
    </StepShell>
  );
}

function StatusChip({ ok, label }: { ok: boolean; label: string }) {
  return (
    <span
      className={`rounded-full px-2.5 py-1 text-xs font-medium ${
        ok
          ? "bg-emerald-100 text-emerald-700 dark:bg-emerald-950 dark:text-emerald-300"
          : "bg-neutral-100 text-neutral-500 dark:bg-neutral-800 dark:text-neutral-400"
      }`}
    >
      {label}
    </span>
  );
}

function MicrophoneStep({
  perms,
  refresh,
}: {
  perms: PermissionStatus | null;
  refresh: () => void;
}) {
  const [probing, setProbing] = useState(false);
  const status = perms?.microphone ?? "unknown";

  const probe = async () => {
    setProbing(true);
    try {
      await probeMicrophone();
    } finally {
      setProbing(false);
      refresh();
    }
  };

  return (
    <StepShell title="Microphone">
      <p className="text-neutral-600 dark:text-neutral-300">
        {strings.appName} listens only while you hold your hotkey. Audio never
        leaves this Mac.
      </p>
      <div className="flex items-center gap-3">
        <StatusChip
          ok={status === "granted"}
          label={
            status === "granted"
              ? "Access granted"
              : status === "denied"
                ? "Access denied"
                : "Not yet requested"
          }
        />
        {status !== "granted" && status !== "denied" && (
          <button
            onClick={() => void probe()}
            disabled={probing}
            className="rounded-md bg-accent px-4 py-2 text-sm font-medium text-white hover:bg-accent-strong disabled:opacity-50"
          >
            {probing ? "Requesting…" : "Enable microphone"}
          </button>
        )}
      </div>
      {status === "denied" && (
        <div className="rounded-lg border border-amber-300 bg-amber-50 p-4 text-sm text-amber-900 dark:border-amber-700 dark:bg-amber-950 dark:text-amber-200">
          Microphone access was denied. Enable {strings.appName} under Privacy
          &amp; Security → Microphone, then come back — this screen updates by
          itself.
          <button
            onClick={() => void openPrivacyPane("microphone")}
            className="mt-3 block rounded-md bg-amber-600 px-3 py-1.5 font-medium text-white hover:bg-amber-700"
          >
            Open System Settings
          </button>
        </div>
      )}
    </StepShell>
  );
}

function AccessibilityStep({ perms }: { perms: PermissionStatus | null }) {
  const granted = perms?.accessibility ?? false;
  return (
    <StepShell title="Auto-paste">
      <p className="text-neutral-600 dark:text-neutral-300">
        The Accessibility permission lets {strings.appName} press ⌘V for you,
        so transcribed text lands directly at your cursor.
      </p>
      <div className="flex items-center gap-3">
        <StatusChip ok={granted} label={granted ? "Granted" : "Not granted"} />
        {!granted && (
          <button
            onClick={() => void openPrivacyPane("accessibility")}
            className="rounded-md bg-accent px-4 py-2 text-sm font-medium text-white hover:bg-accent-strong"
          >
            Open System Settings
          </button>
        )}
      </div>
      {!granted && (
        <p className="text-xs text-neutral-400">
          Add {strings.appName} to Privacy &amp; Security → Accessibility. You
          can skip this — dictations are then copied to the clipboard and you
          press ⌘V yourself.
        </p>
      )}
    </StepShell>
  );
}

function HotkeysStep() {
  const profiles = [
    { name: "Hebrew", keys: "⌥ Space", detail: "Hebrew in, Hebrew out" },
    { name: "English", keys: "⇧ ⌥ Space", detail: "English in, English out" },
    {
      name: "Hebrew → English",
      keys: "⌃ ⌥ Space",
      detail: "speak Hebrew, English is pasted",
    },
  ];
  return (
    <StepShell title="Your hotkeys">
      <p className="text-neutral-600 dark:text-neutral-300">
        Hold to talk: press and keep holding, speak, then release.
      </p>
      <div className="flex flex-col gap-2">
        {profiles.map((p) => (
          <div
            key={p.name}
            className="flex items-center justify-between rounded-lg border border-neutral-200 px-4 py-3 dark:border-neutral-800"
          >
            <div>
              <div className="text-sm font-medium">{p.name}</div>
              <div className="text-xs text-neutral-500">{p.detail}</div>
            </div>
            <kbd className="rounded-md border border-neutral-300 bg-neutral-100 px-2.5 py-1 text-sm text-neutral-600 dark:border-neutral-700 dark:bg-neutral-800 dark:text-neutral-300">
              {p.keys}
            </kbd>
          </div>
        ))}
      </div>
      <p className="text-xs text-neutral-400">
        Hotkeys can be changed later in the Dictation section.
      </p>
    </StepShell>
  );
}

function TryItStep() {
  useEffect(() => {
    attachDictationEvents();
  }, []);
  const phase = useDictationStore((s) => s.phase);
  const last = useDictationStore((s) => s.last);

  return (
    <StepShell title="Try it now">
      <p className="text-neutral-600 dark:text-neutral-300">
        Click into the box below, hold <kbd className="font-sans">⌥ Space</kbd>,
        say something in Hebrew, and release.
      </p>
      <textarea
        autoFocus
        dir="auto"
        placeholder="…"
        className="selectable h-28 w-full resize-none rounded-lg border border-neutral-300 bg-white p-3 text-[15px] focus:border-accent focus:outline-none dark:border-neutral-700 dark:bg-neutral-800"
      />
      <div className="text-sm text-neutral-500">
        {phase === "listening" && "Listening…"}
        {phase === "transcribing" && "Transcribing…"}
        {phase === "translating" && "Translating…"}
        {(phase === "pasted" || phase === "copied") && last && (
          <span className="text-emerald-600 dark:text-emerald-400">
            Worked! Decoded in {last.decodeMs} ms.
          </span>
        )}
        {phase === "idle" && !last && "Waiting for your first dictation."}
      </div>
    </StepShell>
  );
}
