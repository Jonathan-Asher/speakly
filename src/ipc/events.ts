import { listen } from "@tauri-apps/api/event";

/** Payloads for engine/app events. Hand-typed for now; specta-generated
 * bindings replace this file when the command surface grows. */

export type DictationPhase =
  | "idle"
  | "listening"
  | "transcribing"
  | "translating"
  | "refining"
  | "pasting"
  | "pasted"
  | "copied"
  | "error";

export interface DictationStateEvent {
  phase: DictationPhase;
  profileId: string;
}

export interface DictationFinalEvent {
  profileId: string;
  text: string;
  phase: "pasted" | "copied";
  note: string | null;
  utteranceMs: number;
  decodeMs: number;
  latencyMs: number;
  translateMs: number | null;
  translated: boolean;
  refined: boolean;
}

export interface EngineWarningEvent {
  code: string;
  message: string;
}

export function onDictationState(cb: (e: DictationStateEvent) => void) {
  return listen<DictationStateEvent>("dictation://state", (e) => cb(e.payload));
}

export interface DictationPartialEvent {
  profileId: string;
  committed: string;
  volatile: string;
}

/** Live transcript while dictating: committed text is stable, volatile is the
 * open window's current best guess. */
export function onDictationPartial(cb: (e: DictationPartialEvent) => void) {
  return listen<DictationPartialEvent>("dictation://partial", (e) => cb(e.payload));
}

export function onDictationFinal(cb: (e: DictationFinalEvent) => void) {
  return listen<DictationFinalEvent>("dictation://final", (e) => cb(e.payload));
}

export function onEngineWarning(cb: (e: EngineWarningEvent) => void) {
  return listen<EngineWarningEvent>("engine://warning", (e) => cb(e.payload));
}

export interface ModelProgressEvent {
  id: string;
  bytes: number;
  total: number | null;
  bps: number;
}

export interface ModelReadyEvent {
  id: string;
  path: string;
}

export interface ModelErrorEvent {
  id: string;
  message: string;
}

export function onModelProgress(cb: (e: ModelProgressEvent) => void) {
  return listen<ModelProgressEvent>("model://progress", (e) => cb(e.payload));
}

export function onModelReady(cb: (e: ModelReadyEvent) => void) {
  return listen<ModelReadyEvent>("model://ready", (e) => cb(e.payload));
}

export function onModelError(cb: (e: ModelErrorEvent) => void) {
  return listen<ModelErrorEvent>("model://error", (e) => cb(e.payload));
}

/** Fired after any settings mutation (profile CRUD etc.); listeners refetch. */
export function onSettingsChanged(cb: () => void) {
  return listen("settings://changed", () => cb());
}

export interface TranscriptSegment {
  startMs: number;
  endMs: number;
  speaker: string | null;
  text: string;
}

export interface JobProgressEvent {
  id: string;
  stage: "decoding" | "transcribing";
  pct: number;
}

export interface JobSegmentEvent {
  id: string;
  segment: TranscriptSegment;
}

export interface JobDoneEvent {
  id: string;
  durationMs: number;
}

export interface JobErrorEvent {
  id: string;
  message: string;
}

export function onJobProgress(cb: (e: JobProgressEvent) => void) {
  return listen<JobProgressEvent>("job://progress", (e) => cb(e.payload));
}

export function onJobSegment(cb: (e: JobSegmentEvent) => void) {
  return listen<JobSegmentEvent>("job://segment", (e) => cb(e.payload));
}

export function onJobDone(cb: (e: JobDoneEvent) => void) {
  return listen<JobDoneEvent>("job://done", (e) => cb(e.payload));
}

export function onJobError(cb: (e: JobErrorEvent) => void) {
  return listen<JobErrorEvent>("job://error", (e) => cb(e.payload));
}

export function onJobCancelled(cb: (e: { id: string }) => void) {
  return listen<{ id: string }>("job://cancelled", (e) => cb(e.payload));
}

export interface JobSegmentsRelabeledEvent {
  id: string;
  segments: TranscriptSegment[];
}

export function onJobSegmentsRelabeled(cb: (e: JobSegmentsRelabeledEvent) => void) {
  return listen<JobSegmentsRelabeledEvent>("job://segments-relabeled", (e) =>
    cb(e.payload),
  );
}

export interface JobPersistedEvent {
  id: string;
  transcriptId: number;
}

export function onJobPersisted(cb: (e: JobPersistedEvent) => void) {
  return listen<JobPersistedEvent>("job://persisted", (e) => cb(e.payload));
}

export interface MeetingRelabeledEvent {
  sessionId: number;
  segments: TranscriptSegment[];
}

export function onMeetingRelabeled(cb: (e: MeetingRelabeledEvent) => void) {
  return listen<MeetingRelabeledEvent>("meeting://relabeled", (e) => cb(e.payload));
}

export interface MeetingPersistedEvent {
  sessionId: number;
  transcriptId: number;
}

export function onMeetingPersisted(cb: (e: MeetingPersistedEvent) => void) {
  return listen<MeetingPersistedEvent>("meeting://persisted", (e) => cb(e.payload));
}

export interface MeetingSegmentEvent {
  sessionId: number;
  t0Ms: number;
  t1Ms: number;
  text: string;
  source: string;
}

export interface MeetingStatusEvent {
  sessionId: number;
  state: "starting" | "live" | "stopped" | "diarizing" | "error";
  message: string | null;
}

export interface MeetingFinishedEvent {
  sessionId: number;
  saved: boolean;
  durationMs: number;
}

export function onMeetingSegment(cb: (e: MeetingSegmentEvent) => void) {
  return listen<MeetingSegmentEvent>("meeting://segment", (e) => cb(e.payload));
}

export function onMeetingStatus(cb: (e: MeetingStatusEvent) => void) {
  return listen<MeetingStatusEvent>("meeting://status", (e) => cb(e.payload));
}

export function onMeetingFinished(cb: (e: MeetingFinishedEvent) => void) {
  return listen<MeetingFinishedEvent>("meeting://finished", (e) => cb(e.payload));
}
