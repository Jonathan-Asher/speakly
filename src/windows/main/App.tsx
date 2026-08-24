import { useEffect, useState } from "react";
import { strings } from "../../lib/strings";
import { DictationView } from "../../features/dictation/DictationView";
import { ModelsView } from "../../features/models/ModelsView";
import { HistoryView } from "../../features/history/HistoryView";
import { SettingsView } from "../../features/settings/SettingsView";
import { OnboardingWizard } from "../../features/onboarding/OnboardingWizard";
import { attachSettings, useSettingsStore } from "../../stores/settings";

type Section = keyof typeof strings.nav;

const sections = Object.keys(strings.nav) as Section[];

export function App() {
  const [active, setActive] = useState<Section>("dictation");
  const loaded = useSettingsStore((s) => s.loaded);
  const onboarded = useSettingsStore((s) => s.settings?.onboarding.completed ?? false);

  useEffect(() => {
    attachSettings();
  }, []);

  if (!loaded) return null;
  if (!onboarded) return <OnboardingWizard />;

  return (
    <div className="flex h-full">
      <nav className="flex w-52 shrink-0 flex-col gap-1 border-e border-neutral-200 p-3 pt-10 dark:border-neutral-800">
        <div className="mb-4 ps-2 text-lg font-semibold">{strings.appName}</div>
        {sections.map((s) => (
          <button
            key={s}
            onClick={() => setActive(s)}
            className={`rounded-md px-3 py-1.5 text-start text-sm transition-colors ${
              active === s
                ? "bg-accent/10 font-medium text-accent-strong dark:text-accent"
                : "text-neutral-600 hover:bg-neutral-100 dark:text-neutral-400 dark:hover:bg-neutral-800"
            }`}
          >
            {strings.nav[s]}
          </button>
        ))}
      </nav>
      <main className="flex flex-1 justify-center overflow-y-auto p-8 pt-12">
        {active === "dictation" ? (
          <DictationView />
        ) : active === "models" ? (
          <ModelsView />
        ) : active === "history" ? (
          <HistoryView />
        ) : active === "settings" ? (
          <SettingsView />
        ) : (
          <div className="self-center text-center text-sm text-neutral-400">
            {strings.nav[active]} — coming in the next phases.
          </div>
        )}
      </main>
    </div>
  );
}
