import { create } from "zustand";
import {
  onJobCancelled,
  onJobDone,
  onJobError,
  onJobPersisted,
  onJobProgress,
  onJobSegment,
  onJobSegmentsRelabeled,
  type TranscriptSegment,
} from "../ipc/events";

export type JobStatus =
  | "queued"
  | "decoding"
  | "transcribing"
  | "diarizing"
  | "done"
  | "error"
  | "cancelled";

export interface FileJob {
  id: string;
  path: string;
  fileName: string;
  language: string;
  modelId: string;
  status: JobStatus;
  pct: number;
  error: string | null;
  segments: TranscriptSegment[];
  /** History row id once persisted; enables speaker renames to stick. */
  transcriptId: number | null;
}

interface JobsStore {
  jobs: Record<string, FileJob>;
  order: string[];
  selectedId: string | null;
  select: (id: string | null) => void;
  addQueued: (
    entries: { id: string; path: string; language: string; modelId: string }[],
  ) => void;
  remove: (id: string) => void;
  clearCompleted: () => void;
}

const baseName = (p: string) => p.split("/").pop() ?? p;

export const useJobsStore = create<JobsStore>((set) => ({
  jobs: {},
  order: [],
  selectedId: null,
  select: (id) => set({ selectedId: id }),
  addQueued: (entries) =>
    set((s) => {
      const jobs = { ...s.jobs };
      const order = [...s.order];
      for (const e of entries) {
        jobs[e.id] = {
          id: e.id,
          path: e.path,
          fileName: baseName(e.path),
          language: e.language,
          modelId: e.modelId,
          status: "queued",
          pct: 0,
          error: null,
          segments: [],
          transcriptId: null,
        };
        order.push(e.id);
      }
      return { jobs, order, selectedId: s.selectedId ?? entries[0]?.id ?? null };
    }),
  remove: (id) =>
    set((s) => {
      const jobs = { ...s.jobs };
      delete jobs[id];
      return {
        jobs,
        order: s.order.filter((x) => x !== id),
        selectedId: s.selectedId === id ? null : s.selectedId,
      };
    }),
  clearCompleted: () =>
    set((s) => {
      const keep = (id: string) =>
        s.jobs[id] && !["done", "error", "cancelled"].includes(s.jobs[id].status);
      const jobs: Record<string, FileJob> = {};
      const order = s.order.filter(keep);
      for (const id of order) jobs[id] = s.jobs[id];
      return {
        jobs,
        order,
        selectedId: s.selectedId && keep(s.selectedId) ? s.selectedId : null,
      };
    }),
}));

function patch(id: string, fields: Partial<FileJob>) {
  useJobsStore.setState((s) => {
    const job = s.jobs[id];
    if (!job) return s;
    return { jobs: { ...s.jobs, [id]: { ...job, ...fields } } };
  });
}

let attached = false;

/** Subscribe this window to job events; idempotent per window. */
export function attachJobEvents() {
  if (attached) return;
  attached = true;
  void onJobProgress((e) => patch(e.id, { status: e.stage, pct: e.pct }));
  void onJobSegment((e) => {
    useJobsStore.setState((s) => {
      const job = s.jobs[e.id];
      if (!job) return s;
      return {
        jobs: {
          ...s.jobs,
          [e.id]: { ...job, segments: [...job.segments, e.segment] },
        },
      };
    });
  });
  void onJobSegmentsRelabeled((e) => patch(e.id, { segments: e.segments }));
  void onJobPersisted((e) => patch(e.id, { transcriptId: e.transcriptId }));
  void onJobDone((e) => patch(e.id, { status: "done", pct: 1 }));
  void onJobError((e) => patch(e.id, { status: "error", error: e.message }));
  void onJobCancelled((e) => patch(e.id, { status: "cancelled" }));
}

/** Apply a speaker rename to a job's segments in place (view state). */
export function renameSpeakerLocal(jobId: string, from: string, to: string) {
  useJobsStore.setState((s) => {
    const job = s.jobs[jobId];
    if (!job) return s;
    const segments = job.segments.map((seg) =>
      seg.speaker === from ? { ...seg, speaker: to } : seg,
    );
    return { jobs: { ...s.jobs, [jobId]: { ...job, segments } } };
  });
}
