import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { DirectionalText } from "../../components/DirectionalText";

interface HistoryItem {
  id: number;
  kind: string;
  createdAt: number;
  durationMs: number;
  language: string | null;
  text: string;
  translatedText: string | null;
}

interface SearchResult {
  items: HistoryItem[];
  hasMore: boolean;
}

interface StoredSegment {
  startMs: number;
  endMs: number;
  speaker: string | null;
  text: string;
}

const KIND_FILTERS = [
  { id: null, label: "All" },
  { id: "dictation", label: "Dictation" },
  { id: "file", label: "Files" },
  { id: "meeting", label: "Meetings" },
] as const;

const relative = new Intl.RelativeTimeFormat(undefined, { numeric: "auto" });

function relativeTime(ms: number) {
  const diff = ms - Date.now();
  const minutes = Math.round(diff / 60_000);
  if (Math.abs(minutes) < 60) return relative.format(minutes, "minute");
  const hours = Math.round(minutes / 60);
  if (Math.abs(hours) < 24) return relative.format(hours, "hour");
  return relative.format(Math.round(hours / 24), "day");
}

function mmss(ms: number) {
  const total = Math.floor(ms / 1000);
  return `${String(Math.floor(total / 60)).padStart(2, "0")}:${String(total % 60).padStart(2, "0")}`;
}

/** Timestamped, speaker-labeled detail for stored file/meeting transcripts. */
function SegmentDetail({ item }: { item: HistoryItem }) {
  const [segments, setSegments] = useState<StoredSegment[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    invoke<StoredSegment[]>("history_segments", { transcriptId: item.id })
      .then(setSegments)
      .catch((e) => setError(String(e)));
  }, [item.id]);

  const doExport = async (format: "txt" | "srt") => {
    if (!segments || segments.length === 0) return;
    await invoke("export_transcript", {
      format,
      suggestedName: `${item.kind}-${item.id}`,
      segments: segments.map(({ startMs, endMs, text }) => ({ startMs, endMs, text })),
    });
  };

  if (error) return <div className="mt-2 text-xs text-red-500">{error}</div>;
  if (!segments) return <div className="mt-2 text-xs text-neutral-400">Loading…</div>;
  if (segments.length === 0) {
    // Older rows predate per-segment storage; the joined text above stands.
    return null;
  }

  return (
    <div className="mt-2 border-t border-neutral-200 pt-2 dark:border-neutral-800">
      <div className="mb-2 flex justify-end gap-2">
        {(["txt", "srt"] as const).map((f) => (
          <button
            key={f}
            onClick={() => void doExport(f)}
            className="rounded border border-neutral-300 px-2 py-0.5 text-[11px] uppercase text-neutral-500 hover:bg-neutral-100 dark:border-neutral-700 dark:hover:bg-neutral-800"
          >
            {f}
          </button>
        ))}
      </div>
      <div className="flex max-h-72 flex-col gap-1.5 overflow-y-auto">
        {segments.map((s, i) => (
          <div key={i} className="flex gap-2 text-sm">
            <span className="w-11 shrink-0 pt-0.5 text-end font-mono text-[11px] text-neutral-400">
              {mmss(s.startMs)}
            </span>
            <DirectionalText className="selectable min-w-0 flex-1 leading-relaxed">
              {s.speaker ? `${s.speaker}: ${s.text}` : s.text}
            </DirectionalText>
          </div>
        ))}
      </div>
    </div>
  );
}

