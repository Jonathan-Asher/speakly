import { listen } from "@tauri-apps/api/event";

/** Payloads for engine/app events. Hand-typed for now; specta-generated
 * bindings replace this file when the command surface grows. */

export type DictationPhase =
  | "idle"
  | "listening"
  | "transcribing"
  | "translating"
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
}

export interface EngineWarningEvent {
  code: string;
  message: string;
}

export function onDictationState(cb: (e: DictationStateEvent) => void) {
  return listen<DictationStateEvent>("dictation://state", (e) => cb(e.payload));
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

export interface MeetingSegmentEvent {
  sessionId: number;
  t0Ms: number;
  t1Ms: number;
  text: string;
  source: string;
}

export interface MeetingStatusEvent {
  sessionId: number;
  state: "starting" | "live" | "stopped" | "error";
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
