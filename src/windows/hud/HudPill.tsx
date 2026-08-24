import { strings } from "../../lib/strings";

/**
 * The recording pill. Lives in its own tiny always-on-top non-activating
 * window; states are driven by `dictation://state` engine events (wired in
 * P1 — this is the static shell).
 */
export function HudPill() {
  return (
    <div className="flex h-screen items-center justify-center bg-transparent">
      <div className="flex items-center gap-2.5 rounded-full bg-neutral-900/90 px-4 py-2 text-white shadow-lg backdrop-blur">
        <span className="relative flex size-2.5">
          <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-red-400 opacity-75" />
          <span className="relative inline-flex size-2.5 rounded-full bg-red-500" />
        </span>
        <span className="text-sm font-medium">{strings.hud.listening}</span>
      </div>
    </div>
  );
}