export function HistoryView() {
  const [query, setQuery] = useState("");
  const [kind, setKind] = useState<string | null>(null);
  const [items, setItems] = useState<HistoryItem[]>([]);
  const [hasMore, setHasMore] = useState(false);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [expanded, setExpanded] = useState<Set<number>>(new Set());
  const pageRef = useRef(0);
  const debounceRef = useRef<ReturnType<typeof setTimeout>>(undefined);

  const load = useCallback(
    async (q: string, k: string | null, page: number, append: boolean) => {
      setLoading(true);
      setError(null);
      try {
        const result = await invoke<SearchResult>("history_search", {
          query: q.trim() ? q.trim() : null,
          kind: k,
          page,
        });
        pageRef.current = page;
        setHasMore(result.hasMore);
        setItems((prev) => (append ? [...prev, ...result.items] : result.items));
      } catch (e) {
        setError(String(e));
      } finally {
        setLoading(false);
      }
    },
    [],
  );

  useEffect(() => {
    void load("", null, 0, false);
  }, [load]);

  const onQueryChange = (q: string) => {
    setQuery(q);
    clearTimeout(debounceRef.current);
    debounceRef.current = setTimeout(() => void load(q, kind, 0, false), 250);
  };

  const onKindChange = (k: string | null) => {
    setKind(k);
    void load(query, k, 0, false);
  };

  const onDelete = async (id: number) => {
    await invoke("history_delete", { id });
    setItems((prev) => prev.filter((i) => i.id !== id));
  };

  const onClear = async () => {
    if (!window.confirm("Delete all history? This cannot be undone.")) return;
    await invoke("history_clear");
    setItems([]);
    setHasMore(false);
  };

  const toggle = (id: number) =>
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  return (
    <div className="flex w-full max-w-2xl flex-col gap-4">
      <div className="flex items-center gap-2">
        <input
          dir="auto"
          value={query}
          onChange={(e) => onQueryChange(e.target.value)}
          placeholder="Search history…"
          className="w-full rounded-lg border border-neutral-300 bg-white px-3 py-2 text-sm outline-none focus:border-accent dark:border-neutral-700 dark:bg-neutral-800"
        />
        {items.length > 0 && (
          <button
            onClick={() => void onClear()}
            className="shrink-0 rounded-lg border border-neutral-300 px-3 py-2 text-sm text-neutral-500 hover:border-red-400 hover:text-red-500 dark:border-neutral-700"
          >
            Clear all
          </button>
        )}
      </div>

      <div className="flex gap-1.5">
        {KIND_FILTERS.map((f) => (
          <button
            key={f.label}
            onClick={() => onKindChange(f.id)}
            className={`rounded-full px-3 py-1 text-xs transition-colors ${
              kind === f.id
                ? "bg-accent/15 font-medium text-accent-strong dark:text-accent"
                : "bg-neutral-100 text-neutral-500 hover:bg-neutral-200 dark:bg-neutral-800 dark:hover:bg-neutral-700"
            }`}
          >
            {f.label}
          </button>
        ))}
      </div>

      {error && <div className="text-sm text-red-500">{error}</div>}

      {items.length === 0 && !loading && !error && (
        <div className="rounded-xl border border-dashed border-neutral-300 p-8 text-center text-sm text-neutral-400 dark:border-neutral-700">
          {query.trim() || kind ? "No results." : "Dictations you make will appear here."}
        </div>
      )}

      <ul className="flex flex-col gap-2">
        {items.map((item) => {
          const isOpen = expanded.has(item.id);
          const hasSegments = item.kind === "file" || item.kind === "meeting";
          return (
            <li
              key={item.id}
              className="group rounded-xl border border-neutral-200 p-3 dark:border-neutral-800"
            >
              <div className="mb-1 flex items-center gap-2 text-xs text-neutral-500">
                <span>{relativeTime(item.createdAt)}</span>
                {item.language && (
                  <span className="rounded bg-neutral-100 px-1.5 py-0.5 dark:bg-neutral-800">
                    {item.language}
                  </span>
                )}
                <span className="rounded bg-neutral-100 px-1.5 py-0.5 dark:bg-neutral-800">
                  {item.kind}
                </span>
                <span className="ms-auto flex items-center gap-2">
                  <button
                    onClick={() => void navigator.clipboard.writeText(item.text)}
                    className="opacity-0 transition-opacity group-hover:opacity-100"
                    title="Copy"
                  >
                    ⧉
                  </button>
                  <button
                    onClick={() => void onDelete(item.id)}
                    className="opacity-0 transition-opacity hover:text-red-500 group-hover:opacity-100"
                    title="Delete"
                  >
                    ✕
                  </button>
                </span>
              </div>
              <button className="block w-full text-start" onClick={() => toggle(item.id)}>
                <DirectionalText
                  className={`selectable text-sm leading-relaxed ${isOpen ? "" : "line-clamp-2"}`}
                >
                  {item.text}
                </DirectionalText>
              </button>
              {item.translatedText && isOpen && (
                <DirectionalText className="selectable mt-2 border-t border-neutral-200 pt-2 text-sm text-neutral-500 dark:border-neutral-800">
                  {item.translatedText}
                </DirectionalText>
              )}
              {hasSegments && isOpen && <SegmentDetail item={item} />}
            </li>
          );
        })}
      </ul>

      {hasMore && (
        <button
          disabled={loading}
          onClick={() => void load(query, kind, pageRef.current + 1, true)}
          className="rounded-lg border border-neutral-300 px-3 py-2 text-sm text-neutral-600 hover:bg-neutral-100 disabled:opacity-50 dark:border-neutral-700 dark:text-neutral-300 dark:hover:bg-neutral-800"
        >
          {loading ? "Loading…" : "Load more"}
        </button>
      )}
    </div>
  );
}
