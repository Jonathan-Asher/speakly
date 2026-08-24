import { create } from "zustand";
import {
  onDictationFinal,
  onDictationPartial,
  onDictationState,
  onEngineWarning,
  type DictationFinalEvent,
  type DictationPartialEvent,
  type DictationPhase,
} from "../ipc/events";

interface DictationStore {
  phase: DictationPhase;
  profileId: string | null;
  /** Live transcript while listening; null outside a dictation. */
  partial: DictationPartialEvent | null;
  last: DictationFinalEvent | null;
  warning: string | null;
}

export const useDictationStore = create<DictationStore>(() => ({
  phase: "idle",
  profileId: null,
  partial: null,
  last: null,
  warning: null,
}));

let attached = false;

/** Subscribe this window to engine events; idempotent per window. */
export function attachDictationEvents() {
  if (attached) return;
  attached = true;
  void onDictationState((e) => {
    useDictationStore.setState({
      phase: e.phase,
      profileId: e.profileId,
      ...(e.phase === "listening" ? { warning: null, partial: null } : {}),
      ...(e.phase === "idle" ? { partial: null } : {}),
    });
  });
  void onDictationPartial((e) => {
    useDictationStore.setState({ partial: e });
  });
  void onDictationFinal((e) => {
    useDictationStore.setState({ last: e, partial: null });
  });
  void onEngineWarning((e) => {
    useDictationStore.setState({ warning: e.message });
  });
}
