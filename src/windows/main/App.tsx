import { useState } from "react";
import { strings } from "../../lib/strings";
import { DirectionalText } from "../../components/DirectionalText";

type Section = keyof typeof strings.nav;

const sections = Object.keys(strings.nav) as Section[];

export function App() {
  const [active, setActive] = useState<Section>("dictation");

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
      <main className="flex flex-1 items-center justify-center p-8">
        <div className="text-center text-neutral-400">
          <DirectionalText className="text-sm">
            {strings.nav[active]} — coming together in P1.
          </DirectionalText>
        </div>
      </main>
    </div>
  );
}
