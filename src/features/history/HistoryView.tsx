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

const relative = new Intl.RelativeTimeFormat(undefined, { numeric: "auto" });

function relativeTime(ms: number) {
  const diff = ms - Date.now();
  const minutes = Math.round(diff / 60_000);
  if (Math.abs(minutes) < 60) return relative.format(minutes, "minute");
  const hours = Math.round(minutes / 60);
  if (Math.abs(hours) < 24) return relative.format(hours, "hour");
  return relative.format(Math.round(hours / 24), "day");
}

export function HistoryView() {
  const [query, setQuery] = useState("");
  const [items, setItems] = useState<HistoryItem[]>([]);
  const [hasMore, setHasMore] = useState(false);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [expanded, setExpanded] = useState<Set<number>>(new Set());
  const pageRef = useRef(0);
  const debounceRef = useRef<ReturnType<typeof setTimeout>>(undefined);

  const load = useCallback(async (q: string, page: number, append: boolean) => {
    setLoading(true);
    setError(null);
    try {
      const result = await invoke<SearchResult>("history_search", {
        query: q.trim() ? q.trim() : null,
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
  }, []);

  useEffect(() => {
    void load("", 0, false);
  }, [load]);

  const onQueryChange = (q: string) => {
    setQuery(q);
    clearTimeout(debounceRef.current);
    debounceRef.current = setTimeout(() => void load(q, 0, false), 250);
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

      {error && <div className="text-sm text-red-500">{error}</div>}

      {items.length === 0 && !loading && !error && (
        <div className="rounded-xl border border-dashed border-neutral-300 p-8 text-center text-sm text-neutral-400 dark:border-neutral-700">
          {query.trim()
            ? "No results."
            : "Dictations you make will appear here."}
        </div>
      )}

      <ul className="flex flex-col gap-2">
        {items.map((item) => {
          const isOpen = expanded.has(item.id);
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
            </li>
          );
        })}
      </ul>

      {hasMore && (
        <button
          disabled={loading}
          onClick={() => void load(query, pageRef.current + 1, true)}
          className="rounded-lg border border-neutral-300 px-3 py-2 text-sm text-neutral-600 hover:bg-neutral-100 disabled:opacity-50 dark:border-neutral-700 dark:text-neutral-300 dark:hover:bg-neutral-800"
        >
          {loading ? "Loading…" : "Load more"}
        </button>
      )}
    </div>
  );
}
