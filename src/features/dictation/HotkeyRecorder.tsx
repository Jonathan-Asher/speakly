import { useEffect, useRef, useState } from "react";
import { bareModifierFromEvent, eventToAccelerator, prettyHotkey } from "../../lib/hotkey";

/**
 * Click to record the next key combo. Esc cancels. Pressing and releasing a
 * lone supported modifier (Right ⌥, Left ⌥, Right ⌘) records a bare-modifier
 * hold binding; combining it with a key records a normal combo instead.
 */
export function HotkeyRecorder({
  value,
  onChange,
}: {
  value: string;
  onChange: (accelerator: string) => void;
}) {
  const [recording, setRecording] = useState(false);
  // A lone-modifier press being tracked; cleared when any other key joins.
  const pendingBare = useRef<string | null>(null);

  useEffect(() => {
    if (!recording) return;
    const onKeyDown = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.key === "Escape") {
        pendingBare.current = null;
        setRecording(false);
        return;
      }
      const bare = bareModifierFromEvent(e);
      if (bare) {
        pendingBare.current = bare;
        return;
      }
      // Any non-modifier key cancels a pending bare hold and may finish a combo.
      pendingBare.current = null;
      const accelerator = eventToAccelerator(e);
      if (accelerator) {
        onChange(accelerator);
        setRecording(false);
      }
    };
    const onKeyUp = (e: KeyboardEvent) => {
      const bare = bareModifierFromEvent(e);
      if (bare && pendingBare.current === bare) {
        pendingBare.current = null;
        onChange(bare);
        setRecording(false);
      }
    };
    window.addEventListener("keydown", onKeyDown, true);
    window.addEventListener("keyup", onKeyUp, true);
    return () => {
      window.removeEventListener("keydown", onKeyDown, true);
      window.removeEventListener("keyup", onKeyUp, true);
    };
  }, [recording, onChange]);

  return (
    <div className="flex items-center gap-2">
      <button
        type="button"
        onClick={() => setRecording((r) => !r)}
        className={`rounded-md border px-3 py-1.5 text-sm transition-colors ${
          recording
            ? "border-accent bg-accent/10 text-accent-strong dark:text-accent"
            : "border-neutral-300 bg-neutral-100 text-neutral-700 hover:bg-neutral-200 dark:border-neutral-700 dark:bg-neutral-800 dark:text-neutral-200 dark:hover:bg-neutral-700"
        }`}
      >
        {recording ? "Press a combo, or tap a lone ⌥/⌘…" : prettyHotkey(value) || "Record hotkey"}
      </button>
      {recording && (
        <span className="text-xs text-neutral-500">
          Esc cancels · lone Right ⌥/⌘ makes a hold hotkey (needs Accessibility)
        </span>
      )}
    </div>
  );
}
