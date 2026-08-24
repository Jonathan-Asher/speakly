import { useEffect } from "react";
import { strings } from "../../lib/strings";
import { attachDictationEvents, useDictationStore } from "../../stores/dictation";

/**
 * The recording pill. Lives in its own tiny always-on-top non-activating
 * window; states are driven by `dictation://state` engine events.
 */
export function HudPill() {
  useEffect(() => {
    attachDictationEvents();
  }, []);
  const phase = useDictationStore((s) => s.phase);
  const warning = useDictationStore((s) => s.warning);

  return (
    <div className="flex h-screen items-end justify-center bg-transparent pb-1">
      <div className="flex items-center gap-2.5 rounded-full bg-neutral-900/90 px-4 py-2 text-white shadow-lg backdrop-blur">
        {phase === "listening" && (
          <>
            <span className="relative flex size-2.5">
              <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-red-400 opacity-75" />
              <span className="relative inline-flex size-2.5 rounded-full bg-red-500" />
            </span>
            <span className="text-sm font-medium">{strings.hud.listening}</span>
          </>
        )}
        {(phase === "transcribing" || phase === "pasting") && (
          <>
            <span className="size-3 animate-spin rounded-full border-2 border-neutral-500 border-t-white" />
            <span className="text-sm font-medium">{strings.hud.transcribing}</span>
          </>
        )}
        {phase === "pasted" && (
          <>
            <span className="text-emerald-400">✓</span>
            <span className="text-sm font-medium">{strings.hud.pasted}</span>
          </>
        )}
        {phase === "copied" && (
          <>
            <span className="text-amber-300">⧉</span>
            <span className="text-sm font-medium">Copied — press ⌘V</span>
          </>
        )}
        {phase === "error" && (
          <>
            <span className="text-red-400">!</span>
            <span className="max-w-56 truncate text-sm font-medium">
              {warning ?? "Something went wrong"}
            </span>
          </>
        )}
        {phase === "idle" && <span className="text-sm text-neutral-400">…</span>}
      </div>
    </div>
  );
}
