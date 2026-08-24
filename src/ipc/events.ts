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
