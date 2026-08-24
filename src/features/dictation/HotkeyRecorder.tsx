import { useEffect, useState } from "react";
import { eventToAccelerator, prettyHotkey } from "../../lib/hotkey";

/**
 * Click to record the next key combo. Esc cancels. Bare modifiers are
 * rejected (modifier-only hotkeys like Right-Option come later).
 */
export function HotkeyRecorder({
  value,
  onChange,
}: {
  value: string;
  onChange: (accelerator: string) => void;
}) {
  const [recording, setRecording] = useState(false);

  useEffect(() => {
    if (!recording) return;
    const onKeyDown = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.key === "Escape") {
        setRecording(false);
        return;
      }
      const accelerator = eventToAccelerator(e);
      if (accelerator) {
        onChange(accelerator);
        setRecording(false);
      }
    };
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
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
        {recording ? "Press a key combo…" : prettyHotkey(value) || "Record hotkey"}
      </button>
      {recording && (
        <span className="text-xs text-neutral-500">
          Esc cancels · modifier-only hotkeys come later
        </span>
      )}
    </div>
  );
}
