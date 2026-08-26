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
  const partial = useDictationStore((s) => s.partial);
  const hasPartial = !!partial && (partial.committed !== "" || partial.volatile !== "");

  return (
    <div className="flex h-screen items-end justify-center bg-transparent pb-1">
      <div className="flex items-center gap-2.5 rounded-full bg-neutral-900/90 px-4 py-2 text-white shadow-lg backdrop-blur">
        {phase === "listening" && (
          <>
            <span className="relative flex size-2.5 shrink-0">
              <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-red-400 opacity-75" />
              <span className="relative inline-flex size-2.5 rounded-full bg-red-500" />
            </span>
            {hasPartial ? (
              // dir=auto + flex-end keeps the *end* of the transcript visible
              // for both LTR and RTL text as it grows.
              <div dir="auto" className="flex max-w-[400px] justify-end overflow-hidden">
                <span
                  className="text-sm font-medium whitespace-nowrap"
                  style={{ unicodeBidi: "plaintext" }}
                >
                  {partial.committed}
                  {partial.committed && partial.volatile ? " " : ""}
                  {partial.volatile && (
                    <span className="italic text-neutral-400">{partial.volatile}</span>
                  )}
                </span>
              </div>
            ) : (
              <span className="text-sm font-medium">
                {strings.hud.listening}
                <span className="ms-2 text-xs font-normal text-neutral-500">esc cancels</span>
              </span>
            )}
          </>
        )}
        {(phase === "transcribing" || phase === "pasting") && (
          <>
            <span className="size-3 animate-spin rounded-full border-2 border-neutral-500 border-t-white" />
            <span className="text-sm font-medium">{strings.hud.transcribing}</span>
          </>
        )}
        {phase === "refining" && (
          <>
            <span className="size-3 animate-spin rounded-full border-2 border-violet-400 border-t-white" />
            <span className="text-sm font-medium">{strings.hud.refining}</span>
          </>
        )}
        {phase === "translating" && (
          <>
            <span className="size-3 animate-spin rounded-full border-2 border-indigo-400 border-t-white" />
            <span className="text-sm font-medium">{strings.hud.translating}</span>
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
            <span className="max-w-[380px] truncate text-sm font-medium">
              {warning ?? "Something went wrong"}
            </span>
          </>
        )}
        {phase === "idle" && <span className="text-sm text-neutral-400">…</span>}
      </div>
    </div>
  );
}
