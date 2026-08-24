import { useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { DirectionalText } from "../../components/DirectionalText";
import { SpeakerChip } from "../../components/SpeakerChip";
import { buildDocx } from "../../lib/exporters/docx";
import { buildPdf } from "../../lib/exporters/pdf";
import { saveBinary } from "../../lib/exporters/save";
import { renameSpeakerLocal, type FileJob } from "../../stores/jobs";

const EXPORT_FORMATS = ["txt", "md", "srt", "vtt"] as const;
const RICH_FORMATS = ["docx", "pdf"] as const;

function mmss(ms: number) {
  const total = Math.floor(ms / 1000);
  const m = Math.floor(total / 60);
  const s = total % 60;
  return `${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
}

/** Highlight `query` inside `text` (case-insensitive, plain substring). */
function Highlighted({ text, query }: { text: string; query: string }) {
  if (!query) return <>{text}</>;
  const lower = text.toLowerCase();
  const q = query.toLowerCase();
  const parts: React.ReactNode[] = [];
  let pos = 0;
  let hit = lower.indexOf(q);
  while (hit >= 0) {
    if (hit > pos) parts.push(text.slice(pos, hit));
    parts.push(
      <mark key={hit} className="rounded-sm bg-amber-200 dark:bg-amber-700/60">
        {text.slice(hit, hit + q.length)}
      </mark>,
    );
    pos = hit + q.length;
    hit = lower.indexOf(q, pos);
  }
  parts.push(text.slice(pos));
  return <>{parts}</>;
}

export function TranscriptView({ job }: { job: FileJob }) {
  const [query, setQuery] = useState("");
  const [notice, setNotice] = useState<string | null>(null);

  const visible = useMemo(() => {
    if (!query) return job.segments;
    const q = query.toLowerCase();
    return job.segments.filter((s) => s.text.toLowerCase().includes(q));
  }, [job.segments, query]);

  const copyAll = async () => {
    await navigator.clipboard.writeText(job.segments.map((s) => s.text).join("\n"));
    flash("Copied");
  };

  const flash = (msg: string) => {
    setNotice(msg);
    window.setTimeout(() => setNotice(null), 2000);
  };

  const renameSpeaker = (from: string, to: string) => {
    renameSpeakerLocal(job.id, from, to);
    if (job.transcriptId != null) {
      invoke("rename_speaker", { transcriptId: job.transcriptId, from, to }).catch(
        (e) => flash(String(e)),
      );
    }
  };

  const doExport = async (format: string) => {
    try {
      const saved = await invoke<string | null>("export_transcript", {
        format,
        suggestedName: job.fileName.replace(/\.[^.]+$/, "") || "transcript",
        segments: job.segments.map(({ startMs, endMs, speaker, text }) => ({
          startMs,
          endMs,
          speaker,
          text,
        })),
      });
      if (saved) flash(`Saved ${format.toUpperCase()}`);
    } catch (e) {
      flash(String(e));
    }
  };

  const doRichExport = async (format: (typeof RICH_FORMATS)[number]) => {
    try {
      const name = job.fileName.replace(/\.[^.]+$/, "") || "transcript";
      const opts = { title: name, timestamps: true };
      const bytes =
        format === "docx"
          ? await buildDocx(job.segments, opts)
          : buildPdf(job.segments, opts);
      const saved = await saveBinary(bytes, name, format, format.toUpperCase());
      if (saved) flash(`Saved ${format.toUpperCase()}`);
    } catch (e) {
      flash(String(e));
    }
  };

  return (
    <div className="flex h-full flex-col gap-3">
      <div className="flex items-center gap-2">
        <input
          dir="auto"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="Search transcript…"
          className="min-w-0 flex-1 rounded-md border border-neutral-300 bg-white px-3 py-1.5 text-sm dark:border-neutral-700 dark:bg-neutral-800"
        />
        <button
          onClick={() => void copyAll()}
          disabled={job.segments.length === 0}
          className="rounded-md border border-neutral-300 px-3 py-1.5 text-sm text-neutral-600 hover:bg-neutral-100 disabled:opacity-40 dark:border-neutral-700 dark:text-neutral-300 dark:hover:bg-neutral-800"
        >
          Copy
        </button>
        {EXPORT_FORMATS.map((f) => (
          <button
            key={f}
            onClick={() => void doExport(f)}
            disabled={job.segments.length === 0}
            className="rounded-md border border-neutral-300 px-2.5 py-1.5 text-xs uppercase text-neutral-600 hover:bg-neutral-100 disabled:opacity-40 dark:border-neutral-700 dark:text-neutral-300 dark:hover:bg-neutral-800"
          >
            {f}
          </button>
        ))}
        {RICH_FORMATS.map((f) => (
          <button
            key={f}
            onClick={() => void doRichExport(f)}
            disabled={job.segments.length === 0}
            className="rounded-md border border-neutral-300 px-2.5 py-1.5 text-xs uppercase text-neutral-600 hover:bg-neutral-100 disabled:opacity-40 dark:border-neutral-700 dark:text-neutral-300 dark:hover:bg-neutral-800"
          >
            {f}
          </button>
        ))}
      </div>

      {notice && <div className="text-xs text-emerald-600 dark:text-emerald-400">{notice}</div>}

      <div className="min-h-0 flex-1 overflow-y-auto rounded-xl border border-neutral-200 p-4 dark:border-neutral-800">
        {job.segments.length === 0 && (
          <div className="py-8 text-center text-sm text-neutral-400">
            {job.status === "error"
              ? (job.error ?? "Transcription failed")
              : job.status === "queued"
                ? "Waiting in queue…"
                : "Transcribing — segments appear as they are ready."}
          </div>
        )}
        {query && visible.length === 0 && job.segments.length > 0 && (
          <div className="py-8 text-center text-sm text-neutral-400">No matches.</div>
        )}
        <div className="flex flex-col gap-2.5">
          {visible.map((s, i) => {
            const newSpeaker =
              s.speaker != null && (i === 0 || visible[i - 1].speaker !== s.speaker);
            return (
              <div key={`${s.startMs}-${i}`} className="flex gap-3">
                <span className="w-12 shrink-0 pt-0.5 text-end font-mono text-xs text-neutral-400">
                  {mmss(s.startMs)}
                </span>
                <div className="min-w-0 flex-1">
                  {newSpeaker && s.speaker && (
                    <div className="mb-0.5">
                      <SpeakerChip
                        label={s.speaker}
                        onRename={(to) => renameSpeaker(s.speaker!, to)}
                      />
                    </div>
                  )}
                  <DirectionalText className="selectable text-[15px] leading-relaxed">
                    <Highlighted text={s.text} query={query} />
                  </DirectionalText>
                </div>
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}
