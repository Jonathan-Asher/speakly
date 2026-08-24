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

/** Fired after any settings mutation (profile CRUD etc.); listeners refetch. */
export function onSettingsChanged(cb: () => void) {
  return listen("settings://changed", () => cb());
}
