import { useState } from "react";

/** Six accessible speaker hues, stable per label, legible in both themes. */
const PALETTE = [
  "bg-indigo-100 text-indigo-800 dark:bg-indigo-900/60 dark:text-indigo-200",
  "bg-emerald-100 text-emerald-800 dark:bg-emerald-900/60 dark:text-emerald-200",
  "bg-amber-100 text-amber-800 dark:bg-amber-900/60 dark:text-amber-200",
  "bg-sky-100 text-sky-800 dark:bg-sky-900/60 dark:text-sky-200",
  "bg-rose-100 text-rose-800 dark:bg-rose-900/60 dark:text-rose-200",
  "bg-violet-100 text-violet-800 dark:bg-violet-900/60 dark:text-violet-200",
];

export function speakerColor(label: string) {
  let hash = 0;
  for (const ch of label) hash = (hash * 31 + ch.codePointAt(0)!) >>> 0;
  return PALETTE[hash % PALETTE.length];
}

/**
 * A speaker label chip. When `onRename` is provided, clicking it opens an
 * inline rename input (Enter commits, Escape cancels).
 */
export function SpeakerChip({
  label,
  onRename,
}: {
  label: string;
  onRename?: (to: string) => void;
}) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(label);

  if (editing && onRename) {
    return (
      <input
        autoFocus
        dir="auto"
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            const to = draft.trim();
            if (to && to !== label) onRename(to);
            setEditing(false);
          }
          if (e.key === "Escape") {
            setDraft(label);
            setEditing(false);
          }
        }}
        onBlur={() => {
          setDraft(label);
          setEditing(false);
        }}
        className="w-28 rounded-full border border-neutral-300 bg-white px-2 py-0.5 text-xs dark:border-neutral-600 dark:bg-neutral-800"
      />
    );
  }
  return (
    <button
      type="button"
      onClick={onRename ? () => setEditing(true) : undefined}
      title={onRename ? "Rename speaker" : undefined}
      className={`rounded-full px-2 py-0.5 text-xs font-medium ${speakerColor(label)} ${
        onRename ? "cursor-pointer hover:opacity-80" : "cursor-default"
      }`}
    >
      {label}
    </button>
  );
}
